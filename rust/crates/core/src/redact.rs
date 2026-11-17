//! Masking of passwords in anything that reaches a log, a transcript or the screen. The device password
//! (`AT+PWORD`, and `AT+PIN` as the label prints it), the MQTT password (`AT+PWD`) and the OTA password (`AT+OTAPWD`)
//! are always masked, whatever a template
//! says: an S31-NB on firmware v1.4.0 prints `AT+PWORD` in clear in its `AT+CFG` dump, and a saved transcript travels
//! into bug reports. Templates add their own `type = "secret"` keys on top.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::Regex;

/// Keys whose value is masked everywhere, template or not.
pub const ALWAYS_SECRET: &[&str] = &["PWORD", "PIN", "PWD", "OTAPWD"];

/// What a secret value is shown as. Fixed length, so the mask does not give away how long the value is.
pub const MASK: &str = "******";

/// Whether `key` (as in `AT+<key>=`) is always masked.
pub fn is_always_secret(key: &str) -> bool {
    ALWAYS_SECRET.iter().any(|k| k.eq_ignore_ascii_case(key.trim()))
}

fn pattern(keys: &[&str]) -> Option<Regex> {
    let alternatives: Vec<String> = ALWAYS_SECRET.iter().chain(keys).map(|k| regex::escape(k.trim())).filter(|k| !k.is_empty()).collect();
    Regex::new(&format!(r"(?i)(AT\+(?:{})=)[^\r\n]*", alternatives.join("|"))).ok()
}

fn default_pattern() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| pattern(&[])).as_ref()
}

/// `line` with the value of every `AT+<secret>=` in it masked, from the `=` to the end of the line: a sent command
/// (`>> AT+PWD=…`), a reply line of an `AT+CFG` dump, a line the window shows. `extra` are a template's secret keys.
/// Lines without a secret come back borrowed.
pub fn redact_line<'a>(line: &'a str, extra: &[&str]) -> Cow<'a, str> {
    let owned;
    let re = if extra.is_empty() {
        match default_pattern() {
            Some(re) => re,
            None => return Cow::Owned(MASK.into()),
        }
    } else {
        match pattern(extra) {
            Some(re) => {
                owned = re;
                &owned
            }
            None => return Cow::Owned(MASK.into()),
        }
    };
    re.replace_all(line, format!("${{1}}{MASK}").as_str())
}

/// Which way a line went over the link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// From the app to the device: `>> `.
    Sent,
    /// From the device: `<< `.
    Received,
}

/// A line as a link logs it: `>> ` or `<< ` and the line with every always-secret value masked. Every [`crate::link::Link`]
/// logs through it, so the terminal and a saved transcript never show `AT+PWORD` in clear, whichever link wrote it.
pub fn log_line(direction: Direction, line: &str) -> String {
    let prefix = match direction {
        Direction::Sent => ">> ",
        Direction::Received => "<< ",
    };
    format!("{prefix}{}", redact_line(line, &[]))
}

/// Whether `cmd` sets a secret, so it goes out through [`crate::link::Link::send_secret`] and its echo is masked.
pub fn is_secret_command(cmd: &str, extra: &[&str]) -> bool {
    matches!(redact_line(cmd, extra), Cow::Owned(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_and_mqtt_passwords_are_always_masked() {
        assert_eq!(redact_line("AT+PWORD=kxqmzp", &[]), "AT+PWORD=******");
        assert_eq!(redact_line("<< AT+PWD=s3cr3t-token", &[]), "<< AT+PWD=******");
        assert_eq!(redact_line(">> at+pwd=x", &[]), ">> at+pwd=******", "case does not matter");
        assert_eq!(redact_line("AT+OTAPWD=ota", &[]), "AT+OTAPWD=******");
        assert_eq!(redact_line(">> AT+PIN=349011", &[]), ">> AT+PIN=******", "the label's form of the password");
        assert_eq!(redact_line("AT+PWD=NULL", &[]), "AT+PWD=******", "even an empty one");
        assert_eq!(redact_line("AT+PWORD=abc\r", &[]), "AT+PWORD=******\r");
    }

    #[test]
    fn a_logged_line_has_its_direction_and_no_password() {
        assert_eq!(log_line(Direction::Sent, "AT+PWD=mqtt-geheim"), ">> AT+PWD=******");
        assert_eq!(log_line(Direction::Received, "AT+PWORD=kxqmzp"), "<< AT+PWORD=******");
        assert_eq!(log_line(Direction::Received, "AT+TDC=900"), "<< AT+TDC=900");
        assert_eq!(log_line(Direction::Sent, "AT+CFG"), ">> AT+CFG");
        assert_eq!(log_line(Direction::Received, ""), "<< ");
    }

    #[test]
    fn other_lines_pass_unchanged() {
        for line in ["AT+TDC=900", "AT+UNAME=pwd", "AT+PWORDX=1", "Password Correct", "AT+PWD", "AT+PWD?", "AT+PINX=1"] {
            assert!(matches!(redact_line(line, &[]), Cow::Borrowed(l) if l == line), "{line}");
        }
    }

    #[test]
    fn a_template_adds_its_own_secrets() {
        assert_eq!(redact_line("AT+TOKEN=abc", &["TOKEN"]), "AT+TOKEN=******");
        assert_eq!(redact_line("AT+TOKEN=abc", &[]), "AT+TOKEN=abc");
        assert_eq!(redact_line("AT+PWD=abc", &["TOKEN"]), "AT+PWD=******", "the fixed ones stay");
        assert!(is_secret_command("AT+PWORD=abcdef", &[]));
        assert!(is_secret_command("AT+TOKEN=1", &["TOKEN"]));
        assert!(!is_secret_command("AT+TDC=900", &["TOKEN"]));
        assert!(is_always_secret("pwd") && !is_always_secret("UNAME"));
    }
}
