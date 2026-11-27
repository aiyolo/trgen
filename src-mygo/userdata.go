package main

import (
	"os"
	"path/filepath"
	"runtime"
)

// resolveDataDir 返回应用数据目录：Windows 为 %LOCALAPPDATA%\StructSheet，
// 其他平台沿用 mygo 的用户数据目录。
func resolveDataDir() (string, error) {
	if runtime.GOOS == "windows" {
		local := os.Getenv("LOCALAPPDATA")
		if local != "" {
			return ensureDir(filepath.Join(local, "StructSheet"))
		}
	}
	if base, err := os.UserConfigDir(); err == nil {
		return ensureDir(filepath.Join(base, "StructSheet"))
	} else {
		return "", err
	}
}

func ensureDir(path string) (string, error) {
	if err := os.MkdirAll(path, 0o755); err != nil {
		return "", err
	}
	return path, nil
}
