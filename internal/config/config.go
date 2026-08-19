// Package config resolves the application's on-disk locations and provides
// crash-safe file writes shared across the app.
package config

import (
	"os"
	"path/filepath"
)

// appDir is the per-user directory holding command history, saved sessions,
// playbooks, device templates and saved logs.
const appDir = "aircord"

// legacyDir is the directory name used before the app was renamed to Aircord.
// Migrate moves it across so existing users keep their data.
const legacyDir = "blescan"

// root returns the app's config directory. When the OS user-config dir is
// unavailable it falls back to the temp dir rather than the process working
// directory, which is unpredictable for a double-clicked GUI app.
func root() string {
	dir, err := os.UserConfigDir()
	if err != nil || dir == "" {
		dir = os.TempDir()
	}
	return filepath.Join(dir, appDir)
}

// Dir returns the app's config directory (…/aircord/<sub…>).
func Dir(sub ...string) string {
	return filepath.Join(append([]string{root()}, sub...)...)
}

// Migrate renames a pre-Aircord config directory to the current one, so an
// upgrading user keeps their history, sessions, playbooks and templates. It is
// a no-op once the current directory exists — the legacy directory is then left
// untouched rather than merged, because a partial merge is worse than two
// directories the user can inspect.
//
// Call it once at startup, before any store reads. It deliberately reports no
// error: a failed migration must not stop the app from launching, it only means
// the user starts with empty stores and their old data is still on disk.
func Migrate() {
	cur := root()
	migrateDir(cur, filepath.Join(filepath.Dir(cur), legacyDir))
}

// migrateDir is Migrate with both paths injected, so the behaviour can be
// tested without touching the real user-config directory.
func migrateDir(cur, old string) {
	if _, err := os.Stat(cur); err == nil {
		return // already migrated (or a fresh install that has run before)
	}
	info, err := os.Stat(old)
	if err != nil || !info.IsDir() {
		return // nothing to migrate
	}
	_ = os.Rename(old, cur)
}

// AtomicWriteFile writes data to path via a temp file + rename, creating parent
// directories as needed. The rename is atomic on POSIX, so a reader never
// observes a half-written file and a failed write never clobbers the original.
func AtomicWriteFile(path string, data []byte, perm os.FileMode) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, data, perm); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}
