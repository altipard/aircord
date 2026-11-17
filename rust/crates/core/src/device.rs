//! Device templates: a device type as a list of friendly actions that map to AT commands, so nobody needs the raw
//! syntax. The presets ship inside the binary; TOML files in `<config>/aircord/templates/` override or add to them. A
//! template says what actions exist and what each sends, not how they look: the window turns it into a control panel.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use regex::{NoExpand, Regex};
use serde::Deserialize;

use crate::config;

/// The presets, read from the Go tree so both builds ship the same ones.
const PRESETS: &[(&str, &str)] = &[("dragino-nb.toml", include_str!("../../../../internal/device/presets/dragino-nb.toml"))];

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
    action: Vec<RawAction>,
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

/// A device type and its panel's actions.
#[derive(Clone, Debug)]
pub struct Template {
    pub name: String,
    /// Tested against the advertised name; none: chosen by hand only.
    matches: Option<Regex>,
    pub actions: Vec<Action>,
}

impl Template {
    /// Whether the template fits a device of this advertised name.
    pub fn matches(&self, name: &str) -> bool {
        self.matches.as_ref().is_some_and(|re| re.is_match(name))
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
    if raw.action.is_empty() {
        return Err(format!("template {name:?}: no actions"));
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
    Ok(Template { name, matches, actions })
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
        assert_eq!(t.names(), ["Dragino NB-IoT", "Zed"]);
        assert_eq!(t.errors().len(), 1);
        assert!(t.errors()[0].starts_with("bad.toml"));
    }
}
