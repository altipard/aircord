//! The device's current settings as `AT+CFG` prints them: one `AT+<KEY>=<value>` line per setting, then `OK`. Measured
//! on an S31-NB with firmware v1.4.0 (`docs/devices/s31-nb-cfg-sample.md`): the first line is
//! `AT+MODEL=<model>,v<version>`, `NULL` stands for an unset value, and blank lines turn up in the middle. Anything else
//! (an echo, a banner, a `Signal Strength:99` the device prints every few seconds) is not a setting and is skipped.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::redact;

/// What one `AT+CFG` dump said.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Cfg {
    /// The model from `AT+MODEL=S31-NB,v1.4.0`: `S31-NB`.
    pub model: Option<String>,
    /// The firmware version from the same line, without its `v`: `1.4.0`.
    pub firmware: Option<String>,
    /// Whether the closing `OK` came; a dump cut short by a drop or a timeout is not the whole state.
    pub complete: bool,
    values: BTreeMap<String, String>,
    /// Keys masked in `{:?}` on top of the always-secret ones: a template's `type = "secret"` settings.
    hidden: BTreeSet<String>,
}

/// `AT+<KEY>=<value>` as key and value, or nothing for any other line.
pub fn setting_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("AT+")?;
    let (key, value) = rest.split_once('=')?;
    crate::settings::valid_key(key).then_some((key, value.trim()))
}

fn is_ok(line: &str) -> bool {
    line.trim().eq_ignore_ascii_case("OK")
}

impl Cfg {
    /// Reads a dump, up to and including its `OK`; lines after that are not part of it.
    pub fn parse<'a>(lines: impl IntoIterator<Item = &'a str>) -> Cfg {
        let mut cfg = Cfg::default();
        for line in lines {
            if is_ok(line) {
                cfg.complete = true;
                break;
            }
            if let Some((key, value)) = setting_line(line) {
                let value = if value == "NULL" { "" } else { value };
                cfg.values.insert(key.to_string(), value.to_string());
            }
        }
        if let Some(line) = cfg.values.get("MODEL") {
            let (model, version) = line.split_once(',').unwrap_or((line.as_str(), ""));
            let version = version.trim();
            cfg.model = Some(model.trim().to_string()).filter(|m| !m.is_empty());
            cfg.firmware = Some(version.strip_prefix(['v', 'V']).unwrap_or(version).to_string()).filter(|v| !v.is_empty());
        }
        cfg
    }

    /// [`Cfg::parse`] over the lines of one text.
    pub fn parse_text(text: &str) -> Cfg {
        Cfg::parse(text.lines())
    }

    /// A setting's value; `NULL` reads as empty. `None`: the device did not report the key at all.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// The value of the `AT+MODEL` line, `S31-NB,v1.4.0`, which a template's `model` pattern is tested against.
    pub fn model_line(&self) -> Option<&str> {
        self.get("MODEL")
    }

    /// Every key, sorted.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Masks these keys in `{:?}` as well; [`crate::device::Template::protect`] passes a template's secrets.
    pub fn hide<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) {
        self.hidden.extend(keys.into_iter().map(str::to_string));
    }

    fn is_hidden(&self, key: &str) -> bool {
        redact::is_always_secret(key) || self.hidden.contains(key)
    }
}

/// Lists the values with every secret one masked (the always-secret keys and those given to [`Cfg::hide`]), so a
/// `{:?}` in a log can't leak a password.
impl fmt::Debug for Cfg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown: BTreeMap<&str, &str> =
            self.values.iter().map(|(k, v)| (k.as_str(), if self.is_hidden(k) { redact::MASK } else { v.as_str() })).collect();
        f.debug_struct("Cfg")
            .field("model", &self.model)
            .field("firmware", &self.firmware)
            .field("complete", &self.complete)
            .field("values", &shown)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const S31: &str = include_str!("../testdata/s31-nb-v1.4.0-cfg.txt");

    #[test]
    fn the_real_s31_dump() {
        let cfg = Cfg::parse_text(S31);
        assert!(cfg.complete);
        assert_eq!(cfg.model.as_deref(), Some("S31-NB"));
        assert_eq!(cfg.firmware.as_deref(), Some("1.4.0"));
        assert_eq!(cfg.model_line(), Some("S31-NB,v1.4.0"));
        assert_eq!(cfg.len(), 35, "every AT+ line of the dump, MODEL included");
        assert_eq!(cfg.get("TDC"), Some("900"));
        assert_eq!(cfg.get("SERVADDR"), Some("udp.os.1nce.com,4445"));
        assert_eq!(cfg.get("PRO"), Some("2,5"));
        assert_eq!(cfg.get("APN"), Some("iot.1nce.net"));
        assert_eq!(cfg.get("DNSCFG"), Some("\"8.8.8.8\",\"8.8.4.4\""));
        assert_eq!(cfg.get("BKDNS"), Some("1,0,NULL"), "only a whole NULL is empty");
        assert_eq!(cfg.get("SHTEMP"), Some("0,0"));
        assert_eq!(cfg.get("CLOCKLOG"), Some("1,65535,15,8"));
        assert_eq!(cfg.get("PWD"), Some(""), "NULL is empty");
        assert_eq!(cfg.get("QCOPS"), Some(""));
        assert_eq!(cfg.get("PWORD"), Some("kxqmzp"));
        assert_eq!(cfg.get("QBAND"), None, "the band is not in the dump");
    }

    #[test]
    fn debug_masks_passwords() {
        let mut cfg = Cfg::parse_text(&S31.replace("AT+UNAME=NULL", "AT+UNAME=token-123"));
        let shown = format!("{cfg:?}");
        assert!(!shown.contains("kxqmzp"), "{shown}");
        assert!(shown.contains("\"PWORD\": \"******\"") && shown.contains("\"TDC\": \"900\""), "{shown}");
        cfg.hide(["UNAME"]);
        let shown = format!("{cfg:?}");
        assert!(!shown.contains("token-123") && shown.contains("\"UNAME\": \"******\""), "{shown}");
        assert_eq!(cfg.get("UNAME"), Some("token-123"), "hiding is for display only");
    }

    #[test]
    fn echo_banner_and_noise_are_skipped() {
        let text = "AT+CFG\nStop Tx events,Please wait for all configurations to print\n\nAT+MODEL=D20-NB,v1.3.4\n[28891]Signal Strength:99\n\
                    AT+TDC=7200\n  AT+TR=900  \nAT+lower=1\nIMEI: 866207050000001\nOK\nAT+TDC=1";
        let cfg = Cfg::parse_text(text);
        assert!(cfg.complete);
        assert_eq!(cfg.keys().collect::<Vec<_>>(), ["MODEL", "TDC", "TR"]);
        assert_eq!(cfg.get("TDC"), Some("7200"), "what follows OK is not part of the dump");
        assert_eq!(cfg.get("TR"), Some("900"));
    }

    #[test]
    fn a_dump_without_ok_is_incomplete_and_a_model_without_version_is_kept() {
        let cfg = Cfg::parse_text("AT+MODEL=WQS-NB\nAT+TDC=600");
        assert!(!cfg.complete);
        assert_eq!(cfg.model.as_deref(), Some("WQS-NB"));
        assert_eq!(cfg.firmware, None);
        assert!(Cfg::parse_text("ok").complete, "a lower-case ok closes it too");
        assert!(Cfg::parse_text("").is_empty());
    }
}
