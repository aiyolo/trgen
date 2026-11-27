package main

import (
	"context"
	"fmt"
	"net/url"
	"path/filepath"
	"strings"
	"sync"

	"github.com/egoist/mygo"

	"structsheets/mygo/internal/parser"
	"structsheets/mygo/internal/xlsxwrite"
)

// mainWindow 是应用主窗口，用于单实例聚焦。
var (
	mainMu     sync.Mutex
	mainWindow *mygo.Window
)

func setMainWindow(w *mygo.Window) {
	mainMu.Lock()
	mainWindow = w
	mainMu.Unlock()
}

func getMainWindow() *mygo.Window {
	mainMu.Lock()
	defer mainMu.Unlock()
	return mainWindow
}

// Desktop 是绑定给前端的原生桌面能力：系统对话框、文件打开与定位、
// 版本信息。业务解析与导出在 Sheets 服务里。
type Desktop struct{}

// AppVersion 返回 mygo.json 里的应用版本。
func (Desktop) AppVersion() string { return mygo.App.Version() }

// PickFiles 弹出系统打开文件对话框，返回选中的绝对路径数组。
func (Desktop) PickFiles(ctx context.Context, title string, extensions []string, multiple bool) ([]string, error) {
	paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
		Parent:   mygo.CallerWindow(ctx),
		Title:    title,
		Multiple: multiple,
		Filters:  extensionFilters(extensions),
	})
	if err != nil {
		return nil, err
	}
	if paths == nil {
		paths = []string{}
	}
	return paths, nil
}

// PickFolder 弹出系统选择文件夹对话框，取消时返回空串。
func (Desktop) PickFolder(ctx context.Context, title string) (string, error) {
	paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
		Parent:    mygo.CallerWindow(ctx),
		Title:     title,
		Directory: true,
		Multiple:  false,
	})
	if err != nil {
		return "", err
	}
	if len(paths) == 0 {
		return "", nil
	}
	return paths[0], nil
}

// SaveFile 弹出系统保存对话框，取消时返回空串。
func (Desktop) SaveFile(ctx context.Context, title, defaultPath string, extensions []string) (string, error) {
	path, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
		Parent:      mygo.CallerWindow(ctx),
		Title:       title,
		DefaultPath: defaultPath,
		Filters:     extensionFilters(extensions),
	})
	if err != nil {
		return "", err
	}
	return path, nil
}

// RevealInFolder 在资源管理器中定位文件。
func (Desktop) RevealInFolder(ctx context.Context, path string) error {
	if !filepath.IsAbs(path) {
		return fmt.Errorf("在文件资源管理器中定位需要绝对路径")
	}
	mygo.Shell.ShowItemInFolder(path)
	return nil
}

// OpenPath 用系统默认程序打开文件或目录。
func (Desktop) OpenPath(ctx context.Context, path string) error {
	if !filepath.IsAbs(path) {
		return fmt.Errorf("打开文件或目录需要绝对路径")
	}
	mygo.Shell.OpenPath(path)
	return nil
}

func extensionFilters(extensions []string) []mygo.FileFilter {
	if len(extensions) == 0 {
		return nil
	}
	names := map[string]string{
		"h":    "C 头文件",
		"c":    "C 源文件",
		"txt":  "文本文件",
		"xlsx": "Excel 工作簿",
		"xls":  "Excel 97-2003 工作簿",
		"xlsb": "Excel 二进制工作簿",
		"ods":  "OpenDocument 表格",
		"csv":  "CSV 文本",
	}
	label := "支持的文件"
	if len(extensions) == 1 {
		if name, ok := names[extensions[0]]; ok {
			label = name
		}
	} else {
		label = "表格"
	}
	return []mygo.FileFilter{{Name: label, Extensions: extensions}}
}

// appWindowOptions 返回隐藏标题栏 + 页面自绘的窗口配置。
func appWindowOptions(title, pageURL string) mygo.WindowOptions {
	return mygo.WindowOptions{
		Title:             title,
		URL:               pageURL,
		Width:             1240,
		Height:            790,
		MinWidth:          960,
		MinHeight:         600,
		BackgroundColor:   "#f4f2ec",
		TitleBarStyle:     mygo.TitleBarHidden,
		HideWindowButtons: true,
		TitleBarHeight:    34,
	}
}

// protectWindow 安装导航白名单与弹窗拒绝守卫。
func protectWindow(w *mygo.Window) {
	page := w.Page()
	if page == nil {
		return
	}
	page.SetPermissionHandler(func(mygo.PermissionRequest) bool { return false })
	page.SetWindowOpenHandler(func(mygo.WindowOpenRequest) *mygo.WindowOptions { return nil })
	page.OnWillNavigate(func(e *mygo.NavigateEvent) {
		if !allowNavigation(e.URL, mygo.IsDev()) {
			e.PreventDefault()
		}
	})
}

// allowNavigation 只允许应用自身页面与开发服务器。
func allowNavigation(raw string, isDev bool) bool {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return false
	}
	u, err := url.Parse(trimmed)
	if err != nil {
		return false
	}
	appPage := u.Path == "/" || u.Path == "/index.html"
	switch u.Scheme {
	case "":
		return u.Host == "" && (appPage || u.Path == "index.html")
	case "mygo":
		return appPage
	case "http", "https":
		if !isDev {
			return false
		}
		return u.Hostname() == "127.0.0.1" || u.Hostname() == "localhost"
	default:
		return false
	}
}

var _ = parser.Discover
var _ = xlsxwrite.WriteWorkbook
