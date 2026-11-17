//! Device templates: a device type as a list of friendly actions that map to AT commands, and as typed settings read
//! from `AT+CFG` ([`crate::settings`]), so nobody needs the raw syntax. The presets ship inside the binary; TOML files
//! in `<config>/aircord/templates/` override or add to them. A template says what exists and what each sends, not how
//! it looks: the window turns it into a control panel and a settings form.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use regex::{NoExpand, Regex};
use serde::Deserialize;

use crate::config;
use crate::redact;
use crate::settings::{RawSetting, Setting};

/// The presets. `dragino-nb.toml` is the generic panel picked by the advertised name; the others are picked by the
/// model `AT+CFG` reports and carry the typed settings.
const PRESETS: &[(&str, &str)] = &[
    ("dragino-nb.toml", include_str!("../presets/dragino-nb.toml")),
    ("dragino-s31-nb.toml", include_str!("../presets/dragino-s31-nb.toml")),
    ("dragino-d20-nb.toml", include_str!("../presets/dragino-d20-nb.toml")),
    ("dragino-d22-nb.toml", include_str!("../presets/dragino-d22-nb.toml")),
    ("dragino-d23-nb.toml", include_str!("../presets/dragino-d23-nb.toml")),
    ("dragino-wqs-nb.toml", include_str!("../presets/dragino-wqs-nb.toml")),
];

/// How a control looks and behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Sends a fixed command.
    Button,
    /// A value field and a command whose `<placeholder>` takes the value.
    Input,
    /// A fixed set of options, each with its own command.
    Select,
    /// Sends a command and shows the value [`Action::extract`] finds in the reply.
    Read,
}

/// One choice of a select action.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub cmd: String,
}

#[derive(Deserialize)]
struct RawAction {
    #[serde(default)]
    label: String,
    #[serde(default)]
    cmd: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    options: Vec<Choice>,
    #[serde(default)]
    parse: String,
    #[serde(default)]
    confirm: bool,
}

#[derive(Deserialize)]
struct RawTemplate {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "match")]
    matches: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    action: Vec<RawAction>,
    #[serde(default)]
    setting: Vec<RawSetting>,
}

/// One control of a device's panel.
#[derive(Clone, Debug)]
pub struct Action {
    pub label: String,
    pub cmd: String,
    pub kind: Kind,
    pub options: Vec<Choice>,
    /// A read action's pattern: its first group, or the whole match, is the value.
    parse: Option<Regex>,
    /// Ask before sending: reboots, factory resets, band changes that drop the uplink. Anything nobody wants from a
    /// stray click on a device already in the field.
    pub confirm: bool,
}

fn placeholder() -> Option<&'static Regex> {
    static RE: std::sync::OnceLock<Option<Regex>> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[^>]*>").ok()).as_ref()
}

impl Action {
    /// The command with its `<placeholder>` replaced by `value`, taken literally (a `$` in it stays a `$`). Commands
    /// without one come back unchanged.
    pub fn render(&self, value: &str) -> String {
        match placeholder() {
            Some(re) => re.replace_all(&self.cmd, NoExpand(value)).into_owned(),
            None => self.cmd.clone(),
        }
    }

    /// The value a read action finds in a reply line: its pattern's first group, or the whole match without one.
    pub fn extract(&self, line: &str) -> Option<String> {
        let caps = self.parse.as_ref()?.captures(line)?;
        caps.get(1).or_else(|| caps.get(0)).map(|m| m.as_str().to_string())
    }
}

/// A device type: its panel's actions and its settings.
#[derive(Clone, Debug)]
pub struct Template {
    pub name: String,
    /// Tested against the advertised name; none: chosen by hand only.
    matches: Option<Regex>,
    /// Tested against the value of the `AT+MODEL` line of `AT+CFG` (`S31-NB,v1.4.0`).
    model: Option<Regex>,
    pub actions: Vec<Action>,
    /// In template order, which is the order changes are sent in.
    pub settings: Vec<Setting>,
}

impl Template {
    /// Whether the template fits a device of this advertised name.
    pub fn matches(&self, name: &str) -> bool {
        self.matches.as_ref().is_some_and(|re| re.is_match(name))
    }

    /// Whether the template fits a device whose `AT+MODEL` value is `model_line` (`S31-NB,v1.4.0`).
    pub fn fits_model(&self, model_line: &str) -> bool {
        self.model.as_ref().is_some_and(|re| re.is_match(model_line.trim()))
    }

    pub fn setting(&self, key: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.key == key)
    }

    /// The settings a form offers for a device: those its `AT+CFG` reported, the expert ones only in expert mode.
    /// Whether each is shown also depends on its `visible_if` and the form's values: see [`Setting::visible`].
    pub fn settings_for<'a>(&'a self, current: &'a crate::cfg::Cfg, expert: bool) -> impl Iterator<Item = &'a Setting> + 'a {
        self.settings.iter().filter(move |s| (expert || !s.expert) && current.get(&s.key).is_some())
    }

    /// The keys of its `type = "secret"` settings, which [`redact::redact_line`] masks on top of the fixed ones.
    pub fn secret_keys(&self) -> Vec<&str> {
        self.settings.iter().filter(|s| s.is_secret()).map(|s| s.key.as_str()).collect()
    }

    /// `line` with every password in it masked: the fixed ones and this template's secrets.
    pub fn redact<'a>(&self, line: &'a str) -> std::borrow::Cow<'a, str> {
        redact::redact_line(line, &self.secret_keys())
    }

    /// The restart action (`ATZ`) the window offers after a transfer that needs one; never sent on its own.
    pub fn reboot_action(&self) -> Option<&Action> {
        self.actions.iter().find(|a| a.kind == Kind::Button && a.cmd.trim().eq_ignore_ascii_case("ATZ"))
    }
}

/// Parses and checks one template.
pub fn parse(text: &str) -> Result<Template, String> {
    let raw: RawTemplate = toml::from_str(text).map_err(|e| e.to_string())?;
    let name = raw.name;
    if name.trim().is_empty() {
        return Err("template: name is required".into());
    }
    let matches = match raw.matches.as_str() {
        "" => None,
        m => Some(Regex::new(m).map_err(|e| format!("template {name:?}: invalid match regexp: {e}"))?),
    };
    let model = match raw.model.as_str() {
        "" => None,
        m => Some(Regex::new(m).map_err(|e| format!("template {name:?}: invalid model regexp: {e}"))?),
    };
    if raw.action.is_empty() && raw.setting.is_empty() {
        return Err(format!("template {name:?}: no actions or settings"));
    }
    let mut settings: Vec<Setting> = Vec::with_capacity(raw.setting.len());
    for s in raw.setting {
        let s = s.check().map_err(|e| format!("template {name:?} {e}"))?;
        if settings.iter().any(|o| o.key == s.key) {
            return Err(format!("template {name:?}: setting {} appears twice", s.key));
        }
        settings.push(s);
    }
    let mut actions = Vec::with_capacity(raw.action.len());
    for a in raw.action {
        let label = a.label;
        if label.trim().is_empty() {
            return Err(format!("template {name:?}: an action is missing its label"));
        }
        let at = |msg: &str| format!("template {name:?} action {label:?}: {msg}");
        let has_cmd = !a.cmd.trim().is_empty();
        let kind = match a.kind.as_str() {
            "button" if has_cmd => Kind::Button,
            "button" => return Err(at("button needs a cmd")),
            "input" if !has_cmd => return Err(at("input needs a cmd")),
            "input" if !placeholder().is_some_and(|re| re.is_match(&a.cmd)) => return Err(at("input cmd needs a <placeholder>")),
            "input" => Kind::Input,
            "select" if a.options.is_empty() => return Err(at("select needs options")),
            "select" if a.options.iter().any(|o| o.text.trim().is_empty() || o.cmd.trim().is_empty()) => {
                return Err(at("every option needs text and cmd"));
            }
            "select" => Kind::Select,
            "read" if !has_cmd => return Err(at("read needs a cmd")),
            "read" if a.parse.trim().is_empty() => return Err(at("read needs a parse regexp")),
            "read" => Kind::Read,
            other => return Err(at(&format!("unknown type {other:?} (want button|input|select|read)"))),
        };
        let parse = match a.parse.as_str() {
            "" => None,
            p => match Regex::new(p) {
                Ok(re) => Some(re),
                Err(e) if kind == Kind::Read => return Err(at(&format!("invalid parse regexp: {e}"))),
                Err(_) => None,
            },
        };
        actions.push(Action { label, cmd: a.cmd, kind, options: a.options, parse, confirm: a.confirm });
    }
    Ok(Template { name, matches, model, actions, settings })
}

/// The presets with the user's templates over them, by name.
#[derive(Clone, Debug, Default)]
pub struct Templates {
    by_name: BTreeMap<String, Template>,
    errors: Vec<String>,
}

impl Templates {
    /// The presets and `<config>/aircord/templates/*.toml`.
    pub fn load() -> Templates {
        Templates::load_from(&config::dir("templates"))
    }

    /// The presets and the TOML files in `user_dir` (which may be missing). A file that fails is listed in
    /// [`Templates::errors`] and left out, so one bad file can't blank the panel.
    pub fn load_from(user_dir: &Path) -> Templates {
        let mut t = Templates::default();
        for (file, text) in PRESETS {
            t.add(file, text);
        }
        let mut files: Vec<_> = fs::read_dir(user_dir)
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "toml")).collect())
            .unwrap_or_default();
        files.sort();
        for path in files {
            let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match fs::read_to_string(&path) {
                Ok(text) => t.add(&file, &text),
                Err(e) => t.errors.push(format!("{file}: {e}")),
            }
        }
        t
    }

    fn add(&mut self, file: &str, text: &str) {
        match parse(text) {
            Ok(tpl) => {
                self.by_name.insert(tpl.name.clone(), tpl);
            }
            Err(e) => self.errors.push(format!("{file}: {e}")),
        }
    }

    /// The first template, in name order, that fits the device name.
    pub fn suggest(&self, device_name: &str) -> Option<&Template> {
        self.by_name.values().find(|t| t.matches(device_name))
    }

    /// The first template, in name order, whose `model` fits the `AT+MODEL` value (`S31-NB,v1.4.0`).
    pub fn for_model(&self, model_line: &str) -> Option<&Template> {
        self.by_name.values().find(|t| t.fits_model(model_line))
    }

    /// The template for a device: by the model `AT+CFG` reported when there is one, else by the advertised name. The
    /// model wins because the name of an NB node is usually its IMEI.
    pub fn select(&self, model_line: Option<&str>, device_name: &str) -> Option<&Template> {
        model_line.and_then(|m| self.for_model(m)).or_else(|| self.suggest(device_name))
    }

    pub fn get(&self, name: &str) -> Option<&Template> {
        self.by_name.get(name)
    }

    /// Every template's name, sorted.
    pub fn names(&self) -> Vec<String> {
        self.by_name.keys().cloned().collect()
    }

    /// Why files were left out.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
name  = "Dragino NB-IoT"
match = "(?i)^d20"

[[action]]
label = "Einstellungen anzeigen"
cmd   = "AT+CFG"
type  = "button"

[[action]]
label = "Intervall (s)"
cmd   = "AT+TDC=<n>"
type  = "input"

[[action]]
label = "Band"
type  = "select"
  [[action.options]]
  text = "B8"
  cmd  = "AT+QBAND=1,8"
"#;

    #[test]
    fn a_valid_template() {
        let t = parse(VALID).unwrap();
        assert_eq!(t.name, "Dragino NB-IoT");
        assert_eq!(t.actions.iter().map(|a| a.kind).collect::<Vec<_>>(), [Kind::Button, Kind::Input, Kind::Select]);
        assert_eq!(t.actions[2].options.len(), 1);
        assert!(t.matches("D20-NB") && t.matches("d20") && !t.matches("x-d20"));
    }

    #[test]
    fn templates_that_cannot_work_are_refused() {
        for (what, text) in [
            ("missing name", "[[action]]\nlabel = \"x\"\ncmd = \"AT\"\ntype = \"button\""),
            ("no actions", "name = \"x\""),
            ("unknown type", "name = \"x\"\n[[action]]\nlabel = \"x\"\ncmd = \"AT\"\ntype = \"slider\""),
            ("input without placeholder", "name = \"x\"\n[[action]]\nlabel = \"x\"\ncmd = \"AT+TDC=\"\ntype = \"input\""),
            ("select without options", "name = \"x\"\n[[action]]\nlabel = \"x\"\ntype = \"select\""),
            ("button without cmd", "name = \"x\"\n[[action]]\nlabel = \"x\"\ntype = \"button\""),
            ("bad match", "name = \"x\"\nmatch = \"(\"\n[[action]]\nlabel = \"x\"\ncmd = \"AT\"\ntype = \"button\""),
            ("read without parse", "name = \"x\"\n[[action]]\nlabel = \"x\"\ncmd = \"AT+VER\"\ntype = \"read\""),
            ("label missing", "name = \"x\"\n[[action]]\ncmd = \"AT\"\ntype = \"button\""),
            ("not toml", "name = "),
        ] {
            assert!(parse(text).is_err(), "{what}");
        }
    }

    #[test]
    fn an_empty_match_never_matches() {
        let t = parse("name = \"x\"\n[[action]]\nlabel = \"x\"\ncmd = \"AT\"\ntype = \"button\"").unwrap();
        assert!(!t.matches("anything"));
    }

    #[test]
    fn read_render_extract_and_confirm() {
        let t = parse("name = \"x\"\n[[action]]\nlabel = \"v\"\ncmd = \"AT+VER\"\ntype = \"read\"\nparse = \"([0-9]+\\\\.[0-9]+)\"\nconfirm = true\n[[action]]\nlabel = \"w\"\ncmd = \"AT+VER\"\ntype = \"read\"\nparse = \"v[0-9]\"").unwrap();
        assert!(t.actions[0].confirm && !t.actions[1].confirm);
        assert_eq!(t.actions[0].extract("Version 1.4.2").as_deref(), Some("1.4"));
        assert_eq!(t.actions[0].extract("nothing"), None);
        assert_eq!(t.actions[1].extract("is v3").as_deref(), Some("v3"));
        let input = parse(VALID).unwrap().actions[1].clone();
        assert_eq!(input.render("3600"), "AT+TDC=3600");
        assert_eq!(input.render("$1"), "AT+TDC=$1", "a value is taken literally");
    }

    #[test]
    fn the_preset_loads_and_marks_destructive_actions() {
        let t = Templates::load_from(Path::new("/nonexistent"));
        assert!(t.errors().is_empty(), "{:?}", t.errors());
        let nb = t.get("Dragino NB-IoT").unwrap();
        let confirmed: Vec<_> = nb.actions.iter().filter(|a| a.confirm).map(|a| a.label.as_str()).collect();
        assert_eq!(confirmed, ["NB-IoT band (Germany)", "Reboot device"]);
        assert_eq!(t.suggest("D23-NB").map(|t| t.name.as_str()), Some("Dragino NB-IoT"));
        assert!(t.suggest("Pixel 9").is_none());
    }

    const S31_CFG: &str = include_str!("../testdata/s31-nb-v1.4.0-cfg.txt");
    const INVENTORY: &str = include_str!("../../../../docs/devices/dragino-nb-at-commands.md");

    fn presets() -> Templates {
        Templates::load_from(Path::new("/nonexistent"))
    }

    #[test]
    fn every_preset_parses() {
        let t = presets();
        assert!(t.errors().is_empty(), "{:?}", t.errors());
        assert_eq!(t.names(), ["Dragino D20-NB", "Dragino D22-NB", "Dragino D23-NB", "Dragino NB-IoT", "Dragino S31-NB", "Dragino WQS-NB"]);
        for name in t.names().iter().filter(|n| *n != "Dragino NB-IoT") {
            let tpl = t.get(name).unwrap();
            assert!(!tpl.settings.is_empty(), "{name}");
            assert!(tpl.matches.is_none(), "{name}: model presets must not take over the advertised-name suggestion");
            assert_eq!(tpl.reboot_action().map(|a| a.confirm), Some(true), "{name}: restart offered, with a question");
            assert!(tpl.settings.iter().filter(|s| s.key == "PWORD" || s.key == "PWD").all(Setting::is_secret), "{name}");
            assert!(tpl.settings.iter().filter(|s| s.key == "PWORD").all(|s| s.expert), "{name}: PWORD is expert only");
        }
    }

    #[test]
    fn the_model_picks_the_template() {
        let t = presets();
        for (line, want) in [
            ("S31-NB,v1.4.0", Some("Dragino S31-NB")),
            ("D20-NB,v1.3.4", Some("Dragino D20-NB")),
            ("D20S-NB,v1.2.1", Some("Dragino D20-NB")),
            ("D22-NB,v1.3.4", Some("Dragino D22-NB")),
            ("D23-NB,v1.3.4", Some("Dragino D23-NB")),
            ("WQS-NB,v1.1.0", Some("Dragino WQS-NB")),
            ("WQS-NS,v1.0.2", Some("Dragino WQS-NB")),
            ("WQS-NB", Some("Dragino WQS-NB")),
            ("S31-NBX,v1.0", None),
            ("LDS12-NB,v1.0", None),
        ] {
            assert_eq!(t.for_model(line).map(|t| t.name.as_str()), want, "{line}");
        }
        let cfg = crate::cfg::Cfg::parse_text(S31_CFG);
        assert_eq!(t.select(cfg.model_line(), "866207050000001").map(|t| t.name.as_str()), Some("Dragino S31-NB"), "BLE name is the IMEI");
        assert_eq!(t.select(None, "D20-NB").map(|t| t.name.as_str()), Some("Dragino NB-IoT"), "no AT+CFG yet: by name");
        assert_eq!(t.select(Some("X1-NB,v1"), "D23-NB").map(|t| t.name.as_str()), Some("Dragino NB-IoT"), "unknown model: by name");
        assert!(t.select(None, "866207050000001").is_none());
    }

    #[test]
    fn the_s31_preset_holds_only_what_the_device_reports_and_accepts_its_values() {
        let tpl = presets().get("Dragino S31-NB").cloned().unwrap();
        let cfg = crate::cfg::Cfg::parse_text(S31_CFG);
        for s in &tpl.settings {
            let value = cfg.get(&s.key).unwrap_or_else(|| panic!("{} is not in the S31 AT+CFG", s.key));
            if !value.is_empty() {
                assert!(s.check(value).is_ok(), "{}: the device's own value {value:?} is refused: {:?}", s.key, s.check(value));
            }
        }
        for key in ["SHTEMP", "SHHUM", "APN", "PRO", "SERVADDR", "TDC"] {
            assert!(tpl.setting(key).is_some_and(|s| !s.expert), "{key} is a lay setting");
        }
        assert_eq!(tpl.settings_for(&cfg, false).count(), tpl.settings.iter().filter(|s| !s.expert).count());
        assert_eq!(tpl.settings_for(&cfg, true).count(), tpl.settings.len());
        assert_eq!(tpl.redact("<< AT+PWORD=kxqmzp"), "<< AT+PWORD=******");
    }

    #[test]
    fn the_d2x_and_wqs_presets_use_only_documented_commands() {
        let t = presets();
        for name in ["Dragino D20-NB", "Dragino D22-NB", "Dragino D23-NB", "Dragino WQS-NB"] {
            let tpl = t.get(name).unwrap();
            let cmds = tpl.actions.iter().flat_map(|a| std::iter::once(a.cmd.clone()).chain(a.options.iter().map(|o| o.cmd.clone())));
            for cmd in cmds.filter(|c| !c.is_empty()) {
                let verb = cmd.split('=').next().unwrap_or_default();
                assert!(INVENTORY.contains(&format!("`{verb}")), "{name}: {verb} is not in the inventory");
            }
            for s in &tpl.settings {
                assert!(INVENTORY.contains(&format!("`AT+{}=", s.key)), "{name}: AT+{} is not in the inventory", s.key);
            }
            assert!(tpl.setting("QBAND").is_none(), "{name}: QBAND waits for a device test");
        }
        let alarms = |name: &str| t.get(name).unwrap().settings.iter().filter(|s| s.key.starts_with("TEMPALARM")).count();
        assert_eq!([alarms("Dragino D20-NB"), alarms("Dragino D22-NB"), alarms("Dragino D23-NB")], [1, 2, 3], "one alarm per probe");
        for name in ["Dragino D20-NB", "Dragino D22-NB", "Dragino D23-NB", "Dragino WQS-NB", "Dragino S31-NB"] {
            let tpl = t.get(name).unwrap();
            let refused = |key: &str, v: &str| tpl.setting(key).is_some_and(|s| s.check(v).is_err());
            assert!(refused("TDC", "59") && refused("TDC", "86401") && !refused("TDC", "60"), "{name}: TDC bounds");
            assert!(refused("CSQTIME", "0") && refused("CSQTIME", "61"), "{name}: CSQTIME bounds");
            assert!(refused("RXDL", "10001") && !refused("RXDL", "1000"), "{name}: RXDL bound");
        }
        for name in ["Dragino D20-NB", "Dragino D22-NB", "Dragino D23-NB"] {
            let ps = t.get(name).unwrap().setting("POWER_SAVE").unwrap();
            assert_eq!(ps.risk, crate::settings::Risk::Network, "{name}: a wrong window looks like a dead node");
            assert!(ps.check("0,8,8").is_err() && ps.check("0,22,6").is_ok(), "{name}: start and end must differ");
        }
        let tr = t.get("Dragino D20-NB").unwrap().setting("TR").unwrap();
        assert_eq!(tr.check("120").unwrap_err(), "Bitte mindestens 180 Sekunden eingeben.");
    }

    #[test]
    fn settings_in_a_template_and_their_errors() {
        let t = parse("name = \"x\"\nmodel = '^X1-NB(,|$)'\n[[setting]]\nkey = \"TDC\"\nlabel = \"t\"\ngroup = \"g\"\ntype = \"duration\"\nunit = \"s\"")
            .unwrap();
        assert!(t.actions.is_empty() && t.settings.len() == 1, "settings alone make a template");
        assert!(t.fits_model("X1-NB,v1.0") && !t.fits_model("X1-NBB"));
        let twice = "name = \"x\"\n[[setting]]\nkey = \"TDC\"\nlabel = \"t\"\ngroup = \"g\"\ntype = \"text\"\n\
                     [[setting]]\nkey = \"TDC\"\nlabel = \"u\"\ngroup = \"g\"\ntype = \"text\"";
        assert!(parse(twice).unwrap_err().contains("appears twice"));
        assert!(parse("name = \"x\"\nmodel = '('\n[[action]]\nlabel = \"x\"\ncmd = \"AT\"\ntype = \"button\"").is_err());
        let bad = parse("name = \"x\"\n[[setting]]\nkey = \"TDC\"\nlabel = \"t\"\ngroup = \"g\"\ntype = \"slider\"").unwrap_err();
        assert!(bad.starts_with("template \"x\" setting TDC: unknown type"), "{bad}");
    }

    #[test]
    fn a_user_template_overrides_a_preset_and_a_bad_file_is_collected() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("mine.toml"), "name = \"Dragino NB-IoT\"\n[[action]]\nlabel = \"Mine\"\ncmd = \"AT\"\ntype = \"button\"")
            .unwrap();
        fs::write(d.path().join("zz.toml"), "name = \"Zed\"\n[[action]]\nlabel = \"Z\"\ncmd = \"AT\"\ntype = \"button\"").unwrap();
        fs::write(d.path().join("bad.toml"), "name = ").unwrap();
        fs::write(d.path().join("notes.txt"), "ignored").unwrap();
        let t = Templates::load_from(d.path());
        assert_eq!(t.get("Dragino NB-IoT").unwrap().actions[0].label, "Mine");
        assert_eq!(
            t.names(),
            ["Dragino D20-NB", "Dragino D22-NB", "Dragino D23-NB", "Dragino NB-IoT", "Dragino S31-NB", "Dragino WQS-NB", "Zed"],
            "the user's Zed is added, the presets stay, the override replaces rather than duplicates"
        );
        assert_eq!(t.errors().len(), 1);
        assert!(t.errors()[0].starts_with("bad.toml"));
    }
}
