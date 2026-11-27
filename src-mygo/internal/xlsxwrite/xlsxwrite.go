// Package xlsxwrite 生成 StructSheet 字段表工作簿：黄色表头、按结构体分组交替的
// 低饱和底色、边框、冻结首行与自动筛选。
package xlsxwrite

import (
	"archive/zip"
	"bytes"
	"encoding/xml"
	"fmt"
	"io"
	"strings"

	"structsheets/mygo/internal/parser"
)

var headers = []string{
	"Status",
	"Parameter Name",
	"System Parameter Name",
	"Data Type",
	"Data Size",
	"InIOBuffer",
	"IOBufferOffset",
	"Comments",
}

// 与 Rust 版一致的分组低饱和配色。
var palette = []uint32{0xEAF2FF, 0xEAF7F0, 0xFFF1E6, 0xF2EDFF, 0xFFF8D9, 0xE8F6F7}

var columnWidths = []float64{11, 34, 38, 14, 12, 13, 16, 30}

type styleSet struct {
	text     int
	centered int
	comment  int
}

type workbookBuilder struct {
	styles  []string
	styleID map[string]int
}

// WriteWorkbook 把解析结果写成 .xlsx 并返回完整字节。
func WriteWorkbook(result *parser.ParseResult) ([]byte, error) {
	b := &workbookBuilder{styleID: map[string]int{}}

	headerStyle := b.style(`<alignment horizontal="center" vertical="center"/><font><b val="1"/><sz val="10"/><color rgb="FF2D2A21"/><name val="Calibri"/></font><fill><patternFill patternType="solid"><fgColor rgb="FFFFF200"/></patternFill></fill><border><left style="thin"><color rgb="FFA0A0A0"/></left><right style="thin"><color rgb="FFA0A0A0"/></right><top style="thin"><color rgb="FFA0A0A0"/></top><bottom style="thin"><color rgb="FFA0A0A0"/></bottom></border>`)
	rowStyles := make([]styleSet, len(palette))
	for i, background := range palette {
		base := fmt.Sprintf(`<fill><patternFill patternType="solid"><fgColor rgb="FF%06X"/></patternFill></fill>`, background)
		border := `<border><left style="thin"><color rgb="FFB8B8B8"/></left><right style="thin"><color rgb="FFB8B8B8"/></right><top style="thin"><color rgb="FFB8B8B8"/></top><bottom style="thin"><color rgb="FFB8B8B8"/></bottom></border>`
		font := `<font><sz val="10"/><color rgb="FF28271F"/><name val="Calibri"/></font>`
		vertical := `<alignment vertical="center"/>`
		center := `<alignment horizontal="center" vertical="center"/>`
		wrap := `<alignment vertical="center" wrapText="1"/>`
		rowStyles[i] = styleSet{
			text:     b.style(font + base + border + vertical),
			centered: b.style(font + base + border + center),
			comment:  b.style(font + base + border + wrap),
		}
	}

	groupColors := map[string]int{}
	nextColor := 0

	var sheet strings.Builder
	sheet.WriteString(xml.Header)
	sheet.WriteString(`<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">`)
	sheet.WriteString(`<dimension ref="A1"/>`)
	sheet.WriteString(`<sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews>`)
	sheet.WriteString(`<sheetFormatPr defaultRowHeight="15"/>`)
	sheet.WriteString(`<cols>`)
	for i, width := range columnWidths {
		sheet.WriteString(fmt.Sprintf(`<col min="%d" max="%d" width="%g" customWidth="1"/>`, i+1, i+1, width))
	}
	sheet.WriteString(`</cols>`)
	sheet.WriteString(`<sheetData>`)

	// header row
	sheet.WriteString(`<row r="1" ht="24" customHeight="1">`)
	for column, header := range headers {
		sheet.WriteString(cellXML(1, column+1, header, headerStyle, false))
	}
	sheet.WriteString(`</row>`)

	for index, row := range result.Rows {
		excelRow := index + 2
		colorIndex, seen := groupColors[row.GroupKey]
		if !seen {
			colorIndex = nextColor % len(rowStyles)
			groupColors[row.GroupKey] = colorIndex
			nextColor++
		}
		styles := rowStyles[colorIndex]
		sheet.WriteString(fmt.Sprintf(`<row r="%d">`, excelRow))
		sheet.WriteString(cellXML(excelRow, 1, row.Status, styles.centered, false))
		sheet.WriteString(cellXML(excelRow, 2, row.ParameterName, styles.text, false))
		sheet.WriteString(cellXML(excelRow, 3, row.SystemParameterName, styles.text, false))
		sheet.WriteString(cellXML(excelRow, 4, row.DataType, styles.centered, false))
		sheet.WriteString(cellXML(excelRow, 5, fmt.Sprintf("%d", row.DataSize), styles.centered, true))
		sheet.WriteString(cellXML(excelRow, 6, row.InIOBuffer, styles.centered, false))
		sheet.WriteString(cellXML(excelRow, 7, fmt.Sprintf("%d", row.IOBufferOffset), styles.centered, true))
		sheet.WriteString(cellXML(excelRow, 8, row.Comments, styles.comment, false))
		sheet.WriteString(`</row>`)
	}
	sheet.WriteString(`</sheetData>`)

	if len(result.Rows) > 0 {
		last := fmt.Sprintf("H%d", len(result.Rows)+1)
		sheet.WriteString(`<autoFilter ref="A1:` + last + `"/>`)
	}
	sheet.WriteString(`</worksheet>`)

	sheetName := SafeSheetName(result.RootName)
	return buildZip(sheetName, sheet.String(), b.styles)
}

func (b *workbookBuilder) style(xf string) int {
	if id, ok := b.styleID[xf]; ok {
		return id
	}
	id := len(b.styles)
	b.styles = append(b.styles, xf)
	b.styleID[xf] = id
	return id
}

func cellXML(row, col int, value string, style int, numeric bool) string {
	ref := cellRef(row, col)
	attrs := fmt.Sprintf(`r="%s" s="%d"`, ref, style)
	if numeric {
		attrs += ` t="n"`
	}
	if value == "" {
		return fmt.Sprintf(`<c %s/>`, attrs)
	}
	if numeric {
		return fmt.Sprintf(`<c %s><v>%s</v></c>`, attrs, value)
	}
	return fmt.Sprintf(`<c %s t="inlineStr"><is><t xml:space="preserve">%s</t></is></c>`, attrs, xmlEscape(value))
}

func cellRef(row, col int) string {
	name := ""
	for col > 0 {
		col--
		name = string(rune('A'+col%26)) + name
		col /= 26
	}
	return fmt.Sprintf("%s%d", name, row)
}

func xmlEscape(value string) string {
	var b strings.Builder
	if err := xml.EscapeText(&b, []byte(value)); err != nil {
		return ""
	}
	return b.String()
}

// SafeSheetName cleans an identifier into a legal worksheet name.
func SafeSheetName(name string) string {
	cleaned := strings.Map(func(r rune) rune {
		switch r {
		case '[', ']', ':', '*', '?', '/', '\\':
			return -1
		}
		return r
	}, name)
	if len(cleaned) > 31 {
		runes := []rune(cleaned)
		cleaned = string(runes[:31])
	}
	if strings.TrimSpace(cleaned) == "" {
		return "Struct Fields"
	}
	return cleaned
}

// BuildSimpleWorkbook writes a plain string grid as a one-sheet workbook. It
// backs the X-Macro reader tests and small fixtures.
func BuildSimpleWorkbook(table SheetTable) ([]byte, error) {
	var sheet strings.Builder
	sheet.WriteString(xml.Header)
	sheet.WriteString(`<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">`)
	sheet.WriteString(`<sheetData>`)
	for rowIndex, row := range table.Rows {
		sheet.WriteString(fmt.Sprintf(`<row r="%d">`, rowIndex+1))
		for columnIndex, value := range row {
			sheet.WriteString(cellXML(rowIndex+1, columnIndex+1, value, 0, false))
		}
		sheet.WriteString(`</row>`)
	}
	sheet.WriteString(`</sheetData></worksheet>`)

	var buf bytes.Buffer
	zipWriter := zip.NewWriter(&buf)
	entries := map[string]string{
		"[Content_Types].xml": `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>`,
		"_rels/.rels": `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>`,
		"xl/workbook.xml": `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="` + xmlEscape(table.Name) + `" sheetId="1" r:id="rId1"/></sheets>
</workbook>`,
		"xl/_rels/workbook.xml.rels": `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>`,
		"xl/worksheets/sheet1.xml": sheet.String(),
	}
	for name, content := range entries {
		entry, err := zipWriter.Create(name)
		if err != nil {
			return nil, err
		}
		if _, err := entry.Write([]byte(content)); err != nil {
			return nil, err
		}
	}
	if err := zipWriter.Close(); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func buildZip(sheetName, sheetXML string, styles []string) ([]byte, error) {
	var buf bytes.Buffer
	zipWriter := zip.NewWriter(&buf)

	sheetEntry := func(name, content, contentType string) error {
		entry, err := zipWriter.Create(name)
		if err != nil {
			return err
		}
		if _, err := entry.Write([]byte(content)); err != nil {
			return err
		}
		return nil
	}

	contentTypes := `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
</Types>`

	rootRels := `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>`

	workbookXML := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="%s" sheetId="1" r:id="rId1"/></sheets>
</workbook>`, xmlEscape(sheetName))

	workbookRels := `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>`

	var styleBuf strings.Builder
	styleBuf.WriteString(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">`)
	// 字体、填充和边框按引用计数登记，xf 索引从 cellXfs 顺序派生。
	fonts := []string{`<font><sz val="11"/><name val="Calibri"/></font>`}
	fills := []string{`<fill><patternFill patternType="none"/></fill>`, `<fill><patternFill patternType="gray125"/></fill>`}
	borders := []string{`<border><left/><right/><top/><bottom/><diagonal/></border>`}
	fontIDs := map[string]int{}
	fillIDs := map[string]int{}
	borderIDs := map[string]int{}
	var xfs strings.Builder
	for _, xf := range styles {
		fontID := registerComponent(&fonts, fontIDs, extractComponent(xf, "<font>", "</font>"))
		fillID := registerComponent(&fills, fillIDs, extractComponent(xf, "<fill>", "</fill>"))
		borderID := registerComponent(&borders, borderIDs, extractComponent(xf, "<border>", "</border>"))
		alignment := extractRaw(xf, "<alignment", "/>")
		xfs.WriteString(fmt.Sprintf(`<xf numFmtId="0" fontId="%d" fillId="%d" borderId="%d" applyFont="1" applyFill="1" applyBorder="1" applyAlignment="1">%s</xf>`, fontID, fillID, borderID, alignment))
	}
	styleBuf.WriteString(fmt.Sprintf(`<fonts count="%d">%s</fonts>`, len(fonts), strings.Join(fonts, "")))
	styleBuf.WriteString(fmt.Sprintf(`<fills count="%d">%s</fills>`, len(fills), strings.Join(fills, "")))
	styleBuf.WriteString(fmt.Sprintf(`<borders count="%d">%s</borders>`, len(borders), strings.Join(borders, "")))
	styleBuf.WriteString(`<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>`)
	styleBuf.WriteString(fmt.Sprintf(`<cellXfs count="%d">%s</cellXfs>`, len(styles), xfs.String()))
	styleBuf.WriteString(`<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>`)
	styleBuf.WriteString(`</styleSheet>`)

	if err := sheetEntry("[Content_Types].xml", contentTypes, ""); err != nil {
		return nil, err
	}
	if err := sheetEntry("_rels/.rels", rootRels, ""); err != nil {
		return nil, err
	}
	if err := sheetEntry("xl/workbook.xml", workbookXML, ""); err != nil {
		return nil, err
	}
	if err := sheetEntry("xl/_rels/workbook.xml.rels", workbookRels, ""); err != nil {
		return nil, err
	}
	if err := sheetEntry("xl/styles.xml", styleBuf.String(), ""); err != nil {
		return nil, err
	}
	if err := sheetEntry("xl/worksheets/sheet1.xml", sheetXML, ""); err != nil {
		return nil, err
	}

	if err := zipWriter.Close(); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func extractComponent(xf, open, close string) string {
	start := strings.Index(xf, open)
	if start < 0 {
		return open + close[1:]
	}
	end := strings.Index(xf[start:], close)
	if end < 0 {
		return open + close[1:]
	}
	return xf[start : start+end+len(close)]
}

func extractRaw(value, open, close string) string {
	start := strings.Index(value, open)
	if start < 0 {
		return ""
	}
	end := strings.Index(value[start:], close)
	if end < 0 {
		return ""
	}
	return value[start : start+end+len(close)]
}

func registerComponent(list *[]string, ids map[string]int, component string) int {
	if id, ok := ids[component]; ok {
		return id
	}
	id := len(*list)
	*list = append(*list, component)
	ids[component] = id
	return id
}

// ---- 读取（表格转 X-Macro 输入侧）----

// SheetTable 是一个工作表的名称与文本行。
type SheetTable struct {
	Name string
	Rows [][]string
}

// ReadWorkbookTables reads all sheets of an .xlsx file into string grids.
func ReadWorkbookTables(data []byte) ([]SheetTable, error) {
	reader, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		return nil, fmt.Errorf("open zip: %w", err)
	}
	targets, err := readSheetTargets(reader)
	if err != nil {
		return nil, err
	}
	shared, err := readSharedStrings(reader)
	if err != nil {
		return nil, err
	}

	tables := make([]SheetTable, 0, len(targets))
	for _, target := range targets {
		sheetXML, err := readZipEntry(reader, target.target)
		if err != nil {
			continue
		}
		rows := parseSheetRows(sheetXML, shared)
		tables = append(tables, SheetTable{Name: target.name, Rows: rows})
	}
	if len(tables) == 0 {
		return nil, fmt.Errorf("工作簿中没有可读取的工作表。")
	}
	return tables, nil
}

type sheetTarget struct {
	name   string
	target string
}

func readSheetTargets(reader *zip.Reader) ([]sheetTarget, error) {
	rels := map[string]string{}
	if data, err := readZipEntry(reader, "xl/_rels/workbook.xml.rels"); err == nil {
		rels = parseRelationships(data)
	}
	wbData, err := readZipEntry(reader, "xl/workbook.xml")
	if err != nil {
		return nil, err
	}
	var targets []sheetTarget
	decoder := xml.NewDecoder(bytes.NewReader(wbData))
	decoder.Strict = false
	index := 0
	for {
		tok, err := decoder.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			break
		}
		start, ok := tok.(xml.StartElement)
		if !ok || start.Name.Local != "sheet" {
			continue
		}
		var name, rid string
		for _, attr := range start.Attr {
			if attr.Name.Local == "name" {
				name = attr.Value
			}
			if attr.Name.Local == "id" || strings.HasSuffix(attr.Name.Local, ":id") {
				rid = attr.Value
			}
		}
		target := rels[rid]
		if target == "" {
			target = fmt.Sprintf("xl/worksheets/sheet%d.xml", index+1)
		} else {
			target = normalizeTarget(target)
		}
		targets = append(targets, sheetTarget{name: name, target: target})
		index++
	}
	return targets, nil
}

func normalizeTarget(target string) string {
	target = strings.ReplaceAll(target, "\\", "/")
	if strings.HasPrefix(target, "/xl/") {
		return "xl/" + strings.TrimPrefix(target, "/xl/")
	}
	return "xl/" + strings.TrimPrefix(target, "/")
}

func readZipEntry(reader *zip.Reader, name string) ([]byte, error) {
	for _, entry := range reader.File {
		if entry.Name == name {
			file, err := entry.Open()
			if err != nil {
				return nil, err
			}
			defer file.Close()
			return io.ReadAll(file)
		}
	}
	return nil, fmt.Errorf("entry %q not found", name)
}

func parseRelationships(data []byte) map[string]string {
	out := map[string]string{}
	decoder := xml.NewDecoder(bytes.NewReader(data))
	decoder.Strict = false
	for {
		tok, err := decoder.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			break
		}
		start, ok := tok.(xml.StartElement)
		if !ok || start.Name.Local != "Relationship" {
			continue
		}
		var id, target string
		for _, attr := range start.Attr {
			if attr.Name.Local == "Id" {
				id = attr.Value
			}
			if attr.Name.Local == "Target" {
				target = attr.Value
			}
		}
		if id != "" && target != "" {
			out[id] = target
		}
	}
	return out
}

func readSharedStrings(reader *zip.Reader) ([]string, error) {
	data, err := readZipEntry(reader, "xl/sharedStrings.xml")
	if err != nil {
		return nil, nil
	}
	var strings_ []string
	decoder := xml.NewDecoder(bytes.NewReader(data))
	decoder.Strict = false
	inText := false
	var current strings.Builder
	for {
		tok, err := decoder.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, err
		}
		switch element := tok.(type) {
		case xml.StartElement:
			if element.Name.Local == "t" {
				inText = true
				current.Reset()
			}
		case xml.CharData:
			if inText {
				current.Write(element)
			}
		case xml.EndElement:
			if element.Name.Local == "t" {
				inText = false
				strings_ = append(strings_, current.String())
			}
		}
	}
	return strings_, nil
}

// parseSheetRows materializes cells into a rectangular string grid.
func parseSheetRows(sheetXML []byte, shared []string) [][]string {
	decoder := xml.NewDecoder(bytes.NewReader(sheetXML))
	decoder.Strict = false
	rows := map[int][]string{}
	maxRow := 0
	var currentRow int
	var currentCol int
	var inValue bool
	var inText bool
	var captured strings.Builder

	cellDone := func() {
		if currentRow == 0 {
			return
		}
		value := captured.String()
		if len(rows[currentRow]) < currentCol {
			grown := make([]string, currentCol)
			copy(grown, rows[currentRow])
			rows[currentRow] = grown
		}
		rows[currentRow][currentCol-1] = value
		if currentRow > maxRow {
			maxRow = currentRow
		}
	}

	sharedMode := false

	for {
		tok, err := decoder.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil
		}
		switch element := tok.(type) {
		case xml.StartElement:
			switch element.Name.Local {
			case "row":
				currentRow = 0
				for _, attr := range element.Attr {
					if attr.Name.Local == "r" {
						fmt.Sscanf(attr.Value, "%d", &currentRow)
					}
				}
			case "c":
				sharedMode = false
				captured.Reset()
				currentCol = 0
				ref := ""
				cellType := ""
				for _, attr := range element.Attr {
					switch attr.Name.Local {
					case "r":
						ref = attr.Value
					case "t":
						cellType = attr.Value
					}
				}
				if row, col, ok := parseCellReference(ref); ok {
					currentRow, currentCol = row, col
				}
				if cellType == "s" {
					sharedMode = true
				}
			case "v":
				inValue = true
				captured.Reset()
			case "t":
				inText = true
				captured.Reset()
			}
		case xml.CharData:
			if inValue || inText {
				captured.Write(element)
			}
		case xml.EndElement:
			switch element.Name.Local {
			case "v", "t":
				if element.Name.Local == "v" {
					inValue = false
				} else {
					inText = false
				}
				if sharedMode && element.Name.Local == "v" {
					if index := parseSharedIndex(captured.String()); index >= 0 && index < len(shared) {
						captured.Reset()
						captured.WriteString(shared[index])
					}
				}
			case "c":
				cellDone()
			}
		}
	}

	grid := make([][]string, maxRow)
	for rowIndex := 1; rowIndex <= maxRow; rowIndex++ {
		row := rows[rowIndex]
		if row == nil {
			row = []string{}
		}
		grid[rowIndex-1] = row
	}
	return grid
}

func parseSharedIndex(value string) int {
	var index int
	if _, err := fmt.Sscanf(strings.TrimSpace(value), "%d", &index); err != nil {
		return -1
	}
	return index
}

func parseCellReference(reference string) (int, int, bool) {
	row := 0
	col := 0
	i := 0
	for i < len(reference) && reference[i] >= 'A' && reference[i] <= 'Z' {
		col = col*26 + int(reference[i]-'A') + 1
		i++
	}
	if i == 0 {
		return 0, 0, false
	}
	for i < len(reference) && reference[i] >= '0' && reference[i] <= '9' {
		row = row*10 + int(reference[i]-'0')
		i++
	}
	if row <= 0 || col <= 0 {
		return 0, 0, false
	}
	return row, col, true
}
