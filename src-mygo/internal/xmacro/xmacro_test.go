package xmacro

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"structsheets/mygo/internal/xlsxwrite"
)

func TestParsesByteRangesAndGeneratesExpectedMacro(t *testing.T) {
	candidate := &tableCandidate{
		sheetName: "Sheet1",
		rows: [][]string{
			{"Bytes", "Parameter Name", "Type"},
			{"0~3", "IP_GPM_value", "float"},
			{"5", "IP_GPM_status", "unsigned char"},
		},
		headerRow: 0,
		columns:   columns{offset: 0, hasOffset: true, name: 1, dataType: 2},
		score:     4,
	}
	rows, warnings, err := parseRows(candidate)
	if err != nil {
		t.Fatal(err)
	}
	if len(warnings) != 0 {
		t.Fatalf("warnings = %v", warnings)
	}
	if rows[0].Offset != 0 || rows[0].Size != 4 {
		t.Fatalf("row0 = %+v", rows[0])
	}
	// A single byte number is an offset; its size comes from the C type.
	if rows[1].Offset != 5 || rows[1].Size != 1 {
		t.Fatalf("row1 = %+v", rows[1])
	}
	code := generateCode("EI_FIELDS", "EI_TYPE", rows)
	if !strings.Contains(code, "X(float, IP_GPM_value, 0, 4)") {
		t.Fatalf("code = %s", code)
	}
	if !strings.Contains(code, "X(unsigned char, IP_GPM_status, 5, 1)") {
		t.Fatalf("code = %s", code)
	}
	if !strings.Contains(code, "EI_FIELDS(DECLARE_FIELD)") {
		t.Fatalf("code = %s", code)
	}
}

func TestConvertsACsvFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "table.csv")
	if err := os.WriteFile(path, []byte("Bytes,Parameter Name,Type\n0~3,value,float\n4,status,unsigned char\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	result, err := Convert(path, "", "TEST_FIELDS", "TEST_TYPE")
	if err != nil {
		t.Fatal(err)
	}
	if result.RowCount != 2 {
		t.Fatalf("rows = %d", result.RowCount)
	}
	if result.Rows[1].Size != 1 {
		t.Fatalf("size = %d", result.Rows[1].Size)
	}
}

func TestConvertsAnXlsxFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "table.xlsx")
	data, err := fixtureWorkbook()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatal(err)
	}
	result, err := Convert(path, "", "AIR_FIELDS", "AIR_TYPE")
	if err != nil {
		t.Fatal(err)
	}
	if result.SheetName != "Sheet1" {
		t.Fatalf("sheet = %q", result.SheetName)
	}
	if result.Rows[0].Offset != 0 || result.Rows[0].Size != 4 {
		t.Fatalf("row0 = %+v", result.Rows[0])
	}
}

func fixtureWorkbook() ([]byte, error) {
	table := xlsxwrite.SheetTable{
		Name: "Sheet1",
		Rows: [][]string{
			{"Bytes", "Parameter Name", "Type"},
			{"0~3", "airspeed", "float"},
		},
	}
	return xlsxwrite.BuildSimpleWorkbook(table)
}

func TestConvertsExcelClipboardText(t *testing.T) {
	result, err := Convert("", "Bytes\tParameter Name\tType\r\n0~3\tIP_GPM_airspeed\tfloat\r\n5\tIP_GPM_status\tunsigned char\r\n", "PFD_FIELDS", "PFD_TYPE")
	if err != nil {
		t.Fatal(err)
	}
	if result.SheetName != "粘贴内容" {
		t.Fatalf("sheet = %q", result.SheetName)
	}
	if result.RowCount != 2 {
		t.Fatalf("rows = %d", result.RowCount)
	}
	if !strings.Contains(result.Code, "X(unsigned char, IP_GPM_status, 5, 1)") {
		t.Fatalf("code = %s", result.Code)
	}
}

func TestCIdentifierNormalization(t *testing.T) {
	cases := []struct {
		value     string
		uppercase bool
		fallback  string
		want      string
	}{
		{"EI_TO_HMGPM", true, "F", "EI_TO_HMGPM"},
		{"ei to hmgpm", true, "F", "EI_TO_HMGPM"},
		{"2fast", false, "F", "_2fast"},
		{"__x__", false, "F", "x"},
		{"a--b", false, "F", "a_b"},
		{"###", false, "F", "F"},
	}
	for _, c := range cases {
		if got := cIdentifier(c.value, c.uppercase, c.fallback); got != c.want {
			t.Errorf("cIdentifier(%q, %v) = %q, want %q", c.value, c.uppercase, got, c.want)
		}
	}
}

func TestParseDelimitedHandlesQuotedCells(t *testing.T) {
	rows := parseDelimited("a,\"b,c\",d\ne,\"f\"\"g\",h", ',')
	if len(rows) != 2 {
		t.Fatalf("rows = %v", rows)
	}
	if rows[0][1] != "b,c" || rows[0][2] != "d" {
		t.Fatalf("row0 = %v", rows[0])
	}
	if rows[1][1] != `f"g` {
		t.Fatalf("row1 = %v", rows[1])
	}
}

func TestHeaderKindMatchesChineseHeaders(t *testing.T) {
	if headerKind("参数名称") != "name" {
		t.Fatal("Chinese name header not matched")
	}
	if headerKind("数据类型") != "type" {
		t.Fatal("Chinese type header not matched")
	}
	if headerKind("Bytes") != "offset" {
		t.Fatal("Bytes header not matched")
	}
}

func TestEmptyInputRejected(t *testing.T) {
	if _, err := Convert("", "", "A_FIELDS", "A_TYPE"); err == nil {
		t.Fatal("empty input should be rejected")
	}
}
