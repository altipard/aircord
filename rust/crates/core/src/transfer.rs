//! Transferring settings: what the person wants against what `AT+CFG` reported, only the differences sent as
//! `AT+<KEY>=<value>`, each reply checked for `OK` or an error, then `AT+CFG` read again and every change reported as
//! verified, different or failed. A restart is reported as needed when a changed setting takes effect only after one;
//! it is never sent from here, the window offers it.
//!
//! Like a playbook run, [`apply`] waits on the link: call it from a job's thread.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::cfg::Cfg;
use crate::device::Template;
use crate::lines::{NoLine, Tap};
use crate::link::Link;
use crate::redact;
use crate::settings::{Field, Risk};

/// The command that prints every setting.
pub const READ_CMD: &str = "AT+CFG";

/// How long the whole `AT+CFG` dump may take, in reply waits: it is some forty lines over BLE, and each line restarts
/// the wait, so this only caps a device that never stops talking.
const DUMP_WAITS: u32 = 4;

/// One setting to send.
#[derive(Clone, PartialEq, Eq)]
pub struct Change {
    pub key: String,
    pub label: String,
    /// The value `AT+CFG` reported (empty for `NULL`).
    pub from: String,
    /// The checked value that is sent.
    pub to: String,
    /// Never shown or logged in clear.
    pub secret: bool,
    pub risk: Risk,
    pub needs_reboot: bool,
    /// Compared by number in the read-back, so `0900` counts as `900`.
    numeric: bool,
}

impl Change {
    /// The command the device gets.
    pub fn cmd(&self) -> String {
        format!("AT+{}={}", self.key, self.to)
    }

    /// The command as it may be shown or logged.
    pub fn shown_cmd(&self) -> String {
        if self.secret { format!("AT+{}={}", self.key, redact::MASK) } else { self.cmd() }
    }

    /// Whether the window must ask before sending it.
    pub fn needs_confirm(&self) -> bool {
        self.risk.needs_confirm()
    }

    fn holds(&self, actual: &str) -> bool {
        if self.numeric
            && let (Ok(a), Ok(b)) = (actual.trim().parse::<i64>(), self.to.trim().parse::<i64>())
        {
            return a == b;
        }
        actual.trim() == self.to.trim()
    }
}

/// Without the values of a secret.
impl fmt::Debug for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (from, to) = if self.secret { (redact::MASK, redact::MASK) } else { (self.from.as_str(), self.to.as_str()) };
        f.debug_struct("Change")
            .field("key", &self.key)
            .field("from", &from)
            .field("to", &to)
            .field("risk", &self.risk)
            .field("needs_reboot", &self.needs_reboot)
            .finish_non_exhaustive()
    }
}

/// Why a wanted value can't be sent, in words for the person in the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    pub key: String,
    pub label: String,
    pub message: String,
}

/// The changes that turn `current` into `wanted` (key → value as entered), in template order. A value equal to the
/// current one is no change and is not checked, so a device value outside the template's rules never blocks the
/// others. A setting hidden by its `visible_if` (with the wanted values over the current ones) is left out. A key the
/// template lacks, or that the device did not report, is an error, as is any value [`crate::settings::Setting::check`]
/// refuses; every error is returned, not just the first.
pub fn plan(tpl: &Template, current: &Cfg, wanted: &BTreeMap<String, String>) -> Result<Vec<Change>, Vec<FieldError>> {
    let mut errors = Vec::new();
    for key in wanted.keys().filter(|k| tpl.setting(k).is_none()) {
        errors.push(FieldError { key: key.clone(), label: key.clone(), message: "Diese Einstellung kennt die Vorlage nicht.".into() });
    }
    let value_of = |k: &str| wanted.get(k).map(String::as_str).or_else(|| current.get(k));
    let mut changes = Vec::new();
    for s in &tpl.settings {
        let Some(want) = wanted.get(&s.key) else { continue };
        let error = |message: String| FieldError { key: s.key.clone(), label: s.label.clone(), message };
        if !s.visible(value_of) {
            continue;
        }
        let Some(have) = current.get(&s.key) else {
            errors.push(error("Das Gerät meldet diese Einstellung nicht. Sie kann hier nicht geändert werden.".into()));
            continue;
        };
        if s.same(have, want) {
            continue;
        }
        match s.check(want) {
            Ok(to) => changes.push(Change {
                key: s.key.clone(),
                label: s.label.clone(),
                from: have.to_string(),
                to,
                secret: s.is_secret(),
                risk: s.risk,
                needs_reboot: s.needs_reboot,
                numeric: matches!(s.field, Field::Duration { .. } | Field::Number { .. }),
            }),
            Err(message) => errors.push(error(message)),
        }
    }
    if errors.is_empty() { Ok(changes) } else { Err(errors) }
}

/// The changes the window must ask about before [`apply`]: those that can cost the network or can't be undone.
pub fn needing_confirmation(changes: &[Change]) -> Vec<&Change> {
    changes.iter().filter(|c| c.needs_confirm()).collect()
}

/// How a change ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// The device took it and `AT+CFG` shows it.
    Verified,
    /// The device answered `OK` but `AT+CFG` shows another value (masked for a secret).
    Differs { actual: String },
    /// Refused, unanswered or not sent.
    Failed { why: String },
    /// Taken, but the read-back could not confirm it.
    Unverified { why: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldResult {
    pub key: String,
    pub label: String,
    pub status: Status,
}

/// What a transfer did.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// One per change, in the order sent.
    pub fields: Vec<FieldResult>,
    /// A change that only works after a restart was taken: offer the template's restart.
    pub reboot_needed: bool,
    /// The settings after the transfer, when they could be read.
    pub readback: Option<Cfg>,
}

impl Outcome {
    pub fn all_verified(&self) -> bool {
        self.fields.iter().all(|f| f.status == Status::Verified)
    }
}

/// What a running transfer tells the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    /// Change `n` of `total` is being sent.
    Sending { n: usize, total: usize, key: String, label: String },
    /// `AT+CFG` is being read to check the result.
    Verifying,
}

/// How a command's reply ended.
#[derive(Debug, PartialEq, Eq)]
enum End {
    Ok,
    Error(String),
    Timeout,
    Closed,
    Stopped,
}

/// Whether a reply line is one of the console's errors: `AT_ERROR`, `AT_PARAM_ERROR`, `AT_TEST_PARAM_OVERFLOW`,
/// `AT_RX_ERROR`, a bare `ERROR`.
pub fn is_error_reply(line: &str) -> bool {
    let l = line.trim().to_ascii_uppercase();
    l == "ERROR" || (l.starts_with("AT_") && (l.ends_with("ERROR") || l.ends_with("OVERFLOW")))
}

/// Reads reply lines up to the closing `OK` (any case: some firmware says `ok`) or an error line. `idle` is the
/// longest silence, `total` the longest overall. Lines in between (a dump, `Signal Strength:99`) are returned.
fn collect(tap: &Tap, idle: Duration, total: Duration, stop: &AtomicBool) -> (Vec<String>, End) {
    let start = Instant::now();
    let mut last = start;
    let mut lines = Vec::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return (lines, End::Stopped);
        }
        let left = (last + idle).min(start + total).saturating_duration_since(Instant::now());
        if left.is_zero() {
            return (lines, End::Timeout);
        }
        match tap.recv(left.min(Duration::from_millis(250))) {
            Ok(line) => {
                last = Instant::now();
                if line.trim().eq_ignore_ascii_case("OK") {
                    return (lines, End::Ok);
                }
                if is_error_reply(&line) {
                    return (lines, End::Error(line.trim().to_string()));
                }
                lines.push(line);
            }
            Err(NoLine::Timeout) => {}
            Err(NoLine::Closed) => return (lines, End::Closed),
        }
    }
}

fn read_on(tap: &Tap, link: &dyn Link, wait: Duration, stop: &AtomicBool) -> Result<Cfg, String> {
    if !link.is_connected() {
        return Err("Keine Verbindung zum Gerät.".into());
    }
    tap.drain();
    link.send(READ_CMD);
    let (lines, end) = collect(tap, wait, wait * DUMP_WAITS, stop);
    match end {
        End::Ok => Ok(Cfg::parse(lines.iter().map(String::as_str).chain(["OK"]))),
        End::Error(e) => Err(format!("Das Gerät hat {READ_CMD} abgelehnt ({e}).")),
        End::Timeout => Err(format!("Keine vollständige Antwort auf {READ_CMD}.")),
        End::Closed => Err("Verbindung verloren.".into()),
        End::Stopped => Err("Abgebrochen.".into()),
    }
}

/// Sends `AT+CFG` and reads the dump. Waits: call it from a job's thread.
pub fn read_cfg(link: &dyn Link, wait: Duration, stop: &AtomicBool) -> Result<Cfg, String> {
    read_on(&link.lines(), link, wait, stop)
}

/// Sends each change and checks its reply, then reads `AT+CFG` again and reports every change. A refused or
/// unanswered change does not stop the others, they are independent settings; a lost link or `stop` does, and the
/// rest are reported as not sent. Secrets go out through [`Link::send_secret`], so their echo is masked, and no
/// result carries their value. `ATZ` is never sent.
pub fn apply(link: &dyn Link, changes: &[Change], wait: Duration, stop: &AtomicBool, report: &mut dyn FnMut(Progress)) -> Outcome {
    let tap = link.lines();
    let total = changes.len();
    let mut sent: Vec<Result<(), String>> = Vec::with_capacity(total);
    let mut halted: Option<String> = None;
    for (i, c) in changes.iter().enumerate() {
        if halted.is_none() {
            if stop.load(Ordering::Relaxed) {
                halted = Some("Abgebrochen, nicht gesendet.".into());
            } else if !link.is_connected() {
                halted = Some("Keine Verbindung, nicht gesendet.".into());
            }
        }
        if let Some(why) = &halted {
            sent.push(Err(why.clone()));
            continue;
        }
        report(Progress::Sending { n: i + 1, total, key: c.key.clone(), label: c.label.clone() });
        tap.drain();
        if c.secret {
            link.send_secret(&c.cmd());
        } else {
            link.send(&c.cmd());
        }
        let (_, end) = collect(&tap, wait, wait, stop);
        sent.push(match end {
            End::Ok => Ok(()),
            End::Error(e) => Err(format!("Das Gerät hat den Wert abgelehnt ({e}).")),
            End::Timeout => Err("Keine Antwort vom Gerät.".into()),
            End::Closed => {
                halted = Some("Verbindung verloren, nicht gesendet.".into());
                Err("Verbindung verloren.".into())
            }
            End::Stopped => {
                halted = Some("Abgebrochen, nicht gesendet.".into());
                Err("Abgebrochen.".into())
            }
        });
    }
    let readback = if sent.iter().any(Result::is_ok) {
        report(Progress::Verifying);
        Some(read_on(&tap, link, wait, stop))
    } else {
        None
    };
    let fields: Vec<FieldResult> = changes
        .iter()
        .zip(sent)
        .map(|(c, result)| {
            let status = match (result, &readback) {
                (Err(why), _) => Status::Failed { why },
                (Ok(()), Some(Ok(cfg))) => match cfg.get(&c.key) {
                    None => Status::Unverified { why: "Das Gerät meldet diese Einstellung nicht.".into() },
                    Some(v) if c.holds(v) => Status::Verified,
                    Some(v) => Status::Differs { actual: if c.secret { redact::MASK.into() } else { v.to_string() } },
                },
                (Ok(()), Some(Err(why))) => Status::Unverified { why: why.clone() },
                (Ok(()), None) => Status::Unverified { why: "Nicht erneut gelesen.".into() },
            };
            FieldResult { key: c.key.clone(), label: c.label.clone(), status }
        })
        .collect();
    let reboot_needed =
        changes.iter().zip(&fields).any(|(c, f)| c.needs_reboot && matches!(f.status, Status::Verified | Status::Unverified { .. }));
    Outcome { fields, reboot_needed, readback: readback.and_then(Result::ok) }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::device::{Templates, parse};
    use crate::link::Events;
    use crate::link::fake::FakeLink;

    const S31: &str = include_str!("../testdata/s31-nb-v1.4.0-cfg.txt");
    const WAIT: Duration = Duration::from_millis(200);

    fn s31_template() -> Template {
        Templates::load_from(Path::new("/nonexistent")).get("Dragino S31-NB").unwrap().clone()
    }

    fn want(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn device() -> FakeLink {
        let link = FakeLink::configured("A", "866207050000001", S31);
        link.connect("A");
        link
    }

    #[derive(Default)]
    struct Log(Mutex<Vec<String>>);

    impl Events for Log {
        fn log(&self, line: &str) {
            self.0.lock().unwrap().push(line.to_string());
        }
        fn link_dropped(&self) {}
        fn scan_failed(&self, _: &str) {}
    }

    fn run(link: &FakeLink, changes: &[Change]) -> (Outcome, Vec<Progress>) {
        let mut progress = Vec::new();
        let out = apply(link, changes, WAIT, &AtomicBool::new(false), &mut |p| progress.push(p));
        (out, progress)
    }

    #[test]
    fn only_what_differs_is_planned_in_template_order() {
        let tpl = s31_template();
        let cfg = Cfg::parse_text(S31);
        let wanted = want(&[
            ("TDC", "1800"),
            ("APN", "iot.1nce.net"),
            ("PRO", "3,5"),
            ("UNAME", "sensor-7"),
            ("SHTEMP", "20,30"),
            ("CSQTIME", "05"),
        ]);
        let changes = plan(&tpl, &cfg, &wanted).unwrap();
        assert_eq!(
            changes.iter().map(Change::cmd).collect::<Vec<_>>(),
            ["AT+PRO=3,5", "AT+UNAME=sensor-7", "AT+TDC=1800", "AT+SHTEMP=20,30"]
        );
        assert_eq!(changes[2].from, "900");
        assert_eq!(changes[1].from, "", "NULL is empty");
        let ask: Vec<&str> = needing_confirmation(&changes).iter().map(|c| c.key.as_str()).collect();
        assert_eq!(ask, ["PRO", "UNAME"], "network risk asks first");
    }

    #[test]
    fn hidden_settings_are_left_out() {
        let tpl = s31_template();
        let cfg = Cfg::parse_text(S31);
        let changes = plan(&tpl, &cfg, &want(&[("UNAME", "sensor-7"), ("TDC", "1800")])).unwrap();
        assert_eq!(changes.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["TDC"], "MQTT fields only with PRO=3,*");
    }

    #[test]
    fn wrong_input_unknown_and_unreported_keys_are_errors_in_german() {
        let tpl = s31_template();
        let cfg = Cfg::parse_text(S31);
        let errors = plan(&tpl, &cfg, &want(&[("TDC", "eine Stunde"), ("SERVADDR", "server-ohne-port"), ("XYZ", "1")])).unwrap_err();
        let by_key: BTreeMap<&str, &str> = errors.iter().map(|e| (e.key.as_str(), e.message.as_str())).collect();
        assert_eq!(by_key["TDC"], "Bitte eine ganze Zahl eingeben.");
        assert_eq!(
            by_key["SERVADDR"],
            "Die Eingabe hat nicht das erwartete Format. Adresse und Port mit Komma, z. B. udp.os.1nce.com,4445."
        );
        assert_eq!(by_key["XYZ"], "Diese Einstellung kennt die Vorlage nicht.");
        let port = plan(&tpl, &cfg, &want(&[("SERVADDR", "udp.os.1nce.com,70000")])).unwrap_err();
        assert_eq!(port.len(), 1, "a port above 65535 is refused");

        let d20 = Templates::load_from(Path::new("/nonexistent")).get("Dragino D20-NB").unwrap().clone();
        let without_tr = Cfg::parse_text("AT+MODEL=D20-NB,v1.3.4\nAT+TDC=7200\nOK");
        let e = plan(&d20, &without_tr, &want(&[("TR", "900")])).unwrap_err();
        assert_eq!(e[0].message, "Das Gerät meldet diese Einstellung nicht. Sie kann hier nicht geändert werden.");
    }

    #[test]
    fn a_device_value_outside_the_rules_does_not_block_other_changes() {
        let tpl = parse(
            "name = \"x\"\n[[setting]]\nkey = \"TR\"\nlabel = \"t\"\ngroup = \"g\"\ntype = \"duration\"\nunit = \"s\"\nmin = 180\n\
                         [[setting]]\nkey = \"TDC\"\nlabel = \"d\"\ngroup = \"g\"\ntype = \"duration\"\nunit = \"s\"",
        )
        .unwrap();
        let cfg = Cfg::parse_text("AT+TR=60\nAT+TDC=900\nOK");
        let changes = plan(&tpl, &cfg, &want(&[("TR", "60"), ("TDC", "1200")])).unwrap();
        assert_eq!(changes.iter().map(Change::cmd).collect::<Vec<_>>(), ["AT+TDC=1200"]);
    }

    #[test]
    fn a_transfer_sends_the_diff_and_verifies_it() {
        let link = device();
        let tpl = s31_template();
        let changes = plan(&tpl, &Cfg::parse_text(S31), &want(&[("PRO", "3,5"), ("UNAME", "sensor-7"), ("TDC", "1800")])).unwrap();
        let (out, progress) = run(&link, &changes);
        assert_eq!(link.sent(), ["AT+PRO=3,5", "AT+UNAME=sensor-7", "AT+TDC=1800", "AT+CFG"]);
        assert!(out.all_verified(), "{:?}", out.fields);
        assert!(out.reboot_needed, "PRO and UNAME need a restart");
        assert!(!link.sent().iter().any(|c| c.eq_ignore_ascii_case("ATZ")), "the restart is only offered");
        assert_eq!(out.readback.as_ref().and_then(|c| c.get("TDC")), Some("1800"));
        assert_eq!(progress.len(), 4);
        assert_eq!(progress[0], Progress::Sending { n: 1, total: 3, key: "PRO".into(), label: "Übertragungsart und Datenformat".into() });
        assert_eq!(progress[3], Progress::Verifying);
        assert_eq!(tpl.reboot_action().map(|a| a.cmd.as_str()), Some("ATZ"));
    }

    #[test]
    fn a_change_without_restart_needs_none() {
        let link = device();
        let changes = plan(&s31_template(), &Cfg::parse_text(S31), &want(&[("TDC", "3600")])).unwrap();
        let (out, _) = run(&link, &changes);
        assert!(out.all_verified() && !out.reboot_needed);
    }

    #[test]
    fn a_refused_value_fails_alone_and_the_rest_goes_on() {
        let link = device();
        link.reject("TDC", "AT_PARAM_ERROR");
        let changes = plan(&s31_template(), &Cfg::parse_text(S31), &want(&[("TDC", "1800"), ("SHTEMP", "20,30")])).unwrap();
        let (out, _) = run(&link, &changes);
        assert_eq!(out.fields[0].status, Status::Failed { why: "Das Gerät hat den Wert abgelehnt (AT_PARAM_ERROR).".into() });
        assert_eq!(out.fields[1].status, Status::Verified);
        assert_eq!(link.sent(), ["AT+TDC=1800", "AT+SHTEMP=20,30", "AT+CFG"]);
        assert_eq!(link.cfg_value("TDC").as_deref(), Some("900"));
    }

    #[test]
    fn at_error_and_silence_are_failures() {
        let link = device();
        link.reject("TDC", "AT_ERROR");
        link.reply("AT+SHTEMP=20,30", &[]);
        let changes = plan(&s31_template(), &Cfg::parse_text(S31), &want(&[("TDC", "1800"), ("SHTEMP", "20,30")])).unwrap();
        let (out, _) = run(&link, &changes);
        assert_eq!(out.fields[0].status, Status::Failed { why: "Das Gerät hat den Wert abgelehnt (AT_ERROR).".into() });
        assert_eq!(out.fields[1].status, Status::Failed { why: "Keine Antwort vom Gerät.".into() });
        assert!(!out.reboot_needed);
        assert!(out.readback.is_none(), "nothing was taken, nothing to read back");
    }

    #[test]
    fn a_value_the_device_drops_is_reported_as_different() {
        let link = device();
        link.ignore_set("SHTEMP");
        let changes = plan(&s31_template(), &Cfg::parse_text(S31), &want(&[("SHTEMP", "20,30")])).unwrap();
        let (out, _) = run(&link, &changes);
        assert_eq!(out.fields[0].status, Status::Differs { actual: "0,0".into() });
        assert!(!out.all_verified());
    }

    #[test]
    fn a_read_back_that_fails_or_lacks_the_key_leaves_it_unverified() {
        let link = device();
        link.reply("AT+CFG", &["AT+MODEL=S31-NB,v1.4.0", "ok"]);
        let changes = plan(&s31_template(), &Cfg::parse_text(S31), &want(&[("PRO", "3,5")])).unwrap();
        let (out, _) = run(&link, &changes);
        assert_eq!(out.fields[0].status, Status::Unverified { why: "Das Gerät meldet diese Einstellung nicht.".into() });
        assert!(out.reboot_needed, "it was taken, so the restart is still due");

        let cut = device();
        cut.reply("AT+CFG", &["AT+MODEL=S31-NB,v1.4.0", "AT+PRO=3,5"]);
        let out = apply(&cut, &changes, Duration::from_millis(40), &AtomicBool::new(false), &mut |_| {});
        assert_eq!(out.fields[0].status, Status::Unverified { why: "Keine vollständige Antwort auf AT+CFG.".into() });
        assert!(out.readback.is_none());
    }

    #[test]
    fn stop_and_a_missing_link_send_nothing() {
        let changes = plan(&s31_template(), &Cfg::parse_text(S31), &want(&[("TDC", "1800")])).unwrap();
        let link = device();
        let out = apply(&link, &changes, WAIT, &AtomicBool::new(true), &mut |_| {});
        assert_eq!(out.fields[0].status, Status::Failed { why: "Abgebrochen, nicht gesendet.".into() });
        assert!(link.sent().is_empty());
        let offline = FakeLink::configured("A", "x", S31);
        let out = apply(&offline, &changes, WAIT, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(out.fields[0].status, Status::Failed { why: "Keine Verbindung, nicht gesendet.".into() });
    }

    #[test]
    fn passwords_never_leave_in_clear() {
        let link = device();
        let log = Arc::new(Log::default());
        link.set_events(log.clone());
        let tpl = s31_template();
        let cfg = Cfg::parse_text(S31);
        let changes = plan(&tpl, &cfg, &want(&[("PRO", "3,5"), ("PWD", "mqtt-geheim"), ("PWORD", "neupwx")])).unwrap();
        assert!(changes.iter().filter(|c| c.key != "PRO").all(|c| c.secret));
        let shown = format!("{changes:?} {}", changes.iter().map(Change::shown_cmd).collect::<Vec<_>>().join(" "));
        assert!(!shown.contains("mqtt-geheim") && !shown.contains("neupwx") && !shown.contains("kxqmzp"), "{shown}");
        assert!(shown.contains("AT+PWD=******") && shown.contains("AT+PWORD=******"));
        assert_eq!(needing_confirmation(&changes).len(), 3, "PWORD is irreversible, PWD and PRO cost the network");

        link.ignore_set("PWORD");
        let (out, _) = run(&link, &changes);
        assert_eq!(out.fields[1].status, Status::Verified);
        assert_eq!(out.fields[2].status, Status::Differs { actual: "******".into() }, "the old password is not shown either");
        let sent = link.sent().join("\n");
        assert!(!sent.contains("mqtt-geheim") && !sent.contains("neupwx"), "{sent}");
        let outgoing: Vec<String> = log.0.lock().unwrap().iter().filter(|l| l.starts_with(">> ")).cloned().collect();
        assert!(outgoing.iter().all(|l| !l.contains("mqtt-geheim") && !l.contains("neupwx")), "{outgoing:?}");
        let results = format!("{:?}", out);
        assert!(!results.contains("mqtt-geheim") && !results.contains("neupwx") && !results.contains("kxqmzp"), "{results}");
        // The link logs the AT+CFG read-back as it comes; masking those lines is the logging side's job.
        let incoming: Vec<String> =
            log.0.lock().unwrap().iter().filter(|l| l.starts_with("<< ")).map(|l| tpl.redact(l).into_owned()).collect();
        assert!(incoming.iter().any(|l| l == "<< AT+PWD=******"), "{incoming:?}");
        assert!(incoming.iter().all(|l| !l.contains("mqtt-geheim") && !l.contains("kxqmzp")));
    }

    #[test]
    fn read_cfg_reads_the_whole_dump_past_noise() {
        let link = device();
        let cfg = read_cfg(&link, WAIT, &AtomicBool::new(false)).unwrap();
        assert_eq!(cfg, Cfg::parse_text(S31));
        let noisy = FakeLink::console("B", "x");
        noisy.connect("B");
        noisy.reply("AT+CFG", &["[28891]Signal Strength:99", "AT+MODEL=S31-NB,v1.4.0", "", "AT+TDC=900", "OK"]);
        let cfg = read_cfg(&noisy, WAIT, &AtomicBool::new(false)).unwrap();
        assert_eq!((cfg.model.as_deref(), cfg.get("TDC")), (Some("S31-NB"), Some("900")));
        noisy.reply("AT+CFG", &["AT_ERROR"]);
        assert_eq!(read_cfg(&noisy, WAIT, &AtomicBool::new(false)).unwrap_err(), "Das Gerät hat AT+CFG abgelehnt (AT_ERROR).");
        assert!(is_error_reply("AT_PARAM_ERROR") && is_error_reply("at_test_param_overflow") && is_error_reply("ERROR"));
        assert!(!is_error_reply("OK") && !is_error_reply("AT+TDC=1"));
    }
}
