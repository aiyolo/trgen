package main

import (
	"log"
	"os"
	"path/filepath"
	"runtime"
	"runtime/debug"

	"github.com/egoist/mygo"

	"structsheets/mygo/internal/applog"
)

func main() {
	mygo.Bind(Desktop{})
	mygo.Bind(Sheets{})

	if os.Getenv("MYGO_GENERATE") != "" {
		if err := mygo.App.Run(); err != nil {
			log.Fatal(err)
		}
		return
	}

	dataDir, err := resolveDataDir()
	if err != nil {
		log.Fatal(err)
	}
	logDir := filepath.Join(dataDir, "logs")
	if err := applog.Init(logDir); err != nil {
		log.Fatal(err)
	}
	defer applog.Close()
	applog.Info("StructSheet %s 启动 platform=%s/%s data=%s", mygo.App.Version(), runtime.GOOS, runtime.GOARCH, dataDir)
	defer func() {
		if recovered := recover(); recovered != nil {
			applog.Error("未捕获 panic: %v\n%s", recovered, debug.Stack())
			_ = applog.Close()
			panic(recovered)
		}
	}()

	if !mygo.WebViewAvailable() {
		log.Fatal("StructSheet 需要 Microsoft Edge WebView2 Runtime")
	}

	if !mygo.App.RequestSingleInstanceLock() {
		return
	}
	mygo.App.OnSecondInstance(func(args []string, workingDir string) {
		if w := getMainWindow(); w != nil {
			w.Show()
			w.Focus()
		}
	})

	mygo.App.WhenReady(func() {
		w := mygo.NewWindow(appWindowOptions("StructSheet", "/"))
		protectWindow(w)
		setMainWindow(w)
	})

	if err := mygo.App.Run(); err != nil {
		log.Fatal(err)
	}
}
