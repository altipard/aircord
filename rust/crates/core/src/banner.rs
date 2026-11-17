//! The status lines a Dragino NB node prints by itself after the password: module, IMEI, IMSI, band, APN and signal,
//! with no command sent. Measured on an S31-NB with firmware v1.4.0; lines start with a `[<ms>]` timestamp or not.
//! [`Status::feed`] takes the lines as they come, so a status card fills while the node boots.

/// Signal strength as the node reports it: the modem's CSQ, 0–31, or 99 for no network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signal(pub u8);

impl Signal {
    pub fn has_network(self) -> bool {
        self.0 != 99 && self.0 <= 31
    }

    /// The CSQ as dBm (−113 dBm at 0, 2 dB a step), when there is a network.
    pub fn dbm(self) -> Option<i16> {
        self.has_network().then(|| -113 + 2 * i16::from(self.0))
    }

    /// In words for the person in the field, by the usual CSQ bands.
    pub fn text(self) -> &'static str {
        match self.0 {
            99 => "kein Netz",
            n if n > 31 => "unbekannt",
            0..=9 => "schwach",
            10..=14 => "mittel",
            15..=19 => "gut",
            _ => "sehr gut",
        }
    }
}

/// What the boot lines said so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub password_ok: bool,
    /// The radio module, `BC660K-GL`.
    pub module: Option<String>,
    pub imei: Option<String>,
    pub imsi: Option<String>,
    /// The bands the node is set to, as printed: `8,20`.
    pub band: Option<String>,
    pub apn_set: bool,
    /// The latest signal line; the node repeats it every few seconds.
    pub signal: Option<Signal>,
}

/// A line without its leading `[<ms>]` timestamp.
fn untimed(line: &str) -> &str {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix('[')
        && let Some((stamp, after)) = rest.split_once(']')
        && !stamp.is_empty()
        && stamp.bytes().all(|b| b.is_ascii_digit())
    {
        return after.trim();
    }
    t
}

fn value_after(line: &str, prefix: &str) -> Option<String> {
    let v = line.strip_prefix(prefix)?.trim().trim_end_matches('.').trim();
    (!v.is_empty()).then(|| v.to_string())
}

impl Status {
    /// Takes one line; whether it said anything about the status.
    pub fn feed(&mut self, line: &str) -> bool {
        let l = untimed(line);
        if l.eq_ignore_ascii_case("Password Correct") {
            self.password_ok = true;
        } else if l.eq_ignore_ascii_case("Set APN successfully") {
            self.apn_set = true;
        } else if let Some(v) = value_after(l, "Model information:") {
            self.module = Some(v);
        } else if let Some(v) = value_after(l, "The IMEI number is:") {
            self.imei = Some(v);
        } else if let Some(v) = value_after(l, "The IMSI number is:") {
            self.imsi = Some(v);
        } else if let Some(v) = value_after(l, "Currently set frequency band:") {
            self.band = Some(v);
        } else if let Some(n) = value_after(l, "Signal Strength:").and_then(|v| v.parse().ok()) {
            self.signal = Some(Signal(n));
        } else {
            return false;
        }
        true
    }

    /// The status from a whole text.
    pub fn from_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Status {
        let mut s = Status::default();
        for l in lines {
            s.feed(l);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOT: &str = include_str!("../testdata/s31-nb-v1.4.0-boot.txt");

    #[test]
    fn the_real_s31_boot() {
        let s = Status::from_lines(BOOT.lines());
        assert_eq!(
            s,
            Status {
                password_ok: true,
                module: Some("BC660K-GL".into()),
                imei: Some("866207050000001".into()),
                imsi: Some("901405100000001".into()),
                band: Some("8,20".into()),
                apn_set: true,
                signal: Some(Signal(99)),
            }
        );
        assert!(!s.signal.unwrap().has_network());
        assert_eq!(s.signal.unwrap().text(), "kein Netz");
    }

    #[test]
    fn signal_lines_update_and_noise_is_ignored() {
        let mut s = Status::default();
        assert!(!s.feed("[8266]NBIOT has responded."));
        assert!(!s.feed("AT+TDC=900"));
        assert!(!s.feed("[x]Signal Strength:20"), "not a timestamp");
        assert!(s.feed("[30000]Signal Strength:18"));
        assert!(s.feed("Signal Strength: 25"));
        assert_eq!(s.signal, Some(Signal(25)));
        assert_eq!(s.signal.unwrap().dbm(), Some(-63));
        assert_eq!(s.signal.unwrap().text(), "sehr gut");
        assert_eq!((Signal(5).text(), Signal(12).text(), Signal(17).text(), Signal(40).text()), ("schwach", "mittel", "gut", "unbekannt"));
        assert_eq!(Signal(99).dbm(), None);
    }
}
