//! The serial-over-BLE link as the rest of the app sees it: discovery, one connection, AT lines out, reply lines in.
//! `aircord-ble` implements it over a radio; [`fake::FakeLink`] over a scripted device, for tests.
//!
//! Calls that wait (`connect`, `send_raw`) are made from a job's own thread, never the window's, as the Go build made
//! them from a goroutine. The others return at once and may be called from anywhere.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::lines::Tap;

/// One discovered peripheral.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// The key a device is known by: the platform's address (a CoreBluetooth UUID on macOS). History and sessions are
    /// keyed by it.
    pub addr: String,
    /// The advertised name; empty when it never said one.
    pub name: String,
    pub rssi: i16,
    /// When its latest advertisement came. A wait for a device to appear after some event (a reset into the
    /// bootloader) compares against it, so a stale entry from before cannot satisfy it.
    pub seen: Instant,
}

/// Where a link reports what happens on it. Called from the link's own threads.
pub trait Events: Send + Sync {
    /// A protocol line for the terminal: the GATT dump, `>> ` and `<< ` traffic, `!! ` errors.
    fn log(&self, line: &str);
    /// The link went down without being asked to (device reset, out of range).
    fn link_dropped(&self);
    /// Scanning could not start or stopped with an error.
    fn scan_failed(&self, why: &str);
}

/// The radio and its one connection.
pub trait Link: Send + Sync {
    /// Starts discovery; failures come through [`Events::scan_failed`].
    fn scan(&self);
    fn stop_scan(&self);
    fn is_scanning(&self) -> bool;
    /// What discovery has found, unsorted.
    fn devices(&self) -> Vec<Device>;
    fn clear_devices(&self);
    /// Connects to `addr`, finds its UART profile and turns on notifications; the profile's name on success. On
    /// failure the cause has already been logged as `!! …`. Waits: call it from a job's thread.
    fn connect(&self, addr: &str) -> Option<String>;
    /// Drops the connection; whether there was one.
    fn disconnect(&self) -> bool;
    fn is_connected(&self) -> bool;
    fn current_addr(&self) -> Option<String>;
    /// Sends one AT line (CRLF added), echoed to the log as `>> cmd`.
    fn send(&self, cmd: &str);
    /// [`Link::send`] for a secret, the PIN above all: the device gets it, the log gets asterisks, so it never
    /// travels into a saved log and from there into a bug report.
    fn send_secret(&self, cmd: &str);
    /// Writes `line` exactly as given (the caller adds any CRLF), logging a one-line summary instead of the payload,
    /// and waits until it is written. Call it from a job's thread.
    fn send_raw(&self, line: &str) -> Result<(), String>;
    /// Starts listening to reply lines. A dropped link ends every tap.
    fn lines(&self) -> Tap;
}

/// How often [`await_device`] looks at the discovered devices again.
pub const DEVICE_POLL: Duration = Duration::from_millis(200);

/// Waits for a device that has advertised at or after `since` and satisfies `wanted`, up to `timeout` or until `stop`
/// is set. The caller has started a scan; this only reads what it records. The `since` bound matters: discovery keeps
/// every device it has seen, so without it a node that advertised under the wanted name before (a failed attempt)
/// would be found at once and connected to while it is not advertising at all.
pub fn await_device(
    snapshot: impl Fn() -> Vec<Device>,
    wanted: impl Fn(&Device) -> bool,
    since: Instant,
    timeout: Duration,
    poll: Duration,
    stop: &AtomicBool,
) -> Option<Device> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(d) = snapshot().into_iter().find(|d| d.seen >= since && wanted(d)) {
            return Some(d);
        }
        let now = Instant::now();
        if stop.load(Ordering::Relaxed) || now >= deadline {
            return None;
        }
        std::thread::sleep(poll.min(deadline - now));
    }
}

/// The mask an echoed secret is shown as: one asterisk per character.
pub fn mask(secret: &str) -> String {
    "*".repeat(secret.chars().count())
}

/// A one-line log entry for a raw command, without its payload: `AT+TX (1234 bytes)` instead of kilobytes of hex.
pub fn summarize_line(line: &str) -> String {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    match trimmed.split_once('=') {
        Some((verb, _)) => format!("{verb} ({} bytes)", trimmed.len()),
        None => trimmed.to_string(),
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod fake;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn dev(name: &str, seen: Instant) -> Device {
        Device { addr: name.to_lowercase(), name: name.into(), rssi: -50, seen }
    }

    #[test]
    fn a_device_already_there_is_found_at_once() {
        let t0 = Instant::now();
        let found =
            await_device(|| vec![dev("X", t0)], |d| d.name == "X", t0, Duration::from_secs(5), DEVICE_POLL, &AtomicBool::new(false));
        assert_eq!(found.map(|d| d.name), Some("X".into()));
    }

    #[test]
    fn a_device_that_appears_later_is_found() {
        let t0 = Instant::now();
        let calls = Mutex::new(0);
        let snapshot = || {
            let mut n = calls.lock().unwrap();
            *n += 1;
            if *n < 3 { vec![] } else { vec![dev("X", Instant::now())] }
        };
        let found =
            await_device(snapshot, |d| d.name == "X", t0, Duration::from_secs(5), Duration::from_millis(1), &AtomicBool::new(false));
        assert!(found.is_some());
    }

    #[test]
    fn a_stale_entry_does_not_count() {
        let stale = Instant::now();
        std::thread::sleep(Duration::from_millis(2));
        let since = Instant::now();
        let found = await_device(
            || vec![dev("X", stale)],
            |d| d.name == "X",
            since,
            Duration::from_millis(20),
            Duration::from_millis(1),
            &AtomicBool::new(false),
        );
        assert!(found.is_none());
    }

    #[test]
    fn stop_ends_the_wait() {
        let t0 = Instant::now();
        let start = Instant::now();
        let found = await_device(Vec::new, |_| true, t0, Duration::from_secs(10), Duration::from_millis(1), &AtomicBool::new(true));
        assert!(found.is_none());
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_raw_line_is_summarised_and_a_secret_masked() {
        assert_eq!(summarize_line("AT+TX=3,FE0CEF\r\n"), "AT+TX (14 bytes)");
        assert_eq!(summarize_line("ATZ\r\n"), "ATZ");
        assert_eq!(mask("pín1"), "****");
    }
}
