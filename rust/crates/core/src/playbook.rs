//! Playbooks: named sequences of AT commands, replayed step by step, each optionally waiting for a matching reply and
//! pausing afterwards. Stored as JSON, one file each, in the format Export writes and Import reads.
//!
//! The editor's text format is one command per line. A ` ?<pattern>` suffix waits until a reply matches the pattern (a
//! bare ` ?` waits for `OK`); an `@<ms>` suffix pauses after the step; lines starting with `#` are comments:
//!
//! ```text
//! AT+CFG
//! AT+APN=iot.1nce.net ?OK
//! AT+TDC=3600 ? @1500
//! ```

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config;
use crate::lines::{NoLine, Tap};
use crate::link::Link;
use crate::session::{named_files, remove};
use crate::slug::slug;

/// How long a step's reply wait lasts. Consoles answer set commands within a second or two when idle; 15 s covers busy
/// phases (boot, a TX window) without hanging a playbook forever.
pub const WAIT_TIMEOUT: Duration = Duration::from_secs(15);

/// The most of a chosen file Import reads: a real playbook is a few hundred bytes.
pub const IMPORT_LIMIT: u64 = 1 << 20;

/// The syntax help shown above the editor for as long as it is open.
pub const SYNTAX: &str = "one command per line\n\
?PATTERN   wait for a matching reply   (bare ? = OK)\n\
@MS        pause afterwards\n\
#          comment";

/// One command, the reply pattern to wait for after it (empty: no wait), and the pause after that.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub cmd: String,
    #[serde(rename = "waitFor", default, skip_serializing_if = "String::is_empty")]
    pub wait_for: String,
    #[serde(rename = "waitMs", default)]
    pub wait_ms: u64,
}

/// A named, ordered sequence of steps.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Playbook {
    pub name: String,
    pub steps: Vec<Step>,
}

/// A file as it is read: lenient about what is missing, and keeping a negative pause so [`decode`] can refuse it.
#[derive(Deserialize)]
struct RawPlaybook {
    #[serde(default)]
    name: String,
    #[serde(default)]
    steps: Vec<RawStep>,
}

#[derive(Deserialize)]
struct RawStep {
    #[serde(default)]
    cmd: String,
    #[serde(rename = "waitFor", default)]
    wait_for: String,
    #[serde(rename = "waitMs", default)]
    wait_ms: i64,
}

impl RawPlaybook {
    fn lenient(self) -> Playbook {
        let steps = self
            .steps
            .into_iter()
            .map(|s| Step { cmd: s.cmd, wait_for: s.wait_for, wait_ms: u64::try_from(s.wait_ms).unwrap_or(0) })
            .collect();
        Playbook { name: self.name, steps }
    }
}

/// The file a playbook is stored under, and the name Export suggests, so an exported file dropped into the store is
/// found again.
pub fn file_name(name: &str) -> String {
    format!("{}.json", slug(name, "playbook"))
}

/// A playbook in the on-disk format, which is also the exchange format.
pub fn encode(pb: &Playbook) -> Vec<u8> {
    serde_json::to_vec_pretty(pb).unwrap_or_default()
}

/// Parses and checks an imported playbook. Unlike [`Playbooks::load`] it refuses anything the editor could not have
/// made (a blank name, no steps, a step without a command, a negative pause): an imported file comes from outside, and
/// a silently half-usable playbook is worse than a clear refusal.
pub fn decode(data: &[u8]) -> Result<Playbook, String> {
    let raw: RawPlaybook = serde_json::from_slice(data).map_err(|e| format!("not a playbook file: {e}"))?;
    let name = raw.name.trim().to_string();
    if name.is_empty() {
        return Err("playbook has no name".into());
    }
    if raw.steps.is_empty() {
        return Err("playbook has no steps".into());
    }
    let mut steps = Vec::with_capacity(raw.steps.len());
    for (i, s) in raw.steps.into_iter().enumerate() {
        let cmd = s.cmd.trim().to_string();
        if cmd.is_empty() {
            return Err(format!("step {} has no command", i + 1));
        }
        let Ok(wait_ms) = u64::try_from(s.wait_ms) else { return Err(format!("step {} has a negative pause", i + 1)) };
        steps.push(Step { cmd, wait_for: s.wait_for, wait_ms });
    }
    Ok(Playbook { name, steps })
}

/// The playbooks in a directory.
#[derive(Clone, Debug)]
pub struct Playbooks {
    dir: PathBuf,
}

impl Playbooks {
    /// The playbooks under `<config>/aircord/playbooks`.
    pub fn open() -> Playbooks {
        Playbooks::at(config::dir("playbooks"))
    }

    pub fn at(dir: impl Into<PathBuf>) -> Playbooks {
        Playbooks { dir: dir.into() }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(file_name(name))
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path(name).is_file()
    }

    /// Writes a playbook, replacing one of the same name.
    pub fn save(&self, pb: &Playbook) -> io::Result<()> {
        if pb.name.trim().is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "playbook name is required"));
        }
        config::atomic_write(&self.path(&pb.name), &encode(pb), 0o644)
    }

    pub fn load(&self, name: &str) -> io::Result<Playbook> {
        let bytes = fs::read(self.path(name))?;
        let raw: RawPlaybook = serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(raw.lenient())
    }

    /// The names of the saved playbooks, sorted.
    pub fn list(&self) -> Vec<String> {
        named_files::<RawPlaybook>(&self.dir, |p| p.name.clone())
    }

    /// Deletes a playbook; one that is not there is not an error.
    pub fn delete(&self, name: &str) -> io::Result<()> {
        remove(&self.path(name))
    }

    /// Saves `pb` only when none of its name exists, so a shipped default never overwrites the user's edits (a deleted
    /// default comes back at the next start).
    pub fn ensure_default(&self, pb: &Playbook) {
        if !self.exists(&pb.name) {
            let _ = self.save(pb);
        }
    }
}

/// The shipped default: the full 1NCE OS provisioning of a Dragino D2x-NB sensor. Every set command waits for its `OK`
/// so a busy console can't swallow steps; the final ATZ reboots into the new settings and answers nothing.
pub fn dragino_1nce() -> Playbook {
    Playbook {
        name: "dragino-nb-1nce".into(),
        steps: parse_steps(
            "AT+QBAND=2,8,20 ?OK\nAT+APN=iot.1nce.net ?OK\nAT+SERVADDR=udp.os.1nce.com,4445 ?OK\nAT+PRO=2,5 ?OK\nAT+TDC=900 ?OK\nAT+CFG ?OK\nATZ",
        ),
    }
}

/// The editor's text as steps; see the module's documentation for the syntax.
pub fn parse_steps(text: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    for line in text.lines() {
        let mut line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut wait_ms = 0;
        if let Some(i) = line.rfind('@')
            && let Ok(ms) = line[i + 1..].trim().parse::<i64>()
            && let Ok(ms) = u64::try_from(ms)
        {
            wait_ms = ms;
            line = line[..i].trim();
        }
        let mut wait_for = String::new();
        if let Some(i) = line.rfind(" ?") {
            wait_for = line[i + 2..].trim().to_string();
            if wait_for.is_empty() {
                wait_for = "OK".into();
            }
            line = line[..i].trim();
        }
        if !line.is_empty() {
            steps.push(Step { cmd: line.to_string(), wait_for, wait_ms });
        }
    }
    steps
}

/// Steps back as editor text, the inverse of [`parse_steps`] for well-formed steps.
pub fn format_steps(steps: &[Step]) -> String {
    let mut out = String::new();
    for s in steps {
        out.push_str(&s.cmd);
        if !s.wait_for.is_empty() {
            out.push_str(" ?");
            out.push_str(&s.wait_for);
        }
        if s.wait_ms > 0 {
            out.push_str(&format!(" @{}", s.wait_ms));
        }
        out.push('\n');
    }
    out
}

/// A step's reply pattern: a regular expression, or, when the text is not a valid one, a literal substring, so a typo
/// can never make a playbook unrunnable.
#[derive(Clone, Debug)]
pub enum Pattern {
    Re(Regex),
    Literal(String),
}

impl Pattern {
    pub fn is_match(&self, line: &str) -> bool {
        match self {
            Pattern::Re(re) => re.is_match(line),
            Pattern::Literal(text) => line.contains(text.as_str()),
        }
    }
}

pub fn compile_wait(pattern: &str) -> Pattern {
    Regex::new(pattern).map_or_else(|_| Pattern::Literal(pattern.to_string()), Pattern::Re)
}

fn risky_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    // `(?-u:\b)`: an ASCII word boundary, as Go's `\b` is.
    RE.get_or_init(|| Regex::new(r"(?i)^AT(Z|\+(FDR|FACNEW|FACTORY|RESET|REBOOT|CFACTORY|QRST))(?-u:\b)").ok()).as_ref()
}

/// Whether `cmd` is a known reboot, reset or factory-wipe command. Running one on a sensor already deployed costs a
/// second site visit. The list is conservative: a command not on it is not thereby safe, only not known to be
/// destructive.
pub fn risky(cmd: &str) -> bool {
    risky_re().is_some_and(|re| re.is_match(cmd.trim()))
}

/// The commands in `steps` that [`risky`] flags, in order and once each, so a confirmation can name what is about to
/// happen.
pub fn risky_steps(steps: &[Step]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    steps.iter().map(|s| s.cmd.trim()).filter(|c| risky(c) && seen.insert(c.to_string())).map(str::to_string).collect()
}

/// Reads reply lines until one matches `re`, the timeout passes, the link drops, or `stop` is set. True only on a match.
pub fn await_match(tap: &Tap, re: &Pattern, timeout: Duration, stop: &AtomicBool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stop.load(Ordering::Relaxed) {
            return false;
        }
        match tap.recv(left.min(Duration::from_millis(250))) {
            Ok(line) if re.is_match(&line) => return true,
            Ok(_) | Err(NoLine::Timeout) => {}
            Err(NoLine::Closed) => return false,
        }
    }
}

/// What a running playbook tells the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    /// Step `n` of `total` is being sent.
    Step { n: usize, total: usize, cmd: String },
}

/// Runs a playbook to its end, or to the first step whose reply doesn't come. Waits: run it on a thread of its own.
/// The error names the step and why.
pub fn run(link: &dyn Link, pb: &Playbook, wait: Duration, stop: &AtomicBool, report: &mut dyn FnMut(Report)) -> Result<(), String> {
    let tap = link.lines();
    let total = pb.steps.len();
    for (i, step) in pb.steps.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return Err(format!("playbook {} stopped", pb.name));
        }
        if !link.is_connected() {
            return Err(format!("playbook {} stopped: link lost", pb.name));
        }
        let cmd = step.cmd.trim();
        if cmd.is_empty() {
            continue;
        }
        // An earlier step's replies must not satisfy this one: the OK closing an AT+CFG dump, say.
        tap.drain();
        report(Report::Step { n: i + 1, total, cmd: cmd.to_string() });
        link.send(cmd);
        if !step.wait_for.is_empty() && !await_match(&tap, &compile_wait(&step.wait_for), wait, stop) {
            let why = if stop.load(Ordering::Relaxed) {
                "stopped".to_string()
            } else if link.is_connected() {
                format!("no reply matching {}", step.wait_for)
            } else {
                "link lost".to_string()
            };
            return Err(format!("playbook {} aborted at step {}/{total} ({cmd}): {why}", pb.name, i + 1));
        }
        if step.wait_ms > 0 {
            sleep_unless(Duration::from_millis(step.wait_ms), stop);
        }
    }
    Ok(())
}

fn sleep_unless(d: Duration, stop: &AtomicBool) {
    let end = Instant::now() + d;
    while !stop.load(Ordering::Relaxed) {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(Duration::from_millis(50)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::fake::FakeLink;

    fn step(cmd: &str, wait_for: &str, wait_ms: u64) -> Step {
        Step { cmd: cmd.into(), wait_for: wait_for.into(), wait_ms }
    }

    #[test]
    fn save_load_round_trip_list_and_delete() {
        let t = tempfile::tempdir().unwrap();
        let s = Playbooks::at(t.path());
        let pb = Playbook { name: "Provision D23".into(), steps: vec![step("AT+CFG", "", 0), step("AT+TDC=3600", "", 1500)] };
        s.save(&pb).unwrap();
        assert_eq!(s.load("Provision D23").unwrap(), pb);
        s.save(&Playbook { name: "Alpha".into(), steps: vec![step("AT", "", 0)] }).unwrap();
        assert_eq!(s.list(), ["Alpha", "Provision D23"]);
        s.delete("Alpha").unwrap();
        s.delete("Alpha").unwrap();
        assert_eq!(s.list(), ["Provision D23"]);
        assert!(s.save(&Playbook { name: " ".into(), steps: vec![] }).is_err());
    }

    #[test]
    fn the_text_format() {
        let text = "\n# provision sequence\nAT+CFG\nAT+TDC=3600 @1500\n   AT+VER\n\n# trailing comment\n";
        assert_eq!(parse_steps(text), [step("AT+CFG", "", 0), step("AT+TDC=3600", "", 1500), step("AT+VER", "", 0)]);
        assert_eq!(parse_steps("AT+SEND=hello@later"), [step("AT+SEND=hello@later", "", 0)]);
        assert_eq!(parse_steps("AT+X @-5"), [step("AT+X @-5", "", 0)], "a negative pause is not a pause");
        let waits = "AT+APN=iot.1nce.net ?OK\nAT+TDC=3600 ? @1500\nAT+QBAND?\nATZ";
        assert_eq!(
            parse_steps(waits),
            [step("AT+APN=iot.1nce.net", "OK", 0), step("AT+TDC=3600", "OK", 1500), step("AT+QBAND?", "", 0), step("ATZ", "", 0)]
        );
    }

    #[test]
    fn format_round_trips() {
        let steps = vec![step("AT+CFG", "", 0), step("AT+TDC=3600", "", 1500), step("AT+APN=x", "OK", 0), step("AT+PRO=2,5", "OK", 500)];
        assert_eq!(parse_steps(&format_steps(&steps)), steps);
    }

    #[test]
    fn a_bad_pattern_matches_literally() {
        assert!(compile_wait("OK").is_match("OK"));
        assert!(compile_wait("(").is_match("ERROR (2)"));
    }

    #[test]
    fn a_default_never_overwrites() {
        let t = tempfile::tempdir().unwrap();
        let s = Playbooks::at(t.path());
        let mine = Playbook { name: "Setup".into(), steps: vec![step("AT+MINE", "", 0)] };
        s.save(&mine).unwrap();
        s.ensure_default(&Playbook { name: "Setup".into(), steps: vec![step("AT+SHIPPED", "", 0)] });
        assert_eq!(s.load("Setup").unwrap(), mine);
        s.ensure_default(&dragino_1nce());
        assert_eq!(s.load("dragino-nb-1nce").unwrap().steps.len(), 7);
    }

    #[test]
    fn encode_decode_round_trip() {
        let pb = Playbook { name: "dragino-nb-1nce".into(), steps: parse_steps("AT+APN=iot.1nce.net ?OK\nAT+TDC=900 ?OK @1500\nATZ") };
        assert_eq!(decode(&encode(&pb)).unwrap(), pb);
    }

    #[test]
    fn decode_refuses_unusable_playbooks() {
        for (what, input) in [
            ("not json", "AT+CFG"),
            ("blank name", r#"{"name":"  ","steps":[{"cmd":"AT"}]}"#),
            ("no steps", r#"{"name":"x","steps":[]}"#),
            ("empty command", r#"{"name":"x","steps":[{"cmd":"AT"},{"cmd":" "}]}"#),
            ("negative pause", r#"{"name":"x","steps":[{"cmd":"AT","waitMs":-1}]}"#),
        ] {
            assert!(decode(input.as_bytes()).is_err(), "{what}");
        }
        assert_eq!(decode(br#"{"name":"x","steps":[{"cmd":"AT"},{"cmd":" "}]}"#).unwrap_err(), "step 2 has no command");
    }

    #[test]
    fn an_import_lands_where_load_finds_it() {
        let t = tempfile::tempdir().unwrap();
        let s = Playbooks::at(t.path());
        let pb = decode(br#"{"name":"Mein Playbook","steps":[{"cmd":"AT+CFG"}]}"#).unwrap();
        assert!(!s.exists(&pb.name));
        s.save(&pb).unwrap();
        assert!(s.exists(&pb.name));
        assert!(s.load(&pb.name).is_ok());
        assert_eq!(file_name(&pb.name), "mein-playbook.json");
    }

    #[test]
    fn it_reads_the_go_build_s_files() {
        let t = tempfile::tempdir().unwrap();
        let json = r#"{
  "name": "go",
  "steps": [
    {"cmd": "AT+CFG", "waitMs": 0},
    {"cmd": "AT+APN=x", "waitFor": "OK", "waitMs": 1500}
  ]
}"#;
        fs::write(t.path().join("go.json"), json).unwrap();
        let pb = Playbooks::at(t.path()).load("go").unwrap();
        assert_eq!(pb.steps, [step("AT+CFG", "", 0), step("AT+APN=x", "OK", 1500)]);
    }

    #[test]
    fn risky_commands() {
        for cmd in ["ATZ", "atz", "  ATZ  ", "AT+FDR", "AT+FACTORY", "AT+RESET", "AT+REBOOT", "AT+QRST"] {
            assert!(risky(cmd), "{cmd}");
        }
        for cmd in ["AT+CFG", "AT+TDC=900", "AT+APN=iot.1nce.net", "AT+QBAND=2,8,20", "AT+VER", "", "ATZED", "AT+FDRINFO"] {
            assert!(!risky(cmd), "{cmd}");
        }
        let steps = parse_steps("AT+CFG\nATZ\nAT+FDR\nATZ");
        assert_eq!(risky_steps(&steps), ["ATZ", "AT+FDR"]);
        assert!(risky_steps(&parse_steps("AT+CFG\nAT+VER")).is_empty());
    }

    #[test]
    fn await_match_sees_a_reply_and_times_out_without_one() {
        let link = FakeLink::console("A", "dev");
        link.connect("A");
        let tap = link.lines();
        link.reply("AT+APN=x", &["Set APN successfully", "OK"]);
        link.send("AT+APN=x");
        assert!(await_match(&tap, &compile_wait("OK"), Duration::from_secs(1), &AtomicBool::new(false)));
        assert!(!await_match(&tap, &compile_wait("OK"), Duration::from_millis(30), &AtomicBool::new(false)));
        link.drop_link();
        let start = Instant::now();
        assert!(!await_match(&tap, &compile_wait("OK"), Duration::from_secs(5), &AtomicBool::new(false)));
        assert!(start.elapsed() < Duration::from_secs(1), "a dropped link ends the wait at once");
    }

    #[test]
    fn a_run_sends_each_step_and_waits_for_its_reply() {
        let link = FakeLink::console("A", "dev");
        link.connect("A");
        let pb = dragino_1nce();
        let mut steps = Vec::new();
        run(&link, &pb, Duration::from_secs(1), &AtomicBool::new(false), &mut |Report::Step { n, .. }| steps.push(n)).unwrap();
        assert_eq!(steps, (1..=7).collect::<Vec<_>>());
        assert_eq!(link.sent(), pb.steps.iter().map(|s| s.cmd.clone()).collect::<Vec<_>>());
    }

    #[test]
    fn a_run_aborts_at_a_step_without_its_reply() {
        let link = FakeLink::console("A", "dev");
        link.connect("A");
        link.reply("AT+APN=iot.1nce.net", &[]);
        let e = run(&link, &dragino_1nce(), Duration::from_millis(30), &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert_eq!(e, "playbook dragino-nb-1nce aborted at step 2/7 (AT+APN=iot.1nce.net): no reply matching OK");
        assert_eq!(link.sent().len(), 2);
    }

    #[test]
    fn a_run_stops_when_the_link_is_gone() {
        let link = FakeLink::console("A", "dev");
        let e = run(&link, &dragino_1nce(), Duration::from_millis(30), &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert!(e.contains("link lost"), "{e}");
    }
}
