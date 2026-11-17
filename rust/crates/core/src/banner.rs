//! The status lines a Dragino NB node prints by itself after the password: module, IMEI, IMSI, band, APN and signal,
//! with no command sent. Measured on an S31-NB with firmware v1.4.0; lines start with a `[<ms>]` timestamp or not.
//! [`Status::feed`] takes the lines as they come, so a status card fills while the node boots.
//!
//! [`NodeStatus`] reads the same lines and more: the network (`Signal Strength`, `Network Information`), the steps to
//! the server, and the upload block a node prints for every uplink, with battery and readings. Measured on the same
//! S31-NB (`docs/devices/s31-nb-cfg-sample.md`, testdata `s31-nb-v1.4.0-upload.txt`).

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

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

/// The cell the node is attached to, from `Network Information:"RSSI:-80","SINR:5","BAND:8"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub rssi_dbm: Option<i16>,
    /// Signal to interference and noise, as printed (dB by the usual meaning; the firmware does not say).
    pub sinr: Option<i16>,
    pub band: Option<u16>,
}

/// Whether the node has a network, as far as its own lines tell. Each state follows from a line it printed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Network {
    /// No line about the network yet.
    #[default]
    Unknown,
    /// The modem started (`NB module is initializing`) and has reported no signal yet.
    Searching { since: SystemTime },
    /// `Signal Strength:99`. `since` is when the modem started searching, or the first 99 after a signal.
    NoNetwork { since: SystemTime },
    /// A signal of 0–31; the cell once `Network Information` came.
    Connected { signal: Signal, cell: Option<Cell> },
}

impl Network {
    /// How long the node has been without a network at `now`: searching or at 99. None while connected or unknown.
    /// When that is long enough for a hint ("Antenne angeschraubt?") is the window's call.
    pub fn without_network_for(&self, now: SystemTime) -> Option<Duration> {
        match self {
            Network::Searching { since } | Network::NoNetwork { since } => Some(now.duration_since(*since).unwrap_or_default()),
            Network::Unknown | Network::Connected { .. } => None,
        }
    }
}

/// How the battery stands, by [`Battery::level`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryLevel {
    Good,
    Low,
    Critical,
}

impl BatteryLevel {
    /// In words for the person in the field.
    pub fn text(self) -> &'static str {
        match self {
            BatteryLevel::Good => "gut",
            BatteryLevel::Low => "schwach",
            BatteryLevel::Critical => "fast leer",
        }
    }
}

/// The battery voltage from `remaining battery =3541 mv`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub mv: u32,
}

impl Battery {
    pub fn volts(self) -> f64 {
        f64::from(self.mv) / 1000.0
    }

    /// The level by voltage. ASSUMPTION, neither measured nor taken from Dragino's documentation: from 3.3 V good,
    /// from 3.1 V low, below that critical. The device test read 3.541 V on a node in use. Check the thresholds
    /// against the battery's discharge curve before the window turns them into advice.
    pub fn level(self) -> BatteryLevel {
        match self.mv {
            3300.. => BatteryLevel::Good,
            3100..3300 => BatteryLevel::Low,
            _ => BatteryLevel::Critical,
        }
    }
}

/// The measurements an upload block printed. Missing lines stay empty: nothing is guessed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Readings {
    pub battery: Option<Battery>,
    /// External DS18B20 probes by their number, in °C. `None`: no probe connected (the node prints `-409.5`).
    pub probes: BTreeMap<u8, Option<f64>>,
    /// ADC inputs by their number, in mV.
    pub adc_mv: BTreeMap<u8, f64>,
    /// The built-in sensor's humidity, in %rh.
    pub humidity: Option<f64>,
    /// The built-in sensor's temperature, in °C.
    pub temperature: Option<f64>,
}

/// How an upload went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadResult {
    /// The block is open and no success line came yet.
    Running,
    /// A success line came: `Upload data successfully`, `Datagram is sent by RF` or `Send complete`.
    Sent,
    /// The block ended (`*****End of upload*****`) without a success line.
    Failed,
    /// The block never ended: a new upload or a modem restart came first, and no success line.
    Interrupted,
}

/// One upload block, `*****Upload start:<n>*****` to `*****End of upload*****`.
#[derive(Clone, Debug, PartialEq)]
pub struct Upload {
    /// The counter after `Upload start:`.
    pub number: u32,
    /// When the start line came, by the caller's clock.
    pub started: SystemTime,
    /// When the end line came; none while running or when interrupted.
    pub ended: Option<SystemTime>,
    pub result: UploadResult,
    pub readings: Readings,
}

/// The server address the node resolved: `Domain IP:10.60.2.239,4445`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub ip: String,
    pub port: Option<u16>,
}

/// What a node's own lines say about it, for a status card: the boot facts ([`Status`]), the network, the way to the
/// server, the latest upload and the battery. [`NodeStatus::feed`] takes each line with the time it arrived, so the
/// window can say how long ago something happened; the parser never reads a clock. Lines it does not know are ignored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeStatus {
    pub boot: Status,
    pub network: Network,
    /// `PSM mode configured`: the modem may sleep between uplinks.
    pub psm: bool,
    /// `DNS configuration is successful`.
    pub dns_ok: bool,
    pub server: Option<Server>,
    /// The upload block being printed now.
    pub upload: Option<Upload>,
    /// The latest upload that is over (sent, failed or interrupted).
    pub last_upload: Option<Upload>,
    /// The latest battery reading, from any upload block.
    pub battery: Option<Battery>,
}

/// A probe's value for "no probe connected".
const NO_PROBE: f64 = -409.5;

/// `<key> =<value> <unit>` or `<key>:<value>` as a number: `remaining battery =3541 mv`, `Humidity =46.08 %rh`.
fn number_after<T: std::str::FromStr>(line: &str, key: &str, unit: &str) -> Option<T> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix(['=', ':'])?.trim();
    let rest = if unit.is_empty() { rest } else { rest.strip_suffix(unit)?.trim_end() };
    rest.parse().ok()
}

/// `<prefix><n>)<rest>` as `n` and the rest: `DS18B20(1) temp is -409.5`, `adc_mV(1):0.00`.
fn indexed<'a>(line: &'a str, prefix: &str) -> Option<(u8, &'a str)> {
    let (n, rest) = line.strip_prefix(prefix)?.split_once(')')?;
    Some((n.trim().parse().ok()?, rest))
}

fn cell(text: &str) -> Cell {
    let mut cell = Cell::default();
    for part in text.split(',') {
        let Some((key, value)) = part.trim().trim_matches('"').split_once(':') else { continue };
        let value = value.trim();
        match key.trim().to_ascii_uppercase().as_str() {
            "RSSI" => cell.rssi_dbm = value.parse().ok(),
            "SINR" => cell.sinr = value.parse().ok(),
            "BAND" => cell.band = value.parse().ok(),
            _ => {}
        }
    }
    cell
}

impl NodeStatus {
    /// Takes one line that arrived `at`; whether it said anything about the node.
    pub fn feed(&mut self, line: &str, at: SystemTime) -> bool {
        let boot = self.boot.feed(line);
        self.feed_own(untimed(line), at) || boot
    }

    /// The status from lines that all arrived `at`.
    pub fn from_lines<'a>(lines: impl IntoIterator<Item = &'a str>, at: SystemTime) -> NodeStatus {
        let mut s = NodeStatus::default();
        for l in lines {
            s.feed(l, at);
        }
        s
    }

    /// Ends the open upload block without its end line.
    fn interrupt_upload(&mut self) {
        if let Some(mut up) = self.upload.take() {
            if up.result == UploadResult::Running {
                up.result = UploadResult::Interrupted;
            }
            self.last_upload = Some(up);
        }
    }

    fn feed_own(&mut self, l: &str, at: SystemTime) -> bool {
        if l.starts_with("NB module is initializing") {
            self.interrupt_upload();
            self.network = Network::Searching { since: at };
            self.psm = false;
            self.dns_ok = false;
            self.server = None;
        } else if let Some(n) = value_after(l, "Signal Strength:").and_then(|v| v.parse::<u8>().ok()) {
            let signal = Signal(n);
            self.network = match self.network {
                _ if signal.has_network() => {
                    let cell = if let Network::Connected { cell, .. } = self.network { cell } else { None };
                    Network::Connected { signal, cell }
                }
                _ if n != 99 => return false,
                Network::Searching { since } | Network::NoNetwork { since } => Network::NoNetwork { since },
                Network::Unknown | Network::Connected { .. } => Network::NoNetwork { since: at },
            };
        } else if let Some(rest) = l.strip_prefix("Network Information:") {
            if let Network::Connected { signal, .. } = self.network {
                self.network = Network::Connected { signal, cell: Some(cell(rest)) };
            }
        } else if l.eq_ignore_ascii_case("PSM mode configured") {
            self.psm = true;
        } else if l.eq_ignore_ascii_case("DNS configuration is successful") {
            self.dns_ok = true;
        } else if let Some(rest) = l.strip_prefix("Domain IP:") {
            let (ip, port) = rest.split_once(',').unwrap_or((rest, ""));
            if ip.trim().is_empty() {
                return false;
            }
            self.server = Some(Server { ip: ip.trim().to_string(), port: port.trim().parse().ok() });
        } else if let Some(n) =
            l.strip_prefix("*****Upload start:").and_then(|r| r.strip_suffix("*****")).and_then(|n| n.trim().parse().ok())
        {
            self.interrupt_upload();
            self.upload =
                Some(Upload { number: n, started: at, ended: None, result: UploadResult::Running, readings: Readings::default() });
        } else {
            return self.feed_upload(l, at);
        }
        true
    }

    /// A line inside an open upload block; outside one, readings and success lines are not taken.
    fn feed_upload(&mut self, l: &str, at: SystemTime) -> bool {
        let Some(up) = self.upload.as_mut() else { return false };
        let r = &mut up.readings;
        if let Some(mv) = number_after::<u32>(l, "remaining battery", "mv") {
            r.battery = Some(Battery { mv });
            self.battery = Some(Battery { mv });
        } else if let Some((i, rest)) = indexed(l, "DS18B20(") {
            let Some(v) = rest.trim().strip_prefix("temp is").and_then(|v| v.trim().parse::<f64>().ok()) else { return false };
            r.probes.insert(i, ((v - NO_PROBE).abs() > 0.05).then_some(v));
        } else if let Some((i, rest)) = indexed(l, "adc_mV(") {
            let Some(v) = rest.trim().strip_prefix(':').and_then(|v| v.trim().parse().ok()) else { return false };
            r.adc_mv.insert(i, v);
        } else if let Some(v) = number_after(l, "Humidity", "%rh") {
            r.humidity = Some(v);
        } else if let Some(v) = number_after(l, "tem", "C") {
            r.temperature = Some(v);
        } else if UPLOAD_SENT.iter().any(|s| l.eq_ignore_ascii_case(s)) {
            up.result = UploadResult::Sent;
        } else if l == "*****End of upload*****" {
            up.ended = Some(at);
            if up.result == UploadResult::Running {
                up.result = UploadResult::Failed;
            }
            self.last_upload = self.upload.take();
        } else {
            return false;
        }
        true
    }
}

/// The lines that say an upload went out.
const UPLOAD_SENT: &[&str] = &["Upload data successfully", "Datagram is sent by RF", "Send complete"];

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

    const UPLOAD: &str = include_str!("../testdata/s31-nb-v1.4.0-upload.txt");

    fn t(s: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + s)
    }

    /// Feeds each line one second after the one before, from `t(0)`.
    fn replay(text: &str) -> NodeStatus {
        let mut s = NodeStatus::default();
        for (i, l) in text.lines().enumerate() {
            s.feed(l, t(i as u64));
        }
        s
    }

    #[test]
    fn the_real_s31_upload() {
        let s = replay(UPLOAD);
        assert_eq!(
            s.network,
            Network::Connected { signal: Signal(17), cell: Some(Cell { rssi_dbm: Some(-80), sinr: Some(5), band: Some(8) }) }
        );
        assert!(s.psm && s.dns_ok);
        assert_eq!(s.server, Some(Server { ip: "10.60.2.239".into(), port: Some(4445) }));
        assert_eq!(s.upload, None, "the block is closed");
        let up = s.last_upload.clone().unwrap();
        assert_eq!((up.number, up.started, up.ended, up.result), (0, t(12), Some(t(23)), UploadResult::Sent));
        assert_eq!(
            up.readings,
            Readings {
                battery: Some(Battery { mv: 3541 }),
                probes: [(1, None)].into(),
                adc_mv: [(1, 0.0)].into(),
                humidity: Some(46.08),
                temperature: Some(23.56),
            },
            "-409.5 is no probe, not a temperature"
        );
        assert_eq!(s.battery, Some(Battery { mv: 3541 }));
        assert_eq!((Battery { mv: 3541 }.volts(), Battery { mv: 3541 }.level()), (3.541, BatteryLevel::Good));
        assert_eq!(s.network.without_network_for(t(100)), None);
        assert_eq!((s.boot.imsi.as_deref(), s.boot.signal), (Some("901405100000001"), Some(Signal(17))), "the boot status is fed too");
        assert_eq!(s.boot.imei, None, "the log starts after the IMEI line");
    }

    #[test]
    fn the_real_s31_boot_without_network() {
        let s = replay(BOOT);
        assert_eq!(s.network, Network::NoNetwork { since: t(0) }, "searching since the modem started");
        assert_eq!(s.network.without_network_for(t(300)), Some(Duration::from_secs(300)));
        assert_eq!(s.boot, Status::from_lines(BOOT.lines()));
        assert!(!s.psm && !s.dns_ok && s.server.is_none() && s.last_upload.is_none() && s.battery.is_none());
    }

    #[test]
    fn the_network_follows_the_lines() {
        let mut s = NodeStatus::default();
        assert!(!s.feed("[1]Exit sleep mode", t(0)));
        assert_eq!(s.network, Network::Unknown);
        assert!(s.feed("[2]NB module is initializing...", t(1)));
        assert_eq!(s.network, Network::Searching { since: t(1) });
        assert_eq!(s.network.without_network_for(t(11)), Some(Duration::from_secs(10)));
        s.feed("Network Information:\"RSSI:-80\",\"SINR:5\",\"BAND:8\"", t(2));
        assert_eq!(s.network, Network::Searching { since: t(1) }, "no cell without a signal");
        s.feed("Signal Strength:12", t(3));
        s.feed("Network Information:\"RSSI:-95\",\"BAND:20\"", t(4));
        s.feed("Signal Strength:14", t(5));
        assert_eq!(
            s.network,
            Network::Connected { signal: Signal(14), cell: Some(Cell { rssi_dbm: Some(-95), sinr: None, band: Some(20) }) }
        );
        s.feed("Signal Strength:40", t(6));
        assert_eq!(
            s.network,
            Network::Connected { signal: Signal(14), cell: Some(Cell { rssi_dbm: Some(-95), sinr: None, band: Some(20) }) }
        );
        s.feed("Signal Strength:99", t(7));
        s.feed("Signal Strength:99", t(17));
        assert_eq!(s.network, Network::NoNetwork { since: t(7) }, "since the first 99 after a signal");
        let mut fresh = NodeStatus::default();
        fresh.feed("Signal Strength:99", t(30));
        assert_eq!(fresh.network, Network::NoNetwork { since: t(30) });
    }

    #[test]
    fn an_upload_without_success_fails_and_an_unfinished_one_is_interrupted() {
        let mut s = NodeStatus::default();
        assert!(!s.feed("remaining battery =3300 mv", t(0)), "readings count only inside a block");
        assert!(!s.feed("Send complete", t(0)));
        s.feed("*****Upload start:4*****", t(1));
        s.feed("remaining battery =3050 mv", t(2));
        assert!(!s.feed("Open a Socket Service failed", t(3)), "unknown lines are tolerated");
        assert_eq!(s.upload.as_ref().map(|u| u.result), Some(UploadResult::Running));
        s.feed("*****End of upload*****", t(4));
        let up = s.last_upload.clone().unwrap();
        assert_eq!((up.number, up.result, up.ended), (4, UploadResult::Failed, Some(t(4))));
        assert_eq!(s.battery.map(Battery::level), Some(BatteryLevel::Critical));

        s.feed("*****Upload start:5*****", t(5));
        s.feed("DS18B20(2) temp is 21.5", t(6));
        s.feed("*****Upload start:6*****", t(7));
        let up = s.last_upload.clone().unwrap();
        assert_eq!((up.number, up.result, up.ended), (5, UploadResult::Interrupted, None));
        assert_eq!(up.readings.probes, [(2, Some(21.5))].into());
        s.feed("Datagram is sent by RF", t(8));
        s.feed("[9]NB module is initializing...", t(9));
        assert_eq!(s.last_upload.as_ref().map(|u| (u.number, u.result)), Some((6, UploadResult::Sent)), "sent, then cut off");
        assert!(s.upload.is_none());
    }

    #[test]
    fn battery_levels() {
        let level = |mv| Battery { mv }.level();
        assert_eq!(
            [level(3300), level(3299), level(3100), level(3099)],
            [BatteryLevel::Good, BatteryLevel::Low, BatteryLevel::Low, BatteryLevel::Critical]
        );
        assert_eq!((BatteryLevel::Good.text(), BatteryLevel::Low.text(), BatteryLevel::Critical.text()), ("gut", "schwach", "fast leer"));
    }
}
