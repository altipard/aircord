//! A firmware update of a Dragino NB-IoT node over BLE: the dance around the bootloader's short window, in the order
//! Dragino's own tool does it.
//!
//! 1. reset the node: ATZ over the open link, or the user presses RESET;
//! 2. scan, and wait for a device advertising as the IMEI, seen after the reset;
//! 3. connect to it and run [`otanb::upgrade`]: erase, flash, verify, reboot.
//!
//! The bootloader listens only right after a reset, 8 to 15 s, advertising under the IMEI. [`run`] is written against
//! [`Link`], so it is tested end to end against a fake node.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use regex::Regex;

use crate::lines::{NoLine, Tap};
use crate::link::{self, Link};
use crate::otanb::{self, Plan};

/// Reboots the node, which takes it through the bootloader.
pub const RESET_CMD: &str = "ATZ";
/// Prints the configuration, the IMEI with it.
pub const READ_IMEI_CMD: &str = "AT+CFG";
/// The most of a chosen file that is read: the application region is about 223 KiB.
pub const IMAGE_LIMIT: u64 = 1 << 20;

/// How long each wait of an update lasts. [`Timing::REAL`] in the app; tests run faster.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// For the node to re-advertise under its IMEI: the documented 8–15 s, plus the reboot and scan latency.
    pub bootloader_window: Duration,
    /// For the node to drop the link after ATZ before this side closes it.
    pub reset_drop: Duration,
    pub poll: Duration,
}

impl Timing {
    pub const REAL: Timing =
        Timing { bootloader_window: Duration::from_secs(20), reset_drop: Duration::from_secs(3), poll: link::DEVICE_POLL };
}

/// Shown in the dialog for as long as it is open: the reset is timing-critical and nobody should have to remember it.
pub const HINT: &str = "1. Start — Aircord scans for the node's bootloader\n\
2. The node resets: ATZ over the link, or press its RESET key\n\
3. For 8–15 s it advertises as the IMEI — Aircord connects\n\
4. Erase, flash, verify, reboot\n\
\n\
A node reset less than two minutes ago may skip the bootloader.\n\
The .bin must be the application only, without the bootloader.";

fn imei_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    // ASCII classes on purpose: Rust's `\d` is any Unicode digit, an IMEI only 0-9.
    RE.get_or_init(|| Regex::new(r"(?i)IMEI[^0-9]{0,8}([0-9]{15})").ok()).as_ref()
}

/// The IMEI in a configuration dump line such as `IMEI: 8606…`.
pub fn extract_imei(line: &str) -> Option<String> {
    imei_re()?.captures(line).and_then(|c| c.get(1)).map(|m| m.as_str().to_string())
}

/// Whether an advertised name is the wanted node's bootloader: the bare IMEI normally, or a name containing it (a
/// firmware that adds a prefix), but only for an IMEI long enough that the substring can't be a coincidence.
pub fn matches_imei(name: &str, imei: &str) -> bool {
    let (name, imei) = (name.trim(), imei.trim());
    if imei.is_empty() {
        return false;
    }
    name.eq_ignore_ascii_case(imei) || (imei.len() >= 8 && name.contains(imei))
}

/// Whether a device name is a bare IMEI: how a node already in its bootloader shows in the list.
pub fn looks_like_imei(name: &str) -> bool {
    name.len() == 15 && name.bytes().all(|b| b.is_ascii_digit())
}

/// The console PIN as the bootloader's password: the fixed default without one, else the padded PIN.
pub fn password(pin: &str) -> Result<[u8; 8], otanb::Error> {
    match pin.trim() {
        "" => Ok(otanb::default_password()),
        pin => otanb::encode_pin(pin),
    }
}

/// Everything an update needs, checked before it starts.
#[derive(Clone, Debug)]
pub struct Job {
    pub plan: Plan,
    /// The image's file name, for messages.
    pub file: String,
    pub imei: String,
    /// Reset by ATZ over the open link; otherwise the user presses RESET.
    pub send_reset: bool,
}

/// Checks the dialog's inputs and builds the job.
pub fn build_job(image: Option<&[u8]>, file: &str, imei: &str, send_reset: bool, connected: bool, pin: &str) -> Result<Job, String> {
    let imei = imei.trim();
    let Some(image) = image else { return Err("choose a firmware image first".into()) };
    if imei.is_empty() {
        return Err("enter the node's IMEI — it is on the label and in AT+CFG".into());
    }
    if send_reset && !connected {
        return Err("not connected — connect first, or reset the node by its key".into());
    }
    let fw = otanb::load_firmware(image).map_err(|e| e.to_string())?;
    let pw = password(pin).map_err(|e| e.to_string())?;
    let plan = otanb::plan_upgrade(fw, pw, 0).map_err(|e| e.to_string())?;
    Ok(Job { plan, file: file.to_string(), imei: imei.to_string(), send_reset })
}

/// What an update tells the window as it goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Report {
    /// A line for the terminal.
    Log(String),
    /// What it is doing now, for the progress dialog.
    Phase(String),
    Progress {
        sent: usize,
        total: usize,
    },
    /// The console link is down; the window shows the disconnected state with this status.
    Disconnected(String),
    /// Connected to the bootloader: Disconnect works as an emergency stop, the console stays closed (a stray AT line in
    /// the middle of a flash frame would corrupt it).
    Bootloader(String),
    /// Scanning for the bootloader started or stopped.
    Scanning(bool),
}

/// The link as an [`otanb::Transport`]: each request is sent raw and answered by the next `AT+DATA` line.
pub struct LinkTransport<'a> {
    pub link: &'a dyn Link,
    pub tap: Tap,
}

impl otanb::Transport for LinkTransport<'_> {
    fn roundtrip(&mut self, line: &str, timeout: Duration) -> Result<String, String> {
        // Lines from before the send are stale: a banner must not pass for this command's reply.
        self.tap.drain();
        self.link.send_raw(line)?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.tap.recv(left) {
                Ok(l) if l.contains("AT+DATA=") => return Ok(l),
                Ok(_) => {}
                Err(NoLine::Timeout) => return Err("timeout waiting for AT+DATA reply".into()),
                Err(NoLine::Closed) => return Err("link dropped during OTA".into()),
            }
        }
    }
}

/// Runs an update to its end. `stop`, set by Cancel (which also drops the link), ends the bootloader wait and makes the
/// next round-trip fail; the error is then just "cancelled".
pub fn run(link: &dyn Link, job: &Job, timing: Timing, stop: &AtomicBool, report: &mut dyn FnMut(Report)) -> Result<(), String> {
    let result = run_inner(link, job, timing, stop, report);
    match result {
        Err(_) if stop.load(Ordering::Relaxed) => Err("cancelled".into()),
        other => other,
    }
}

fn run_inner(link: &dyn Link, job: &Job, timing: Timing, stop: &AtomicBool, report: &mut dyn FnMut(Report)) -> Result<(), String> {
    report(Report::Log(format!("-- firmware update: {} ({} bytes) -> node {} --", job.file, job.plan.firmware.image.len(), job.imei)));
    let since = Instant::now();
    reset(link, job, timing, report)?;

    report(Report::Phase(format!("waiting for the bootloader to advertise as {}…", job.imei)));
    report(Report::Scanning(true));
    link.scan();
    let found =
        link::await_device(|| link.devices(), |d| matches_imei(&d.name, &job.imei), since, timing.bootloader_window, timing.poll, stop);
    link.stop_scan();
    report(Report::Scanning(false));
    let Some(dev) = found else {
        return Err(format!(
            "bootloader {} not seen within {}s — reset the node again (a node reset less than two minutes ago may not re-enter the bootloader)",
            job.imei,
            timing.bootloader_window.as_secs()
        ));
    };

    report(Report::Log(format!("-- bootloader found: {} ({}) — connecting --", dev.name, dev.addr)));
    report(Report::Phase(format!("connecting to {}…", dev.name)));
    if link.connect(&dev.addr).is_none() {
        return Err("connect to bootloader failed (see terminal)".into());
    }
    report(Report::Bootloader(dev.name.clone()));

    let total = job.plan.firmware.image.len();
    report(Report::Phase(format!("erasing and flashing {total} bytes…")));
    let mut transport = LinkTransport { link, tap: link.lines() };
    let result = otanb::upgrade(&mut transport, &job.plan, &mut |sent, total| {
        report(Report::Progress { sent, total });
        report(Report::Phase(if sent >= total { "verifying and rebooting…".into() } else { format!("flashing {sent} / {total} bytes") }));
    });
    drop(transport);
    // REBOOT's reply is the bootloader's last word; the node drops the link itself right after, or this side does.
    link.disconnect();
    report(Report::Disconnected("firmware update finished".into()));
    result.map_err(|e| e.to_string())
}

/// Takes the node through a reset so it enters its bootloader.
fn reset(link: &dyn Link, job: &Job, timing: Timing, report: &mut dyn FnMut(Report)) -> Result<(), String> {
    if !job.send_reset {
        // A scan and a live link are kept apart everywhere else; the reset would kill the link anyway.
        if link.disconnect() {
            report(Report::Log("-- disconnected before the manual reset --".into()));
            report(Report::Disconnected("firmware: press RESET on the node".into()));
        }
        report(Report::Log("-- press the RESET key on the node now (or hold ACT for 3 s) --".into()));
        report(Report::Phase("press the RESET key on the node now".into()));
        return Ok(());
    }
    if !link.is_connected() {
        return Err("link is gone — connect again, or reset the node by its key".into());
    }
    report(Report::Log(format!("-- sending {RESET_CMD} to reset the node into its bootloader --")));
    report(Report::Phase(format!("resetting the node with {RESET_CMD}…")));
    link.send(RESET_CMD);
    let deadline = Instant::now() + timing.reset_drop;
    while link.is_connected() && Instant::now() < deadline {
        std::thread::sleep(timing.poll.min(Duration::from_millis(100)));
    }
    if link.disconnect() {
        report(Report::Log(format!("-- link still up after {RESET_CMD}; closed it to scan for the bootloader --")));
        report(Report::Disconnected("firmware: scanning for bootloader".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::fake::FakeLink;

    #[test]
    fn imei_helpers() {
        assert!(matches_imei("860601234567890", "860601234567890"));
        assert!(matches_imei("NB-860601234567890", " 860601234567890 "));
        assert!(!matches_imei("x1234", "1234"), "a short IMEI must match exactly");
        assert!(!matches_imei("anything", ""));
        assert!(looks_like_imei("860601234567890"));
        assert!(!looks_like_imei("86060123456789"));
        assert!(!looks_like_imei("86060123456789a"));
        assert_eq!(extract_imei("IMEI: 860601234567890").as_deref(), Some("860601234567890"));
        assert_eq!(extract_imei("imei=860601234567890 x").as_deref(), Some("860601234567890"));
        assert_eq!(extract_imei("IMSI: 860601234567890"), None);
        assert_eq!(extract_imei("IMEI: ٨٦٠٦٠١٢٣٤٥٦٧٨٩٠"), None, "only ASCII digits");
    }

    #[test]
    fn the_password_comes_from_the_pin() {
        assert_eq!(password("  ").unwrap(), otanb::default_password());
        assert_eq!(&password("349011").unwrap(), b"349011OO");
        assert!(password("123456789").is_err());
    }

    #[test]
    fn a_job_is_checked() {
        let img = [1u8; 16];
        assert!(build_job(None, "f.bin", "8606", false, false, "").unwrap_err().contains("choose"));
        assert!(build_job(Some(&img), "f.bin", " ", false, false, "").unwrap_err().contains("IMEI"));
        assert!(build_job(Some(&img), "f.bin", "8606", true, false, "").unwrap_err().contains("not connected"));
        assert!(build_job(Some(&img), "f.bin", "8606", false, false, "123456789").is_err());
        let job = build_job(Some(&img), "f.bin", " 8606 ", true, true, "").unwrap();
        assert_eq!((job.imei.as_str(), job.send_reset), ("8606", true));
    }

    const IMEI: &str = "860601234567890";

    fn fast() -> Timing {
        Timing { bootloader_window: Duration::from_secs(2), reset_drop: Duration::from_millis(300), poll: Duration::from_millis(5) }
    }

    #[test]
    fn an_update_resets_finds_the_bootloader_and_flashes() {
        let link = FakeLink::nb_node("AA", "D20-NB", IMEI);
        link.connect("AA");
        let image: Vec<u8> = (0..1000).map(|i| (i % 251) as u8).collect();
        let job = build_job(Some(&image), "fw.bin", IMEI, true, true, "").unwrap();
        let mut reports = Vec::new();
        run(&link, &job, fast(), &AtomicBool::new(false), &mut |r| reports.push(r)).unwrap();
        let flashed = link.bootloader_image();
        assert_eq!(flashed, job.plan.firmware.image);
        assert!(reports.contains(&Report::Bootloader(IMEI.into())));
        let n = job.plan.firmware.image.len();
        assert!(reports.contains(&Report::Progress { sent: n, total: n }));
        assert!(link.sent().iter().any(|s| s == "ATZ"));
        assert!(!link.is_connected());
    }

    #[test]
    fn without_a_bootloader_it_gives_up_with_a_reason() {
        let link = FakeLink::console("AA", "D20-NB");
        let job = build_job(Some(&[1u8; 8]), "fw.bin", IMEI, false, false, "").unwrap();
        let e = run(&link, &job, Timing { bootloader_window: Duration::from_millis(50), ..fast() }, &AtomicBool::new(false), &mut |_| {})
            .unwrap_err();
        assert!(e.contains("not seen"), "{e}");
    }

    #[test]
    fn a_cancelled_update_says_cancelled() {
        let link = FakeLink::console("AA", "D20-NB");
        let job = build_job(Some(&[1u8; 8]), "fw.bin", IMEI, false, false, "").unwrap();
        let e = run(&link, &job, fast(), &AtomicBool::new(true), &mut |_| {}).unwrap_err();
        assert_eq!(e, "cancelled");
    }
}
