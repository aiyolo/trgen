package applog

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestInitWritesAndPrunes(t *testing.T) {
	dir := t.TempDir()
	// Seed stale logs beyond the retention window (16 old files + today's
	// new one keep the most recent 14 only).
	for i := 1; i <= 16; i++ {
		name := fmt.Sprintf("structsheet-2000-01-%02d.log", i)
		if err := os.WriteFile(filepath.Join(dir, name), []byte("old\n"), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	if err := Init(dir); err != nil {
		t.Fatal(err)
	}
	Info("hello %s", "world")
	Error("boom %d", 42)
	if err := Close(); err != nil {
		t.Fatal(err)
	}

	current := LogPath(dir)
	data, err := os.ReadFile(current)
	if err != nil {
		t.Fatal(err)
	}
	if len(data) == 0 {
		t.Fatal("log file empty")
	}
	remaining := 0
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasSuffix(entry.Name(), ".log") {
			remaining++
		}
	}
	if remaining > keepDays {
		t.Fatalf("retention window not enforced: %d files", remaining)
	}
}
