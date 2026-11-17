//! Per-device command history, kept on disk so earlier commands come back when the same device is connected again.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::config;

/// How many commands are kept per device.
pub const MAX_PER_DEVICE: usize = 300;

/// The history file: device address to commands, oldest first.
pub struct History {
    path: PathBuf,
    data: BTreeMap<String, Vec<String>>,
    load_error: Option<String>,
}

impl History {
    /// The history at `<config>/aircord/history.json`.
    pub fn open() -> History {
        History::at(config::dir("history.json"))
    }

    /// The history kept at `path`. A file that does not parse is moved aside as `.corrupt`, so the next save cannot
    /// overwrite what may still be recovered, and the reason is kept for [`History::load_error`]: the app still starts.
    pub fn at(path: PathBuf) -> History {
        let mut h = History { path, data: BTreeMap::new(), load_error: None };
        if let Ok(bytes) = fs::read(&h.path) {
            match serde_json::from_slice(&bytes) {
                Ok(data) => h.data = data,
                Err(e) => {
                    let mut aside = h.path.as_os_str().to_owned();
                    aside.push(".corrupt");
                    let _ = fs::rename(&h.path, PathBuf::from(aside));
                    h.load_error = Some(e.to_string());
                }
            }
        }
        h
    }

    /// Why the file could not be read at start, if it could not.
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// The commands sent to `addr`, oldest first.
    pub fn get(&self, addr: &str) -> Vec<String> {
        self.data.get(addr).cloned().unwrap_or_default()
    }

    /// Records `cmd` for `addr` (a repeat of the last one is not recorded again), keeps the newest
    /// [`MAX_PER_DEVICE`], and writes the file.
    pub fn append(&mut self, addr: &str, cmd: &str) -> std::io::Result<()> {
        let list = self.data.entry(addr.to_string()).or_default();
        if list.last().is_none_or(|last| last != cmd) {
            list.push(cmd.to_string());
        }
        if list.len() > MAX_PER_DEVICE {
            let cut = list.len() - MAX_PER_DEVICE;
            list.drain(..cut);
        }
        let json = serde_json::to_vec_pretty(&self.data).map_err(std::io::Error::other)?;
        config::atomic_write(&self.path, &json, 0o644)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> (tempfile::TempDir, History) {
        let t = tempfile::tempdir().unwrap();
        let h = History::at(t.path().join("history.json"));
        (t, h)
    }

    #[test]
    fn a_fresh_store_has_no_load_error_and_no_history() {
        let (_t, h) = temp();
        assert!(h.load_error().is_none());
        assert!(h.get("x").is_empty());
    }

    #[test]
    fn append_skips_consecutive_duplicates_only() {
        let (_t, mut h) = temp();
        for c in ["AT", "AT", "AT+CFG", "AT"] {
            h.append("dev", c).unwrap();
        }
        assert_eq!(h.get("dev"), ["AT", "AT+CFG", "AT"]);
    }

    #[test]
    fn append_caps_per_device_and_keeps_devices_apart() {
        let (_t, mut h) = temp();
        for i in 0..MAX_PER_DEVICE + 5 {
            h.append("a", &format!("cmd{i}")).unwrap();
        }
        h.append("b", "other").unwrap();
        let a = h.get("a");
        assert_eq!(a.len(), MAX_PER_DEVICE);
        assert_eq!(a[0], "cmd5");
        assert_eq!(h.get("b"), ["other"]);
    }

    #[test]
    fn it_persists_across_instances() {
        let (t, mut h) = temp();
        h.append("dev", "AT+VER").unwrap();
        let again = History::at(t.path().join("history.json"));
        assert_eq!(again.get("dev"), ["AT+VER"]);
    }

    #[test]
    fn a_corrupt_file_is_backed_up_and_surfaced() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("history.json");
        fs::write(&path, "{not json").unwrap();
        let h = History::at(path.clone());
        assert!(h.load_error().is_some());
        assert!(t.path().join("history.json.corrupt").exists());
        assert!(!path.exists());
    }
}
