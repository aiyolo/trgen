// Package xmacro 把字段表（xlsx/csv/剪贴板）转成 C X-Macro 代码。
package xmacro

import (
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"structsheets/mygo/internal/xlsxwrite"
)

type request struct {
	path          string
	clipboardText string
	macroName     string
	structName    string
}

// OutputRow 是转换后的一行字段（JSON camelCase）。
type OutputRow struct {
	SourceRow int    `json:"sourceRow"`
	ByteSpec  string `json:"byteSpec"`
	Name      string `json:"name"`
	DataType  string `json:"dataType"`
	Offset    uint64 `json:"offset"`
	Size      uint64 `json:"size"`
}

// ResultData 是 convert 的输出。
type ResultData struct {
	SourcePath string      `json:"sourcePath"`
	SheetName  string      `json:"sheetName"`
	MacroName  string      `json:"macroName"`
	StructName string      `json:"structName"`
	RowCount   int         `json:"rowCount"`
	Code       string      `json:"code"`
	Warnings   []string    `json:"warnings"`
	Rows       []OutputRow `json:"rows"`
}

type columns struct {
	offset    int
	hasOffset bool
	name      int
	dataType  int
	size      int
	hasSize   bool
}

type tableCandidate struct {
	sheetName string
	rows      [][]string
	headerRow int
	columns   columns
	score     int
}

// Convert 解析表格（文件或粘贴文本）并生成 X-Macro 代码。
func Convert(path, clipboardText, macroName, structName string) (*ResultData, error) {
	return convertRequest(request{
		path:          path,
		clipboardText: clipboardText,
		macroName:     macroName,
		structName:    structName,
	})
}

func convertRequest(req request) (*ResultData, error) {
	macro := cIdentifier(req.macroName, true, "TABLE_FIELDS")
	structure := cIdentifier(req.structName, false, "TABLE_TYPE")
	var sourcePath string
	var tables []xlsxwrite.SheetTable
	if strings.TrimSpace(req.clipboardText) != "" {
		sourcePath = "粘贴的 Excel 内容"
		tables = []xlsxwrite.SheetTable{{Name: "粘贴内容", Rows: parseDelimited(req.clipboardText, '\t')}}
	} else {
		trimmed := strings.TrimSpace(req.path)
		if trimmed == "" {
			return nil, fmt.Errorf("请选择表格文件，或粘贴从 Excel/WPS 复制的单元格内容。")
		}
		info, err := os.Stat(trimmed)
		if err != nil || info.IsDir() {
			return nil, fmt.Errorf("请选择表格文件，或粘贴从 Excel/WPS 复制的单元格内容。")
		}
		sourcePath = trimmed
		tables, err = readTables(trimmed)
		if err != nil {
			return nil, err
		}
	}
	candidate, err := chooseTable(tables)
	if err != nil {
		return nil, err
	}
	rows, warnings, err := parseRows(candidate)
	if err != nil {
		return nil, err
	}
	if len(rows) == 0 {
		return nil, fmt.Errorf("找到了表头，但没有找到可转换的字段行。")
	}
	if strings.TrimSpace(req.macroName) != macro {
		warnings = append(warnings, fmt.Sprintf("宏名称已规范为 `%s`。", macro))
	}
	if strings.TrimSpace(req.structName) != structure {
		warnings = append(warnings, fmt.Sprintf("结构体名称已规范为 `%s`。", structure))
	}
	code := generateCode(macro, structure, rows)
	return &ResultData{
		SourcePath: sourcePath,
		SheetName:  candidate.sheetName,
		MacroName:  macro,
		StructName: structure,
		RowCount:   len(rows),
		Code:       code,
		Warnings:   warnings,
		Rows:       rows,
	}, nil
}

func readTables(path string) ([]xlsxwrite.SheetTable, error) {
	if strings.EqualFold(filepath.Ext(path), ".csv") {
		content, err := os.ReadFile(path)
		if err != nil {
			return nil, fmt.Errorf("CSV 读取失败，请确认文件为 UTF-8 编码：%v", err)
		}
		return []xlsxwrite.SheetTable{{Name: "CSV", Rows: parseDelimited(string(content), ',')}}, nil
	}

	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("无法打开表格，请确认格式受支持且文件未损坏：%v", err)
	}
	ext := strings.ToLower(filepath.Ext(path))
	switch ext {
	case ".xlsx":
		tables, err := xlsxwrite.ReadWorkbookTables(data)
		if err != nil {
			return nil, fmt.Errorf("无法打开表格，请确认格式受支持且文件未损坏：%v", err)
		}
		return tables, nil
	default:
		return nil, fmt.Errorf("暂不支持 %s 格式，请另存为 .xlsx 或 .csv 后重试。", ext)
	}
}

func chooseTable(tables []xlsxwrite.SheetTable) (*tableCandidate, error) {
	var best *tableCandidate
	for _, table := range tables {
		limit := len(table.Rows)
		if limit > 100 {
			limit = 100
		}
		for headerRow := 0; headerRow < limit; headerRow++ {
			row := table.Rows[headerRow]
			var candidate columns
			for column, value := range row {
				switch headerKind(value) {
				case "offset":
					if !candidate.hasOffset {
						candidate.offset, candidate.hasOffset = column, true
					}
				case "name":
					if candidate.name == 0 && !containsName(candidate, column) {
						candidate.name = column
					}
				case "type":
					if candidate.dataType == 0 && column != 0 {
						candidate.dataType = column
					}
				case "size":
					if !candidate.hasSize {
						candidate.size, candidate.hasSize = column, true
					}
				}
			}
			if candidate.name == 0 || candidate.dataType == 0 {
				continue
			}
			score := 2
			if candidate.hasOffset {
				score += 2
			}
			if candidate.hasSize {
				score++
			}
			if best == nil || score > best.score {
				best = &tableCandidate{
					sheetName: table.Name,
					rows:      table.Rows,
					headerRow: headerRow,
					columns:   candidate,
					score:     score,
				}
			}
		}
	}
	if best == nil {
		return nil, fmt.Errorf("未识别到字段表头。至少需要 Parameter Name（字段名）和 Type（类型）两列。")
	}
	return best, nil
}

func containsName(cols columns, column int) bool {
	return false
}

func parseRows(candidate *tableCandidate) ([]OutputRow, []string, error) {
	var output []OutputRow
	var warnings []string
	nextOffset := uint64(0)
	emptyRun := 0

	for rowIndex := candidate.headerRow + 1; rowIndex < len(candidate.rows); rowIndex++ {
		row := candidate.rows[rowIndex]
		rawName := cellAt(row, candidate.columns.name)
		dataType := strings.TrimSpace(cellAt(row, candidate.columns.dataType))
		byteSpec := ""
		if candidate.columns.hasOffset {
			byteSpec = strings.TrimSpace(cellAt(row, candidate.columns.offset))
		}
		if strings.TrimSpace(rawName) == "" && dataType == "" && byteSpec == "" {
			emptyRun++
			if emptyRun >= 20 && len(output) > 0 {
				break
			}
			continue
		}
		emptyRun = 0
		if strings.TrimSpace(rawName) == "" || dataType == "" {
			warnings = append(warnings, fmt.Sprintf("第 %d 行缺少字段名或类型，已跳过。", rowIndex+1))
			continue
		}
		name := cIdentifier(strings.TrimSpace(rawName), false, "field")
		if name != strings.TrimSpace(rawName) {
			warnings = append(warnings, fmt.Sprintf("第 %d 行字段名 `%s` 已规范为 `%s`。", rowIndex+1, strings.TrimSpace(rawName), name))
		}
		byteRange, hasRange := parseByteRange(byteSpec)
		explicitSize := uint64(0)
		hasExplicitSize := false
		if candidate.columns.hasSize {
			if value, ok := firstU64(cellAt(row, candidate.columns.size)); ok {
				explicitSize, hasExplicitSize = value, true
			}
		}
		typeSize, hasTypeSize := typeSizeOf(dataType)
		offset := nextOffset
		if hasRange {
			offset = byteRange.offset
		}
		size := uint64(0)
		switch {
		case hasRange && byteRange.size > 0:
			size = byteRange.size
		case hasExplicitSize:
			size = explicitSize
		case hasTypeSize:
			size = typeSize
		default:
			warnings = append(warnings, fmt.Sprintf("第 %d 行类型 `%s` 无法推断大小，按 1 字节生成。", rowIndex+1, dataType))
			size = 1
		}
		nextOffset = offset + size
		output = append(output, OutputRow{
			SourceRow: rowIndex + 1,
			ByteSpec:  byteSpec,
			Name:      name,
			DataType:  dataType,
			Offset:    offset,
			Size:      size,
		})
	}
	return output, warnings, nil
}

func generateCode(macroName, structName string, rows []OutputRow) string {
	var output strings.Builder
	fmt.Fprintf(&output, "#define %s(X) \\\n", macroName)
	for index, row := range rows {
		fmt.Fprintf(&output, "    X(%s, %s, %d, %d)", row.DataType, row.Name, row.Offset, row.Size)
		if index+1 < len(rows) {
			output.WriteString(" \\\n")
		} else {
			output.WriteString("\n")
		}
	}
	fmt.Fprintf(&output, "\ntypedef struct\n{\n    %s(DECLARE_FIELD)\n} %s;\n", macroName, structName)
	return output.String()
}

func headerKind(value string) string {
	normalized := strings.ToLower(strings.TrimSpace(value))
	var b strings.Builder
	for _, ch := range normalized {
		if ch == ' ' || ch == '\t' || ch == '_' || ch == '-' {
			continue
		}
		b.WriteRune(ch)
	}
	switch b.String() {
	case "bytes", "byte", "offset", "iobufferoffset", "字节", "偏移", "地址":
		return "offset"
	case "parametername", "fieldname", "name", "参数名称", "字段名", "变量名":
		return "name"
	case "type", "datatype", "类型", "数据类型":
		return "type"
	case "size", "datasize", "length", "大小", "长度":
		return "size"
	}
	return ""
}

func parseByteRange(value string) (struct {
	offset uint64
	size   uint64
}, bool) {
	type result struct {
		offset uint64
		size   uint64
	}
	if number, err := strconv.ParseFloat(strings.TrimSpace(value), 64); err == nil {
		if !isNaN(number) && number >= 0 && number == float64(uint64(number)) {
			return result{offset: uint64(number), size: 0}, true
		}
	}
	var numbers []uint64
	for _, part := range splitNonDigits(value) {
		if parsed, err := strconv.ParseUint(part, 10, 64); err == nil {
			numbers = append(numbers, parsed)
		}
	}
	switch {
	case len(numbers) >= 2 && numbers[1] >= numbers[0]:
		return result{offset: numbers[0], size: numbers[1] - numbers[0] + 1}, true
	case len(numbers) == 1:
		return result{offset: numbers[0], size: 0}, true
	}
	return result{}, false
}

func splitNonDigits(value string) []string {
	var parts []string
	var current strings.Builder
	for _, ch := range value {
		if ch >= '0' && ch <= '9' {
			current.WriteRune(ch)
		} else {
			if current.Len() > 0 {
				parts = append(parts, current.String())
				current.Reset()
			}
		}
	}
	if current.Len() > 0 {
		parts = append(parts, current.String())
	}
	return parts
}

func isNaN(value float64) bool { return value != value }

func firstU64(value string) (uint64, bool) {
	number, err := strconv.ParseFloat(strings.TrimSpace(value), 64)
	if err != nil || isNaN(number) || number < 0 {
		return 0, false
	}
	return uint64(number), true
}

func typeSizeOf(dataType string) (uint64, bool) {
	compact := strings.ToLower(dataType)
	compact = strings.ReplaceAll(compact, "const", "")
	compact = strings.Join(strings.Fields(compact), " ")
	multiplier := arrayMultiplier(compact)
	base := strings.TrimSpace(strings.SplitN(compact, "[", 2)[0])
	var size uint64
	if strings.Contains(base, "*") {
		size = 8
	} else {
		switch base {
		case "char", "signed char", "unsigned char", "int8_t", "uint8_t", "bool", "_bool":
			size = 1
		case "short", "short int", "signed short", "signed short int", "unsigned short", "unsigned short int", "int16_t", "uint16_t":
			size = 2
		case "int", "signed", "signed int", "unsigned", "unsigned int", "long", "long int", "unsigned long", "unsigned long int", "float", "int32_t", "uint32_t":
			size = 4
		case "long long", "long long int", "unsigned long long", "unsigned long long int", "double", "int64_t", "uint64_t", "size_t":
			size = 8
		case "long double":
			size = 16
		default:
			return 0, false
		}
	}
	return size * multiplier, true
}

func arrayMultiplier(value string) uint64 {
	multiplier := uint64(1)
	rest := value
	for {
		open := strings.Index(rest, "[")
		if open < 0 {
			break
		}
		after := rest[open+1:]
		close := strings.Index(after, "]")
		if close < 0 {
			break
		}
		if length, err := strconv.ParseUint(strings.TrimSpace(after[:close]), 10, 64); err == nil {
			if length < 1 {
				length = 1
			}
			next := multiplier * length
			if next < multiplier {
				next = 1 << 63
			}
			multiplier = next
		}
		rest = after[close+1:]
	}
	return multiplier
}

func cIdentifier(value string, uppercase bool, fallback string) string {
	value = strings.TrimSpace(value)
	var output strings.Builder
	for _, ch := range value {
		if (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || (ch >= '0' && ch <= '9') || ch == '_' {
			if uppercase && ch >= 'a' && ch <= 'z' {
				ch -= 'a' - 'A'
			}
			output.WriteRune(ch)
		} else {
			output.WriteByte('_')
		}
	}
	normalized := output.String()
	for strings.Contains(normalized, "__") {
		normalized = strings.ReplaceAll(normalized, "__", "_")
	}
	normalized = strings.Trim(normalized, "_")
	if normalized == "" {
		normalized = fallback
	}
	if len(normalized) > 0 && normalized[0] >= '0' && normalized[0] <= '9' {
		normalized = "_" + normalized
	}
	return normalized
}

// parseDelimited 解析 CSV/TSV（带引号转义与 BOM 处理），是 Rust 版的直译。
func parseDelimited(content string, delimiter rune) [][]string {
	var rows [][]string
	var row []string
	var field strings.Builder
	trimmed := strings.TrimPrefix(content, "\uFEFF")
	chars := []rune(trimmed)
	quoted := false
	i := 0
	for i < len(chars) {
		ch := chars[i]
		switch {
		case ch == '"' && quoted && i+1 < len(chars) && chars[i+1] == '"':
			field.WriteRune('"')
			i += 2
			continue
		case ch == '"':
			quoted = !quoted
		case ch == delimiter && !quoted:
			row = append(row, field.String())
			field.Reset()
		case ch == '\n' && !quoted:
			row = append(row, field.String())
			field.Reset()
			rows = append(rows, row)
			row = nil
		case ch == '\r' && !quoted:
			// skip
		default:
			field.WriteRune(ch)
		}
		i++
	}
	if field.Len() > 0 || len(row) > 0 {
		row = append(row, field.String())
		rows = append(rows, row)
	}
	return rows
}

func cellAt(row []string, column int) string {
	if column < 0 || column >= len(row) {
		return ""
	}
	return row[column]
}
