package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestResolveDataDirCreatesDirectory(t *testing.T) {
	dir := t.TempDir()
	target := filepath.Join(dir, "nested", "data")
	resolved, err := ensureDir(target)
	if err != nil {
		t.Fatal(err)
	}
	if resolved != target {
		t.Fatalf("resolved = %q", resolved)
	}
	if info, err := os.Stat(target); err != nil || !info.IsDir() {
		t.Fatalf("directory not created: %v", err)
	}
}

func TestResolveDataDirUsesLocalAppData(t *testing.T) {
	dir, err := resolveDataDir()
	if err != nil {
		t.Fatal(err)
	}
	if dir == "" {
		t.Fatal("empty data dir")
	}
	if !filepath.IsAbs(dir) {
		t.Fatalf("relative data dir: %q", dir)
	}
}

func TestExportStemNormalizesTR2Names(t *testing.T) {
	cases := map[string]string{
		"TR2_DD_IN":  "TR2_SW_DD_IN",
		"tr2_dd_out": "TR2_SW_DD_OUT",
		"Packet":     "Packet",
		"  spaced  ": "spaced",
	}
	for input, want := range cases {
		if got := ExportStem(input); got != want {
			t.Errorf("ExportStem(%q) = %q, want %q", input, got, want)
		}
	}
}

func TestAllowNavigationBlocksExternalSites(t *testing.T) {
	if allowNavigation("https://example.com/", false) {
		t.Fatal("external site must be blocked in production")
	}
	if !allowNavigation("https://127.0.0.1:1420/", true) {
		t.Fatal("dev server must be allowed in dev mode")
	}
	if allowNavigation("http://127.0.0.1:1420/", false) {
		t.Fatal("dev server must be blocked in production")
	}
	if !allowNavigation("mygo://host/", false) {
		t.Fatal("app scheme must be allowed")
	}
}
