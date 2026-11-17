//! Named working sessions: the selected device and its PIN, a snapshot of the device list, and the control-panel
//! template, one JSON file each, so a scan can be put down and picked up later.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config;
use crate::slug::slug;

/// One discovered device as it was when the session was saved. Display data only: reconnecting needs a live scan.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSnapshot {
    #[serde(default)]
    pub addr: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub rssi: i16,
}

/// A saved working context.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "selectedAddr")]
    pub selected_addr: String,
    #[serde(default, rename = "selectedName")]
    pub selected_name: String,
    #[serde(default)]
    pub pin: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub devices: Vec<DeviceSnapshot>,
}

/// The sessions in a directory.
#[derive(Clone, Debug)]
pub struct Sessions {
    dir: PathBuf,
}

impl Sessions {
    /// The sessions under `<config>/aircord/sessions`.
    pub fn open() -> Sessions {
        Sessions::at(config::dir("sessions"))
    }

    pub fn at(dir: impl Into<PathBuf>) -> Sessions {
        Sessions { dir: dir.into() }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.json", slug(name, "session")))
    }

    /// Writes a session, replacing one of the same name. Mode 0600: a session holds the device PIN.
    pub fn save(&self, s: &Session) -> io::Result<()> {
        if s.name.trim().is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "session name is required"));
        }
        let json = serde_json::to_vec_pretty(s).map_err(io::Error::other)?;
        config::atomic_write(&self.path(&s.name), &json, 0o600)
    }

    pub fn load(&self, name: &str) -> io::Result<Session> {
        let bytes = fs::read(self.path(name))?;
        serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// The names of the saved sessions, sorted; files that do not read are left out rather than failing the list.
    pub fn list(&self) -> Vec<String> {
        named_files::<Session>(&self.dir, |s| s.name.clone())
    }

    /// Deletes a session; one that is not there is not an error.
    pub fn delete(&self, name: &str) -> io::Result<()> {
        remove(&self.path(name))
    }
}

/// The `name`s of the JSON files in `dir` that parse as `T` and have one, sorted.
pub(crate) fn named_files<T: for<'de> Deserialize<'de>>(dir: &Path, name: impl Fn(&T) -> String) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json") && e.path().is_file())
        .filter_map(|e| fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice::<T>(&b).ok())
        .map(|t| name(&t))
        .filter(|n| !n.is_empty())
        .collect();
    names.sort();
    names
}

/// Removes a file; one that is not there is not an error.
pub(crate) fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> Session {
        Session {
            name: name.into(),
            selected_addr: "AA-BB".into(),
            selected_name: "D20-NB".into(),
            pin: "349011".into(),
            template: "Dragino NB-IoT".into(),
            devices: vec![DeviceSnapshot { addr: "AA-BB".into(), name: "D20-NB".into(), rssi: -61 }],
        }
    }

    #[test]
    fn save_load_round_trip() {
        let t = tempfile::tempdir().unwrap();
        let s = Sessions::at(t.path());
        s.save(&sample("Field A")).unwrap();
        assert_eq!(s.load("Field A").unwrap(), sample("Field A"));
    }

    #[test]
    fn a_blank_name_is_refused() {
        let t = tempfile::tempdir().unwrap();
        assert!(Sessions::at(t.path()).save(&sample("  ")).is_err());
    }

    #[test]
    fn list_is_sorted_and_delete_is_idempotent() {
        let t = tempfile::tempdir().unwrap();
        let s = Sessions::at(t.path());
        s.save(&sample("b")).unwrap();
        s.save(&sample("a")).unwrap();
        fs::write(t.path().join("junk.json"), "nope").unwrap();
        assert_eq!(s.list(), ["a", "b"]);
        s.delete("a").unwrap();
        s.delete("a").unwrap();
        assert_eq!(s.list(), ["b"]);
    }

    #[test]
    fn an_unsafe_name_stays_inside_the_dir() {
        let t = tempfile::tempdir().unwrap();
        let s = Sessions::at(t.path().join("sessions"));
        s.save(&sample("../../escape")).unwrap();
        assert!(t.path().join("sessions/escape.json").exists());
    }

    #[test]
    fn it_reads_the_go_build_s_files() {
        let json =
            r#"{"name":"x","selectedAddr":"a","selectedName":"n","pin":"1","template":"t","devices":[{"addr":"a","name":"n","rssi":-50}]}"#;
        let s: Session = serde_json::from_str(json).unwrap();
        assert_eq!(s.devices[0].rssi, -50);
        assert_eq!(s.selected_addr, "a");
    }

    #[cfg(unix)]
    #[test]
    fn a_session_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        Sessions::at(t.path()).save(&sample("p")).unwrap();
        let mode = fs::metadata(t.path().join("p.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
