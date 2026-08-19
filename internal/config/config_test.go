package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDirJoinsAppDirAndSub(t *testing.T) {
	got := Dir("logs", "x.log")
	want := filepath.Join(appDir, "logs", "x.log")
	if !strings.HasSuffix(got, want) {
		t.Fatalf("Dir(logs, x.log) = %q, want suffix %q", got, want)
	}
	if !filepath.IsAbs(got) {
		t.Fatalf("Dir returned non-absolute path %q", got)
	}
}

func TestDirNoSubIsAppRoot(t *testing.T) {
	if got := Dir(); !strings.HasSuffix(got, appDir) {
		t.Fatalf("Dir() = %q, want suffix %q", got, appDir)
	}
}

func TestMigrateDirMovesLegacyData(t *testing.T) {
	base := t.TempDir()
	old := filepath.Join(base, legacyDir)
	cur := filepath.Join(base, appDir)
	if err := os.MkdirAll(filepath.Join(old, "logs"), 0o755); err != nil {
		t.Fatalf("seed legacy dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(old, "logs", "a.log"), []byte("keep me"), 0o644); err != nil {
		t.Fatalf("seed legacy file: %v", err)
	}

	migrateDir(cur, old)

	got, err := os.ReadFile(filepath.Join(cur, "logs", "a.log"))
	if err != nil {
		t.Fatalf("migrated file missing: %v", err)
	}
	if string(got) != "keep me" {
		t.Fatalf("content = %q, want %q", got, "keep me")
	}
	if _, err := os.Stat(old); !os.IsNotExist(err) {
		t.Fatalf("legacy dir still present (stat err: %v)", err)
	}
}

func TestMigrateDirKeepsExistingCurrentDir(t *testing.T) {
	base := t.TempDir()
	old := filepath.Join(base, legacyDir)
	cur := filepath.Join(base, appDir)
	if err := os.MkdirAll(old, 0o755); err != nil {
		t.Fatalf("seed legacy dir: %v", err)
	}
	if err := os.MkdirAll(cur, 0o755); err != nil {
		t.Fatalf("seed current dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(cur, "marker"), []byte("current"), 0o644); err != nil {
		t.Fatalf("seed current file: %v", err)
	}

	migrateDir(cur, old)

	got, err := os.ReadFile(filepath.Join(cur, "marker"))
	if err != nil || string(got) != "current" {
		t.Fatalf("current dir was clobbered (content %q, err %v)", got, err)
	}
	if _, err := os.Stat(old); err != nil {
		t.Fatalf("legacy dir should be left untouched for inspection: %v", err)
	}
}

func TestMigrateDirNoLegacyIsNoOp(t *testing.T) {
	base := t.TempDir()
	cur := filepath.Join(base, appDir)

	migrateDir(cur, filepath.Join(base, legacyDir))

	if _, err := os.Stat(cur); !os.IsNotExist(err) {
		t.Fatalf("migrate created %q out of nothing (stat err: %v)", cur, err)
	}
}

func TestAtomicWriteFileCreatesParentDirs(t *testing.T) {
	path := filepath.Join(t.TempDir(), "nested", "deep", "out.txt")
	data := []byte("hello")
	if err := AtomicWriteFile(path, data, 0o644); err != nil {
		t.Fatalf("AtomicWriteFile: %v", err)
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read back: %v", err)
	}
	if string(got) != "hello" {
		t.Fatalf("content = %q, want %q", got, "hello")
	}
}

func TestAtomicWriteFileOverwritesAndLeavesNoTemp(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "out.txt")
	if err := AtomicWriteFile(path, []byte("first"), 0o644); err != nil {
		t.Fatalf("first write: %v", err)
	}
	if err := AtomicWriteFile(path, []byte("second"), 0o644); err != nil {
		t.Fatalf("second write: %v", err)
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read back: %v", err)
	}
	if string(got) != "second" {
		t.Fatalf("content = %q, want %q", got, "second")
	}
	if _, err := os.Stat(path + ".tmp"); !os.IsNotExist(err) {
		t.Fatalf("temp file %q was left behind (stat err: %v)", path+".tmp", err)
	}
}

func TestAtomicWriteFileHonorsPerm(t *testing.T) {
	path := filepath.Join(t.TempDir(), "out.txt")
	if err := AtomicWriteFile(path, []byte("x"), 0o600); err != nil {
		t.Fatalf("AtomicWriteFile: %v", err)
	}
	fi, err := os.Stat(path)
	if err != nil {
		t.Fatalf("stat: %v", err)
	}
	if got := fi.Mode().Perm(); got != 0o600 {
		t.Fatalf("perm = %o, want %o", got, 0o600)
	}
}
