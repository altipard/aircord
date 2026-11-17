//! The app's on-disk locations, and crash-safe writes shared by every store.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The per-user directory holding history, sessions, playbooks, templates and saved logs.
const APP_DIR: &str = "aircord";

/// The directory name used before the app was renamed to Aircord; [`migrate`] moves it across.
const LEGACY_DIR: &str = "blescan";

/// The app's config directory: the OS user-config dir (`~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` or
/// `~/.config` on Linux), the same one earlier releases used, so their files carry over. Without one it falls back to the
/// temp dir, never the working directory, which is unpredictable for a double-clicked app.
pub fn root() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join(APP_DIR)
}

/// `…/aircord/<sub>`.
pub fn dir(sub: &str) -> PathBuf {
    root().join(sub)
}

/// Renames a pre-Aircord config directory to the current one, once, before any store reads. It reports nothing: a failed
/// migration must not stop the app, it only means the user starts with empty stores and the old data is still on disk.
pub fn migrate() {
    let cur = root();
    if let Some(parent) = cur.parent() {
        migrate_dir(&cur, &parent.join(LEGACY_DIR));
    }
}

/// [`migrate`] with both paths given. A no-op once `cur` exists: the legacy directory is then left alone rather than
/// merged, since a partial merge is worse than two directories the user can inspect.
pub fn migrate_dir(cur: &Path, old: &Path) {
    if cur.exists() || !old.is_dir() {
        return;
    }
    let _ = fs::rename(old, cur);
}

/// Writes `data` to `path` through a temp file and a rename, creating parent directories. A reader never sees a
/// half-written file and a failed write never clobbers the original. `mode` is the Unix permission of a new file.
pub fn atomic_write(path: &Path, data: &[u8], mode: u32) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options.open(&tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_joins_the_app_dir_and_the_sub() {
        assert!(dir("playbooks").ends_with("aircord/playbooks"));
        assert!(root().ends_with("aircord"));
    }

    #[test]
    fn migrate_moves_legacy_data() {
        let t = tempfile::tempdir().unwrap();
        let (cur, old) = (t.path().join("aircord"), t.path().join("blescan"));
        fs::create_dir_all(old.join("playbooks")).unwrap();
        fs::write(old.join("history.json"), "{}").unwrap();
        migrate_dir(&cur, &old);
        assert!(cur.join("history.json").exists());
        assert!(cur.join("playbooks").is_dir());
        assert!(!old.exists());
    }

    #[test]
    fn migrate_keeps_an_existing_current_dir() {
        let t = tempfile::tempdir().unwrap();
        let (cur, old) = (t.path().join("aircord"), t.path().join("blescan"));
        fs::create_dir_all(&cur).unwrap();
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("history.json"), "old").unwrap();
        migrate_dir(&cur, &old);
        assert!(old.join("history.json").exists(), "legacy dir left alone");
        assert!(!cur.join("history.json").exists(), "nothing merged");
    }

    #[test]
    fn migrate_without_legacy_is_a_no_op() {
        let t = tempfile::tempdir().unwrap();
        let cur = t.path().join("aircord");
        migrate_dir(&cur, &t.path().join("blescan"));
        assert!(!cur.exists());
    }

    #[test]
    fn atomic_write_creates_parents_overwrites_and_leaves_no_temp() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("a/b/c.json");
        atomic_write(&path, b"one", 0o644).unwrap();
        atomic_write(&path, b"two", 0o644).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert!(!t.path().join("a/b/c.json.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_honours_the_mode() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("secret.json");
        atomic_write(&path, b"pin", 0o600).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
