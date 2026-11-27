package xlsxwrite

import (
	"archive/zip"
	"bytes"
	"strings"
	"testing"

	"structsheets/mygo/internal/parser"
)

func TestCreatesAValidXlsxContainer(t *testing.T) {
	result, err := parser.Parse("typedef struct { uint32_t id; float value; } Packet;", "")
	if err != nil {
		t.Fatal(err)
	}
	data, err := WriteWorkbook(result)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.HasPrefix(data, []byte("PK")) {
		t.Fatal("output is not a zip container")
	}
	if len(data) < 1000 {
		t.Fatalf("output too small: %d bytes", len(data))
	}
}

func TestWorkbookRoundTripsThroughReader(t *testing.T) {
	result, err := parser.Parse("typedef struct {\n uint32_t id; // 编号\n Sensor s;\n } Packet;", "")
	if err != nil {
		t.Fatal(err)
	}
	// 用一个嵌套结构生成多行多组。
	source := `
	typedef struct { uint16_t code; float v; } Sensor;
	typedef struct { uint32_t id; Sensor s; char name[8]; } Packet;
	`
	result, err = parser.Parse(source, "Packet")
	if err != nil {
		t.Fatal(err)
	}
	data, err := WriteWorkbook(result)
	if err != nil {
		t.Fatal(err)
	}

	tables, err := ReadWorkbookTables(data)
	if err != nil {
		t.Fatal(err)
	}
	if len(tables) != 1 || tables[0].Name != "Packet" {
		t.Fatalf("tables = %+v", tables)
	}
	rows := tables[0].Rows
	if len(rows) != len(result.Rows)+1 {
		t.Fatalf("row count = %d, want %d", len(rows), len(result.Rows)+1)
	}
	if got := rows[0][0]; got != "Status" {
		t.Fatalf("header[0] = %q", got)
	}
	if got := rows[0][7]; got != "Comments" {
		t.Fatalf("header[7] = %q", got)
	}
	if got := rows[1][1]; got != "id" {
		t.Fatalf("row1 name = %q", got)
	}
	if got := rows[1][4]; got != "32" {
		t.Fatalf("row1 size = %q", got)
	}
	if got := rows[2][1]; got != "code" {
		t.Fatalf("row2 name = %q", got)
	}
}

func TestWorkbookEscapesXMLInComments(t *testing.T) {
	result, err := parser.Parse("typedef struct { uint32_t id; // a < b & \"c\"\n } Packet;", "")
	if err != nil {
		t.Fatal(err)
	}
	data, err := WriteWorkbook(result)
	if err != nil {
		t.Fatal(err)
	}
	sheet, err := sheetEntry(data)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(sheet, "&lt;") || !strings.Contains(sheet, "&amp;") {
		t.Fatal("comment values are not XML-escaped")
	}
	tables, err := ReadWorkbookTables(data)
	if err != nil {
		t.Fatal(err)
	}
	if got := tables[0].Rows[1][7]; got != `a < b & "c"` {
		t.Fatalf("comment = %q", got)
	}
}

func sheetEntry(data []byte) (string, error) {
	reader, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		return "", err
	}
	raw, err := readZipEntry(reader, "xl/worksheets/sheet1.xml")
	if err != nil {
		return "", err
	}
	return string(raw), nil
}

func TestSafeSheetName(t *testing.T) {
	cases := map[string]string{
		"Packet":                             "Packet",
		"TR[2]/IN":                           "TR2IN",
		"":                                   "Struct Fields",
		"0123456789012345678901234567890123": "0123456789012345678901234567890",
	}
	for input, want := range cases {
		if got := SafeSheetName(input); got != want {
			t.Errorf("SafeSheetName(%q) = %q, want %q", input, got, want)
		}
	}
}

func TestWorkbookHasStylesAndAutoFilter(t *testing.T) {
	result, err := parser.Parse("typedef struct { uint32_t id; uint16_t v; } P;", "")
	if err != nil {
		t.Fatal(err)
	}
	data, err := WriteWorkbook(result)
	if err != nil {
		t.Fatal(err)
	}
	reader, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		t.Fatal(err)
	}
	sheet, err := readZipEntry(reader, "xl/worksheets/sheet1.xml")
	if err != nil {
		t.Fatal(err)
	}
	content := string(sheet)
	if !strings.Contains(content, `<pane ySplit="1"`) {
		t.Fatal("freeze pane missing")
	}
	if !strings.Contains(content, `autoFilter`) {
		t.Fatal("autofilter missing")
	}
	if !strings.Contains(content, `s="1"`) {
		t.Fatal("cell styles missing")
	}
	styles, err := readZipEntry(reader, "xl/styles.xml")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(styles), "FFFFF200") {
		t.Fatal("yellow header fill missing")
	}
}
