//! A scripted device behind the [`Link`] trait, for tests of the runs here and of the window: an AT console that
//! answers `OK`, and, for an NB node, a reset into a frame-accurate bootloader that advertises under the IMEI. A
//! [`FakeLink::configured`] console also keeps settings: `AT+<KEY>=<value>` changes what its `AT+CFG` prints.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::cfg::setting_line;
use crate::lines::{Tap, Taps, lock};
use crate::link::{Device, Events, Link, mask, summarize_line};
use crate::otanb::FakeBootloader;
use crate::redact::redact_line;

/// Events that go nowhere.
pub struct NoEvents;

impl Events for NoEvents {
    fn log(&self, _: &str) {}
    fn link_dropped(&self) {}
    fn scan_failed(&self, _: &str) {}
}

#[derive(Default)]
struct State {
    /// Devices in range; a scan finds them.
    nearby: Vec<(String, String, i16)>,
    found: BTreeMap<String, Device>,
    scanning: bool,
    connected: Option<String>,
    sent: Vec<String>,
    /// Replies by exact command; anything else gets `OK`.
    replies: BTreeMap<String, Vec<String>>,
    /// The node's IMEI, when it is an NB node that resets into its bootloader.
    imei: Option<String>,
    bootloader: FakeBootloader,
    /// The lines `AT+CFG` prints before its `OK`, when the device keeps settings.
    cfg: Option<Vec<String>>,
    /// Keys whose set is refused, with the error line.
    rejects: BTreeMap<String, String>,
    /// Keys whose set is answered `OK` but not kept.
    ignored: BTreeSet<String>,
}

/// The fake link. Everything happens inline, on the caller's thread.
pub struct FakeLink {
    state: Mutex<State>,
    taps: Taps,
    events: Mutex<Arc<dyn Events>>,
}

impl FakeLink {
    /// A device `name` at `addr` that answers every command with `OK`.
    pub fn console(addr: &str, name: &str) -> FakeLink {
        let state = State { nearby: vec![(addr.into(), name.into(), -58)], ..State::default() };
        FakeLink { state: Mutex::new(state), taps: Taps::default(), events: Mutex::new(Arc::new(NoEvents)) }
    }

    /// A Dragino NB node: `AT+CFG` prints its IMEI, and `ATZ` drops the link and brings up its bootloader under the
    /// IMEI, which takes an update.
    pub fn nb_node(addr: &str, name: &str, imei: &str) -> FakeLink {
        let link = FakeLink::console(addr, name);
        {
            let mut s = lock(&link.state);
            s.imei = Some(imei.into());
            s.replies.insert(
                "AT+CFG".into(),
                vec!["Stop Tx events,Please wait for all configurations to print".into(), format!("IMEI: {imei}"), "OK".into()],
            );
        }
        link
    }

    /// A device that keeps settings: `AT+CFG` prints `dump` (up to its `OK`, blank lines and all) with the current
    /// values, and `AT+<KEY>=<value>` for a key in it answers `OK` and changes it. A set for a key the dump lacks
    /// answers `OK` and changes nothing, as a device that takes a command it does not report.
    pub fn configured(addr: &str, name: &str, dump: &str) -> FakeLink {
        let link = FakeLink::console(addr, name);
        let lines = dump.lines().take_while(|l| !l.trim().eq_ignore_ascii_case("OK")).map(str::to_string).collect();
        lock(&link.state).cfg = Some(lines);
        link
    }

    /// A set of `key` is answered with `error` (`AT_PARAM_ERROR`) and changes nothing.
    pub fn reject(&self, key: &str, error: &str) {
        lock(&self.state).rejects.insert(key.into(), error.into());
    }

    /// A set of `key` is answered `OK` and changes nothing, as firmware that quietly drops a value.
    pub fn ignore_set(&self, key: &str) {
        lock(&self.state).ignored.insert(key.into());
    }

    /// What a configured device holds for `key`, as `AT+CFG` would print it.
    pub fn cfg_value(&self, key: &str) -> Option<String> {
        let s = lock(&self.state);
        s.cfg.as_ref()?.iter().find_map(|l| setting_line(l).filter(|(k, _)| *k == key).map(|(_, v)| v.to_string()))
    }

    /// Where the link reports; a window under test passes its own.
    pub fn set_events(&self, events: Arc<dyn Events>) {
        *lock(&self.events) = events;
    }

    /// Another device in range.
    pub fn add_nearby(&self, addr: &str, name: &str, rssi: i16) {
        lock(&self.state).nearby.push((addr.into(), name.into(), rssi));
    }

    /// `cmd` is answered with `lines` (none: it stays silent).
    pub fn reply(&self, cmd: &str, lines: &[&str]) {
        lock(&self.state).replies.insert(cmd.into(), lines.iter().map(|l| l.to_string()).collect());
    }

    /// Everything sent, raw lines as their summary.
    pub fn sent(&self) -> Vec<String> {
        lock(&self.state).sent.clone()
    }

    /// What the bootloader has been flashed with, from the application start.
    pub fn bootloader_image(&self) -> Vec<u8> {
        lock(&self.state).bootloader.image.values().copied().collect()
    }

    /// The link goes down as a device reset does.
    pub fn drop_link(&self) {
        let was = lock(&self.state).connected.take().is_some();
        if was {
            self.taps.close_all();
            self.events().link_dropped();
        }
    }

    fn events(&self) -> Arc<dyn Events> {
        Arc::clone(&lock(&self.events))
    }

    fn found_now(s: &mut State) {
        let now = Instant::now();
        for (addr, name, rssi) in s.nearby.clone() {
            s.found.insert(addr.clone(), Device { addr, name, rssi, seen: now });
        }
    }

    /// The device's answer to one line, and whether it resets.
    fn answer(&self, line: &str) -> (Vec<String>, bool) {
        let mut s = lock(&self.state);
        let cmd = line.trim_end_matches(['\r', '\n']);
        if s.connected.as_deref().is_some_and(|a| a.starts_with("boot-")) {
            return (s.bootloader.answer(cmd).into_iter().collect(), false);
        }
        if cmd.eq_ignore_ascii_case("ATZ") {
            if let Some(imei) = s.imei.clone() {
                s.nearby.push((format!("boot-{imei}"), imei, -60));
                if s.scanning {
                    FakeLink::found_now(&mut s);
                }
            }
            return (Vec::new(), true);
        }
        if let Some(lines) = s.replies.get(cmd) {
            return (lines.clone(), false);
        }
        FakeLink::settings_answer(&mut s, cmd).map_or_else(|| (vec!["OK".into()], false), |lines| (lines, false))
    }

    /// A configured device's answer to `AT+CFG` or a set; none for anything else or a device without settings.
    fn settings_answer(s: &mut State, cmd: &str) -> Option<Vec<String>> {
        let State { cfg, rejects, ignored, .. } = s;
        let dump = cfg.as_mut()?;
        if cmd.trim().eq_ignore_ascii_case("AT+CFG") {
            return Some(dump.iter().cloned().chain(["OK".to_string()]).collect());
        }
        let (key, value) = setting_line(cmd)?;
        if let Some(error) = rejects.get(key) {
            return Some(vec![error.clone()]);
        }
        if !ignored.contains(key)
            && let Some(line) = dump.iter_mut().find(|l| setting_line(l).is_some_and(|(k, _)| k == key))
        {
            *line = format!("AT+{key}={}", if value.is_empty() { "NULL" } else { value });
        }
        Some(vec!["OK".into()])
    }

    fn deliver(&self, replies: Vec<String>, resets: bool) {
        let events = self.events();
        for r in replies {
            events.log(&format!("<< {}", redact_line(&r, &[])));
            self.taps.feed(&r);
        }
        if resets {
            self.drop_link();
        }
    }

    /// Sends `line`; the log and [`FakeLink::sent`] get `echo` with its passwords masked, as the real link logs it.
    fn write(&self, line: &str, echo: &str) -> bool {
        let echo = redact_line(echo, &[]);
        {
            let mut s = lock(&self.state);
            if s.connected.is_none() {
                return false;
            }
            s.sent.push(echo.to_string());
        }
        self.events().log(&format!(">> {echo}"));
        let (replies, resets) = self.answer(line);
        self.deliver(replies, resets);
        true
    }
}

impl Link for FakeLink {
    fn scan(&self) {
        let mut s = lock(&self.state);
        s.scanning = true;
        FakeLink::found_now(&mut s);
    }

    fn stop_scan(&self) {
        lock(&self.state).scanning = false;
    }

    fn is_scanning(&self) -> bool {
        lock(&self.state).scanning
    }

    fn devices(&self) -> Vec<Device> {
        lock(&self.state).found.values().cloned().collect()
    }

    fn clear_devices(&self) {
        lock(&self.state).found.clear();
    }

    fn connect(&self, addr: &str) -> Option<String> {
        let known = {
            let mut s = lock(&self.state);
            let known = s.nearby.iter().any(|(a, _, _)| a == addr);
            if known {
                s.connected = Some(addr.to_string());
            }
            known
        };
        let events = self.events();
        if known {
            events.log("-- GATT --");
            events.log("  svc 0000ffe0-0000-1000-8000-00805f9b34fb");
            events.log("    chr 0000ffe1-0000-1000-8000-00805f9b34fb");
            Some("HM-10".into())
        } else {
            events.log("!! connect failed: device not found");
            None
        }
    }

    fn disconnect(&self) -> bool {
        let was = lock(&self.state).connected.take().is_some();
        if was {
            self.taps.close_all();
        }
        was
    }

    fn is_connected(&self) -> bool {
        lock(&self.state).connected.is_some()
    }

    fn current_addr(&self) -> Option<String> {
        lock(&self.state).connected.clone()
    }

    fn send(&self, cmd: &str) {
        self.write(cmd, cmd);
    }

    fn send_secret(&self, cmd: &str) {
        self.write(cmd, &mask(cmd));
    }

    fn send_raw(&self, line: &str) -> Result<(), String> {
        if self.write(line, &summarize_line(line)) { Ok(()) } else { Err("ble: not connected".into()) }
    }

    fn lines(&self) -> Tap {
        self.taps.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use super::*;
    use crate::transfer::read_cfg;

    #[derive(Default)]
    struct Log(Mutex<Vec<String>>);

    impl Events for Log {
        fn log(&self, line: &str) {
            lock(&self.0).push(line.to_string());
        }
        fn link_dropped(&self) {}
        fn scan_failed(&self, _: &str) {}
    }

    #[test]
    fn the_log_never_shows_a_password_of_the_real_dump() {
        let link = FakeLink::configured("A", "866207050000001", include_str!("../../testdata/s31-nb-v1.4.0-cfg.txt"));
        let log = Arc::new(Log::default());
        link.set_events(log.clone());
        link.connect("A");
        let cfg = read_cfg(&link, Duration::from_millis(200), &AtomicBool::new(false)).unwrap();
        assert_eq!(cfg.get("PWORD"), Some("kxqmzp"), "the reader still gets the real value");
        link.send("AT+PWORD=neupwx");
        link.send("AT+PWD=mqtt-geheim");
        read_cfg(&link, Duration::from_millis(200), &AtomicBool::new(false)).unwrap();
        let lines = lock(&log.0).clone();
        for secret in ["kxqmzp", "neupwx", "mqtt-geheim"] {
            assert!(lines.iter().all(|l| !l.contains(secret)), "{secret} in {lines:?}");
        }
        for masked in [">> AT+PWORD=******", ">> AT+PWD=******", "<< AT+PWORD=******", "<< AT+PWD=******"] {
            assert!(lines.iter().any(|l| l == masked), "{masked} missing in {lines:?}");
        }
        assert!(lines.iter().any(|l| l == "<< AT+TDC=900"), "other lines stay as they are");
        assert!(link.sent().iter().all(|s| !s.contains("neupwx")), "{:?}", link.sent());
    }
}
