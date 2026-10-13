package main

import (
	"bytes"
	"context"
	"embed"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"log"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"

	"github.com/egoist/mygo"
)

//go:embed all:dist
var frontend embed.FS

//go:embed src-tauri/icons/32x32.png
var windowIcon []byte

type coreRequest struct {
	Action        string  `json:"action"`
	Source        string  `json:"source,omitempty"`
	RootName      *string `json:"rootName,omitempty"`
	Path          string  `json:"path,omitempty"`
	ClipboardText string  `json:"clipboardText,omitempty"`
	MacroName     string  `json:"macroName,omitempty"`
	StructName    string  `json:"structName,omitempty"`
}

type coreResponse struct {
	OK    bool   `json:"ok"`
	Data  any    `json:"data"`
	Error string `json:"error"`
}

type Service struct {
	mu     sync.Mutex
	reader *mygo.Window
}

func corePath() (string, error) {
	if path := os.Getenv("STRUCTSHEET_CORE_PATH"); path != "" {
		return path, nil
	}
	executable, err := os.Executable()
	if err != nil {
		return "", err
	}
	name := "StructSheetCore"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	path := filepath.Join(filepath.Dir(executable), name)
	if _, err := os.Stat(path); err == nil {
		return path, nil
	}
	return "", fmt.Errorf("找不到处理核心：%s", path)
}

func runCore(ctx context.Context, request coreRequest) (any, error) {
	path, err := corePath()
	if err != nil {
		return nil, err
	}
	input, err := json.Marshal(request)
	if err != nil {
		return nil, err
	}
	command := exec.CommandContext(ctx, path)
	configureCoreCommand(command)
	command.Stdin = bytes.NewReader(input)
	var stdout, stderr bytes.Buffer
	command.Stdout = &stdout
	command.Stderr = &stderr
	if err := command.Run(); err != nil {
		return nil, fmt.Errorf("处理核心运行失败：%w: %s", err, strings.TrimSpace(stderr.String()))
	}
	var response coreResponse
	if err := json.Unmarshal(stdout.Bytes(), &response); err != nil {
		return nil, fmt.Errorf("处理核心返回的数据无效：%w", err)
	}
	if !response.OK {
		return nil, errors.New(response.Error)
	}
	return response.Data, nil
}

func (s *Service) ListStructs(ctx context.Context, source string) (any, error) {
	return runCore(ctx, coreRequest{Action: "list_structs", Source: source})
}

func (s *Service) ParseSource(ctx context.Context, source string, rootName *string) (any, error) {
	return runCore(ctx, coreRequest{Action: "parse_source", Source: source, RootName: rootName})
}

func (s *Service) ExportExcel(ctx context.Context, source string, rootName *string, path string) (any, error) {
	return runCore(ctx, coreRequest{Action: "export_excel", Source: source, RootName: rootName, Path: path})
}

func (s *Service) ConvertTableToXmacro(ctx context.Context, path, clipboardText, macroName, structName string) (any, error) {
	return runCore(ctx, coreRequest{
		Action: "convert_table_to_xmacro", Path: path, ClipboardText: clipboardText,
		MacroName: macroName, StructName: structName,
	})
}

func (s *Service) ReadExcelDocument(ctx context.Context, path string) (any, error) {
	return runCore(ctx, coreRequest{Action: "read_excel_document", Path: path})
}

func (s *Service) SaveTextFile(path, content string) (string, error) {
	if filepath.Ext(path) == "" {
		path += ".h"
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		return "", fmt.Errorf("代码文件保存失败：%w", err)
	}
	return path, nil
}

func (s *Service) ChooseExportPath(ctx context.Context, defaultName string) (string, error) {
	return mygo.Dialog.Save(mygo.SaveDialogOptions{
		Parent: mygo.CallerWindow(ctx), Title: "导出 Excel", DefaultPath: defaultName,
		Filters: []mygo.FileFilter{{Name: "Excel 工作簿", Extensions: []string{"xlsx"}}},
	})
}

func (s *Service) ChooseCodePath(ctx context.Context, defaultName string) (string, error) {
	return mygo.Dialog.Save(mygo.SaveDialogOptions{
		Parent: mygo.CallerWindow(ctx), Title: "保存 C 代码", DefaultPath: defaultName,
		Filters: []mygo.FileFilter{{Name: "C 头文件", Extensions: []string{"h"}}, {Name: "文本文件", Extensions: []string{"txt"}}},
	})
}

func (s *Service) ChooseTablePath(ctx context.Context) (string, error) {
	paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
		Parent: mygo.CallerWindow(ctx), Title: "选择表格文件",
		Filters: []mygo.FileFilter{{Name: "表格文件", Extensions: []string{"xlsx", "xls", "xlsb", "ods", "csv"}}},
	})
	if err != nil || len(paths) == 0 {
		return "", err
	}
	return paths[0], nil
}

func (s *Service) ShowExcelReader() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.reader != nil {
		s.reader.Show()
		return nil
	}
	reader := mygo.NewWindow(mygo.WindowOptions{
		Title: "StructSheet · Excel 阅读器", URL: "/?mode=reader",
		Width: 1420, Height: 860, MinWidth: 900, MinHeight: 600,
		Hidden: true, BackgroundColor: "#f4f2ec",
		TitleBarStyle: mygo.TitleBarHidden, TitleBarHeight: 34,
	})
	if reader == nil {
		return errors.New("无法打开 Excel 阅读器窗口")
	}
	_ = reader.SetIcon(windowIcon)
	reader.OnReadyToShow(reader.Show)
	reader.OnClosed(func() {
		s.mu.Lock()
		s.reader = nil
		s.mu.Unlock()
	})
	s.reader = reader
	return nil
}

func (s *Service) HideExcelReader() {
	s.mu.Lock()
	reader := s.reader
	s.mu.Unlock()
	if reader != nil {
		reader.Close()
	}
}

func main() {
	web, err := fs.Sub(frontend, "dist")
	if err != nil {
		log.Fatal(err)
	}
	mygo.SetFrontend(web)
	service := &Service{}
	mygo.Bind(service)
	mygo.App.WhenReady(func() {
		window := mygo.NewWindow(mygo.WindowOptions{
			Title: "StructSheet · C 结构体转 Excel", URL: "/",
			Width: 1240, Height: 790, MinWidth: 960, MinHeight: 620,
			Hidden: true, BackgroundColor: "#f4f2ec",
			TitleBarStyle: mygo.TitleBarHidden, TitleBarHeight: 34,
		})
		_ = window.SetIcon(windowIcon)
		window.OnReadyToShow(window.Show)
		window.OnClosed(mygo.App.Quit)
	})
	if err := mygo.App.Run(); err != nil {
		log.Fatal(err)
	}
}
