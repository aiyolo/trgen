// Package applog 按天写运行日志，保留 14 天。
package applog

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
)

const keepDays = 14

var (
	mu     sync.Mutex
	writer io.Writer
	logDir string
)

// Init 打开日志目录并创建今天的日志文件。
func Init(dir string) error {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("create log dir: %w", err)
	}
	mu.Lock()
	defer mu.Unlock()
	logDir = dir
	path := filepath.Join(dir, time.Now().Format("structsheet-2006-01-02.log"))
	file, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		return fmt.Errorf("open log file: %w", err)
	}
	writer = file
	pruneOldLogs(dir)
	return nil
}

// Close 关闭当前日志文件。
func Close() error {
	mu.Lock()
	defer mu.Unlock()
	if closer, ok := writer.(io.Closer); ok && writer != nil {
		err := closer.Close()
		writer = nil
		return err
	}
	return nil
}

// LogPath 返回目录内当前日志文件的完整路径。
func LogPath(dir string) string {
	return filepath.Join(dir, time.Now().Format("structsheet-2006-01-02.log"))
}

// Info 写一条普通日志。
func Info(format string, args ...any) { write("INFO ", format, args...) }

// Error 写一条错误日志。
func Error(format string, args ...any) { write("ERROR", format, args...) }

func write(level, format string, args ...any) {
	mu.Lock()
	defer mu.Unlock()
	if writer == nil {
		return
	}
	fmt.Fprintf(writer, "%s %s %s\n", time.Now().Format("15:04:05"), level, fmt.Sprintf(format, args...))
}

// pruneOldLogs 只保留按文件名排序后最近的 keepDays 个日志。
func pruneOldLogs(dir string) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return
	}
	var names []string
	for _, entry := range entries {
		if entry.IsDir() || !entry.Type().IsRegular() {
			continue
		}
		name := entry.Name()
		if !strings.HasPrefix(name, "structsheet-") || !strings.HasSuffix(name, ".log") {
			continue
		}
		// 只认 structsheet-YYYY-MM-DD.log 形态。
		if len(name) != len("structsheet-2006-01-02.log") {
			continue
		}
		if _, err := time.Parse("2006-01-02", strings.TrimSuffix(strings.TrimPrefix(name, "structsheet-"), ".log")); err != nil {
			continue
		}
		names = append(names, name)
	}
	sort.Strings(names)
	for len(names) > keepDays {
		_ = os.Remove(filepath.Join(dir, names[0]))
		names = names[1:]
	}
}
