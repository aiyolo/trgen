package main

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"structsheets/mygo/internal/parser"
	"structsheets/mygo/internal/xlsxwrite"
	"structsheets/mygo/internal/xmacro"
)

// Sheets 是 StructSheet 的业务服务：解析结构体、导出 Excel、表格转
// X-Macro。所有操作在本机内存中完成，不落盘任何用户数据。
type Sheets struct{}

// ListStructs 扫描源码中的可导出结构体。
func (Sheets) ListStructs(ctx context.Context, source string) (parser.StructCatalog, error) {
	catalog, err := parser.Discover(source)
	if err != nil {
		return parser.StructCatalog{}, err
	}
	return *catalog, nil
}

// ParseSource 解析源码并把根结构体（空串用默认根）展平成字段表。
func (Sheets) ParseSource(ctx context.Context, source, rootName string) (parser.ParseResult, error) {
	result, err := parser.Parse(source, rootName)
	if err != nil {
		return parser.ParseResult{}, err
	}
	return *result, nil
}

// ReadTextFile 读取本地文本文件（用于导入 .h/.c 头文件）。
func (Sheets) ReadTextFile(ctx context.Context, path string) (string, error) {
	if !filepath.IsAbs(path) {
		return "", fmt.Errorf("读取文件需要绝对路径")
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return "", fmt.Errorf("读取文件失败：%v", err)
	}
	return string(data), nil
}

// ExportExcel 解析源码并写出 .xlsx，返回写盘路径。扩展名缺失时自动补 .xlsx。
func (Sheets) ExportExcel(ctx context.Context, source, rootName, path string) (string, error) {
	result, err := parser.Parse(source, rootName)
	if err != nil {
		return "", err
	}
	outputPath := path
	if filepath.Ext(outputPath) == "" {
		outputPath += ".xlsx"
	}
	data, err := xlsxwrite.WriteWorkbook(result)
	if err != nil {
		return "", fmt.Errorf("Excel 文件写入失败：%v", err)
	}
	if err := os.WriteFile(outputPath, data, 0o644); err != nil {
		return "", fmt.Errorf("Excel 文件写入失败：%v", err)
	}
	return outputPath, nil
}

// ConvertTableToXMacro 把字段表文件或粘贴文本转成 X-Macro 代码。
func (Sheets) ConvertTableToXMacro(ctx context.Context, path, clipboardText, macroName, structName string) (xmacro.ResultData, error) {
	result, err := xmacro.Convert(path, clipboardText, macroName, structName)
	if err != nil {
		return xmacro.ResultData{}, err
	}
	return *result, nil
}

// SaveTextFile 保存文本文件（.h 扩展名缺失时自动补全）。
func (Sheets) SaveTextFile(ctx context.Context, path, content string) (string, error) {
	outputPath := path
	if filepath.Ext(outputPath) == "" {
		outputPath += ".h"
	}
	if err := os.WriteFile(outputPath, []byte(content), 0o644); err != nil {
		return "", fmt.Errorf("代码文件保存失败：%v", err)
	}
	return outputPath, nil
}

// ExportStem 规范导出文件名主干（与旧版 UI 行为一致）。
func ExportStem(name string) string {
	upper := strings.ToUpper(strings.TrimSpace(name))
	switch upper {
	case "TR2_DD_IN", "TR2_SW_DD_IN":
		return "TR2_SW_DD_IN"
	case "TR2_DD_OUT", "TR2_SW_DD_OUT":
		return "TR2_SW_DD_OUT"
	}
	return strings.TrimSpace(name)
}
