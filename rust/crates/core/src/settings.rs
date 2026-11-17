//! Typed settings: one `[[setting]]` of a device template, a value the device reports in `AT+CFG` as `AT+<KEY>=<value>`
//! and takes back as `AT+<KEY>=<value>`. A setting says what kind of value it is (a duration, a choice, a switch, text),
//! what is allowed, how risky a change is and when it is shown, so a window can offer a form instead of AT syntax.
//!
//! Load errors are for template authors and in English, like the rest of the template checks. Input errors are for the
//! person in the field and in plain German.

use regex::Regex;
use serde::Deserialize;

use crate::redact;

/// What a wrong value costs. The window asks before it transfers a [`Risk::Network`] or [`Risk::Irreversible`] change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Risk {
    #[default]
    None,
    /// A wrong value takes the device off the network or away from its server; fixing it needs a visit.
    Network,
    /// The change restarts the device.
    Reboot,
    /// The change cannot be taken back in the field (a forgotten password).
    Irreversible,
}

impl Risk {
    /// Whether the window must ask before transferring a change of this risk.
    pub fn needs_confirm(self) -> bool {
        matches!(self, Risk::Network | Risk::Irreversible)
    }
}

/// The unit a duration is stored in on the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Ms,
    S,
    Min,
    H,
}

impl Unit {
    fn parse(s: &str) -> Option<Unit> {
        match s {
            "ms" => Some(Unit::Ms),
            "s" => Some(Unit::S),
            "min" => Some(Unit::Min),
            "h" => Some(Unit::H),
            _ => None,
        }
    }

    /// The German word for `n` of this unit: "1 Minute", "15 Minuten".
    fn amount(self, n: i64) -> String {
        let (one, many) = match self {
            Unit::Ms => ("Millisekunde", "Millisekunden"),
            Unit::S => ("Sekunde", "Sekunden"),
            Unit::Min => ("Minute", "Minuten"),
            Unit::H => ("Stunde", "Stunden"),
        };
        format!("{n} {}", if n == 1 { one } else { many })
    }

    fn in_ms(self) -> i64 {
        match self {
            Unit::Ms => 1,
            Unit::S => 1_000,
            Unit::Min => 60_000,
            Unit::H => 3_600_000,
        }
    }
}

/// A duration as a person reads it: `900` seconds is "15 Minuten", `7200` "2 Stunden", `90` stays "90 Sekunden".
pub fn duration_text(value: i64, unit: Unit) -> String {
    let ms = value.saturating_mul(unit.in_ms());
    for bigger in [Unit::H, Unit::Min, Unit::S] {
        if bigger.in_ms() > unit.in_ms() && ms != 0 && ms % bigger.in_ms() == 0 {
            return bigger.amount(ms / bigger.in_ms());
        }
    }
    unit.amount(value)
}

/// One option of a choice: what the person sees and what the device gets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueChoice {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub value: String,
}

/// The kind of value and what is allowed.
#[derive(Clone, Debug)]
pub enum Field {
    /// A whole number of `unit`, at least 0, with suggested values.
    Duration {
        unit: Unit,
        presets: Vec<i64>,
        min: Option<i64>,
        max: Option<i64>,
    },
    /// A whole number, with an optional display unit (`°C`, `%`).
    Number {
        unit: String,
        presets: Vec<i64>,
        min: Option<i64>,
        max: Option<i64>,
    },
    /// One of `options`; with `custom`, any other value that passes `pattern` and `max_len`.
    Choice {
        options: Vec<ValueChoice>,
        custom: bool,
        pattern: Option<Regex>,
        max_len: Option<usize>,
    },
    /// A switch whose positions send `on` and `off`; inverted device logic (`AT+POWER_SAVE`'s 0 = on) is just values.
    Toggle {
        on: String,
        off: String,
    },
    Text {
        pattern: Option<Regex>,
        max_len: Option<usize>,
    },
    /// Text that is never shown or logged.
    Secret {
        pattern: Option<Regex>,
        max_len: Option<usize>,
    },
}

/// When a setting is shown: `visible_if = "PRO=3,*"` shows it only while `PRO` is `3,` and anything.
#[derive(Clone, Debug)]
pub struct Condition {
    pub key: String,
    pub pattern: String,
    re: Regex,
}

impl Condition {
    fn parse(text: &str) -> Result<Condition, String> {
        let Some((key, pattern)) = text.split_once('=') else { return Err(format!("visible_if {text:?} is not KEY=pattern")) };
        let key = key.trim();
        if !valid_key(key) {
            return Err(format!("visible_if {text:?}: {key:?} is not a setting key"));
        }
        let glob: Vec<String> = pattern.trim().split('*').map(regex::escape).collect();
        let re = Regex::new(&format!("^{}$", glob.join(".*"))).map_err(|e| format!("visible_if {text:?}: {e}"))?;
        Ok(Condition { key: key.to_string(), pattern: pattern.trim().to_string(), re })
    }

    /// Whether it holds when `KEY` has `value`; a key with no value never satisfies it.
    pub fn holds(&self, value: Option<&str>) -> bool {
        value.is_some_and(|v| self.re.is_match(v.trim()))
    }
}

/// One setting of a template.
#[derive(Clone, Debug)]
pub struct Setting {
    /// The name in `AT+CFG`; the device gets `AT+<key>=<value>`.
    pub key: String,
    pub label: String,
    /// The task it belongs to ("Netz", "Messen & Senden"); the window groups by it.
    pub group: String,
    /// A sentence for the person in the field; also the hint when a value has the wrong format.
    pub help: String,
    pub field: Field,
    pub risk: Risk,
    /// Takes effect only after a restart (`ATZ`).
    pub needs_reboot: bool,
    pub visible_if: Option<Condition>,
    /// Shown only in expert mode.
    pub expert: bool,
    /// The value may be cleared: the device gets `AT+<key>=NULL`, as `AT+CFG` prints an unset value. For text an empty
    /// input clears it; a secret only by an explicit clear ([`crate::transfer::plan_clearing`]), since an empty password
    /// field means "unchanged".
    pub clearable: bool,
    /// Positions (from 1) of comma-separated parts of a typed value that must differ: `[2, 3]` refuses
    /// `AT+POWER_SAVE=0,8,8`, a window that starts where it ends.
    pub distinct: Vec<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSetting {
    #[serde(default)]
    key: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    group: String,
    #[serde(default)]
    help: String,
    #[serde(default, rename = "type")]
    kind: String,
    unit: Option<String>,
    presets: Option<Vec<i64>>,
    min: Option<i64>,
    max: Option<i64>,
    options: Option<Vec<ValueChoice>>,
    custom: Option<bool>,
    on: Option<String>,
    off: Option<String>,
    pattern: Option<String>,
    max_len: Option<usize>,
    clearable: Option<bool>,
    distinct: Option<Vec<usize>>,
    #[serde(default)]
    risk: String,
    #[serde(default)]
    needs_reboot: bool,
    #[serde(default)]
    visible_if: String,
    #[serde(default)]
    expert: bool,
}

/// Setting keys as `AT+CFG` prints them: `TDC`, `5VT`, `POWER_SAVE`.
pub(crate) fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

/// A pattern must match the whole input, so a template author can't forget the anchors.
fn whole(pattern: Option<&str>) -> Result<Option<Regex>, String> {
    match pattern {
        None | Some("") => Ok(None),
        Some(p) => Regex::new(&format!("^(?:{p})$")).map(Some).map_err(|e| format!("invalid pattern: {e}")),
    }
}

fn bounds(min: Option<i64>, max: Option<i64>, presets: &[i64]) -> Result<(), String> {
    if let (Some(lo), Some(hi)) = (min, max)
        && lo > hi
    {
        return Err(format!("min {lo} is above max {hi}"));
    }
    if let Some(p) = presets.iter().find(|&&p| min.is_some_and(|lo| p < lo) || max.is_some_and(|hi| p > hi)) {
        return Err(format!("preset {p} is outside min/max"));
    }
    Ok(())
}

impl RawSetting {
    /// The first type-specific field given that the type does not use.
    fn stray(&self, allowed: &[&str]) -> Option<&'static str> {
        let given = [
            ("unit", self.unit.is_some()),
            ("presets", self.presets.is_some()),
            ("min", self.min.is_some()),
            ("max", self.max.is_some()),
            ("options", self.options.is_some()),
            ("custom", self.custom.is_some()),
            ("on", self.on.is_some()),
            ("off", self.off.is_some()),
            ("pattern", self.pattern.is_some()),
            ("max_len", self.max_len.is_some()),
            ("clearable", self.clearable.is_some()),
            ("distinct", self.distinct.is_some()),
        ];
        given.into_iter().find(|(name, set)| *set && !allowed.contains(name)).map(|(name, _)| name)
    }

    pub(crate) fn check(self) -> Result<Setting, String> {
        let key = self.key.trim().to_string();
        if !valid_key(&key) {
            return Err(format!("setting key {key:?} must be the AT+CFG name in capitals (TDC, 5VT, POWER_SAVE)"));
        }
        let at = |msg: &str| format!("setting {key}: {msg}");
        if self.label.trim().is_empty() {
            return Err(at("label is required"));
        }
        if self.group.trim().is_empty() {
            return Err(at("group is required"));
        }
        let allowed: &[&str] = match self.kind.as_str() {
            "duration" | "number" => &["unit", "presets", "min", "max"],
            "choice" => &["options", "custom", "pattern", "max_len", "distinct"],
            "toggle" => &["on", "off"],
            "text" | "secret" => &["pattern", "max_len", "clearable", "distinct"],
            other => return Err(at(&format!("unknown type {other:?} (want duration|choice|toggle|text|secret|number)"))),
        };
        if let Some(name) = self.stray(allowed) {
            return Err(at(&format!("{name} does not apply to type {}", self.kind)));
        }
        if self.max_len == Some(0) {
            return Err(at("max_len must be at least 1"));
        }
        let distinct = self.distinct.clone().unwrap_or_default();
        if self.distinct.is_some()
            && (distinct.len() < 2 || distinct.contains(&0) || (1..distinct.len()).any(|i| distinct[..i].contains(&distinct[i])))
        {
            return Err(at("distinct needs two or more different positions, counted from 1"));
        }
        let pattern = whole(self.pattern.as_deref()).map_err(|e| at(&e))?;
        let presets = self.presets.clone().unwrap_or_default();
        let field = match self.kind.as_str() {
            "duration" => {
                let unit = self.unit.as_deref().unwrap_or_default();
                let Some(unit) = Unit::parse(unit) else { return Err(at(&format!("duration needs unit ms|s|min|h, not {unit:?}"))) };
                if self.min.is_some_and(|m| m < 0) || presets.iter().any(|&p| p < 0) {
                    return Err(at("a duration cannot be negative"));
                }
                bounds(self.min, self.max, &presets).map_err(|e| at(&e))?;
                Field::Duration { unit, presets, min: self.min, max: self.max }
            }
            "number" => {
                bounds(self.min, self.max, &presets).map_err(|e| at(&e))?;
                Field::Number { unit: self.unit.clone().unwrap_or_default(), presets, min: self.min, max: self.max }
            }
            "choice" => {
                let options = self.options.clone().unwrap_or_default();
                let custom = self.custom.unwrap_or(false);
                if options.is_empty() {
                    return Err(at("choice needs options"));
                }
                if options.iter().any(|o| o.text.trim().is_empty() || o.value.trim().is_empty()) {
                    return Err(at("every option needs text and value"));
                }
                if (1..options.len()).any(|i| options[..i].iter().any(|o| o.value == options[i].value)) {
                    return Err(at("two options have the same value"));
                }
                if !custom && (pattern.is_some() || self.max_len.is_some() || self.distinct.is_some()) {
                    return Err(at("pattern, max_len and distinct only check free text: set custom = true"));
                }
                Field::Choice { options, custom, pattern, max_len: self.max_len }
            }
            "toggle" => {
                let on = self.on.clone().unwrap_or_default();
                let off = self.off.clone().unwrap_or_default();
                if on.trim().is_empty() || off.trim().is_empty() {
                    return Err(at("toggle needs on and off"));
                }
                if on.trim() == off.trim() {
                    return Err(at("on and off are the same"));
                }
                Field::Toggle { on: on.trim().into(), off: off.trim().into() }
            }
            "text" => Field::Text { pattern, max_len: self.max_len },
            _ => Field::Secret { pattern, max_len: self.max_len },
        };
        let risk = match self.risk.as_str() {
            "" | "none" => Risk::None,
            "network" => Risk::Network,
            "reboot" => Risk::Reboot,
            "irreversible" => Risk::Irreversible,
            other => return Err(at(&format!("unknown risk {other:?} (want none|network|reboot|irreversible)"))),
        };
        let visible_if = match self.visible_if.trim() {
            "" => None,
            c => Some(Condition::parse(c).map_err(|e| at(&e))?),
        };
        Ok(Setting {
            key,
            label: self.label.trim().into(),
            group: self.group.trim().into(),
            help: self.help.trim().into(),
            field,
            risk,
            needs_reboot: self.needs_reboot,
            visible_if,
            expert: self.expert,
            clearable: self.clearable.unwrap_or(false),
            distinct,
        })
    }
}

fn parse_int(input: &str) -> Option<i64> {
    input.trim().parse().ok()
}

/// Keys whose value is a comma-separated list the firmware prints without spaces: `AT+DNSCFG="8.8.8.8","8.8.4.4"`
/// (S31-NB v1.4.0, `docs/devices/s31-nb-cfg-sample.md`). Whether it also takes spaces is untested, so it gets the form it
/// prints. Only listed keys: in other text (an MQTT topic, a user name) a space after a comma may be part of the value.
const SPACELESS_LISTS: &[&str] = &["DNSCFG"];

/// A checked value in the form the device prints it.
fn device_form(key: &str, value: String) -> String {
    if SPACELESS_LISTS.contains(&key) { value.split(',').map(str::trim).collect::<Vec<_>>().join(",") } else { value }
}

/// How a wanted value is compared with what the device reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    /// As text, outer spaces aside: secrets, where every character counts.
    Exact,
    /// By value: `0900` is `900`.
    Number,
    /// Part by part between commas, each without outer spaces and double quotes: `"8.8.8.8", "8.8.4.4"` is
    /// `"8.8.8.8","8.8.4.4"`, however the firmware prints it.
    Loose,
}

impl Compare {
    pub fn same(self, a: &str, b: &str) -> bool {
        match self {
            Compare::Number => match (parse_int(a), parse_int(b)) {
                (Some(x), Some(y)) => x == y,
                _ => a.trim() == b.trim(),
            },
            Compare::Loose => {
                let parts = |v: &str| v.split(',').map(|p| p.trim().trim_matches('"').trim().to_string()).collect::<Vec<_>>();
                parts(a) == parts(b)
            }
            Compare::Exact => a.trim() == b.trim(),
        }
    }
}

impl Setting {
    /// Whether its value is never shown: `type = "secret"`, or a key that is always secret ([`redact::ALWAYS_SECRET`]).
    pub fn is_secret(&self) -> bool {
        matches!(self.field, Field::Secret { .. }) || redact::is_always_secret(&self.key)
    }

    /// Whether it is shown, given how a key's value is looked up (the form's value over the device's).
    pub fn visible<'a>(&self, value_of: impl Fn(&str) -> Option<&'a str>) -> bool {
        self.visible_if.as_ref().is_none_or(|c| c.holds(value_of(&c.key)))
    }

    /// How its values are compared with the device's.
    pub fn compare(&self) -> Compare {
        match self.field {
            Field::Duration { .. } | Field::Number { .. } => Compare::Number,
            Field::Text { .. } | Field::Choice { .. } if !self.is_secret() => Compare::Loose,
            _ => Compare::Exact,
        }
    }

    /// Whether two values mean the same, by [`Setting::compare`].
    pub fn same(&self, a: &str, b: &str) -> bool {
        self.compare().same(a, b)
    }

    /// A value as the person sees it: a choice's text, "Ein"/"Aus", a duration in words, a secret as asterisks.
    pub fn shown(&self, value: &str) -> String {
        if self.is_secret() {
            return redact::MASK.into();
        }
        let v = value.trim();
        match &self.field {
            Field::Duration { unit, .. } => parse_int(v).map_or_else(|| v.to_string(), |n| duration_text(n, *unit)),
            Field::Number { unit, .. } if !unit.is_empty() && !v.is_empty() => format!("{v} {unit}"),
            Field::Choice { options, .. } => options.iter().find(|o| o.value == v).map_or_else(|| v.to_string(), |o| o.text.clone()),
            Field::Toggle { on, .. } if v == on => "Ein".into(),
            Field::Toggle { off, .. } if v == off => "Aus".into(),
            _ => v.to_string(),
        }
    }

    /// Checks an entered value and returns what is sent, or why not in words for the person in the field. A secret's
    /// message never repeats the input. For a `clearable` setting an empty input is fine and comes back empty, which
    /// [`crate::transfer`] sends as `NULL`; for a secret it only gets here from an explicit clear, because
    /// [`crate::transfer::plan`] takes an empty secret as unchanged.
    pub fn check(&self, input: &str) -> Result<String, String> {
        if input.contains(['\r', '\n']) {
            return Err("Zeilenumbrüche sind nicht erlaubt.".into());
        }
        let v = input.trim();
        let check_text = |pattern: &Option<Regex>, max_len: &Option<usize>| -> Result<String, String> {
            if v.is_empty() {
                return if self.clearable { Ok(String::new()) } else { Err("Bitte einen Wert eingeben.".into()) };
            }
            let n = v.chars().count();
            if let Some(max) = *max_len
                && n > max
            {
                return Err(format!("Höchstens {max} Zeichen erlaubt (eingegeben: {n})."));
            }
            let hint = || if self.help.is_empty() { String::new() } else { format!(" {}", self.help) };
            if pattern.as_ref().is_some_and(|re| !re.is_match(v)) {
                return Err(format!("Die Eingabe hat nicht das erwartete Format.{}", hint()));
            }
            let parts: Vec<&str> = v.split(',').map(str::trim).collect();
            let picked: Vec<&str> = self.distinct.iter().filter_map(|&i| parts.get(i - 1).copied()).collect();
            if let Some(same) = (1..picked.len()).find_map(|i| picked[..i].contains(&picked[i]).then_some(picked[i])) {
                if self.is_secret() {
                    return Err(format!("Zwei Angaben sind gleich, sie müssen sich unterscheiden.{}", hint()));
                }
                return Err(format!("Zwei Angaben sind gleich ({same}), sie müssen sich unterscheiden.{}", hint()));
            }
            Ok(v.to_string())
        };
        let checked = match &self.field {
            Field::Duration { unit, min, max, .. } => {
                let Some(n) = parse_int(v) else { return Err("Bitte eine ganze Zahl eingeben.".into()) };
                if n < 0 {
                    return Err("Eine Dauer kann nicht negativ sein.".into());
                }
                if let Some(lo) = *min
                    && n < lo
                {
                    return Err(format!("Bitte mindestens {} eingeben.", unit.amount(lo)));
                }
                if let Some(hi) = *max
                    && n > hi
                {
                    return Err(format!("Bitte höchstens {} eingeben.", unit.amount(hi)));
                }
                Ok(n.to_string())
            }
            Field::Number { unit, min, max, .. } => {
                let Some(n) = parse_int(v) else { return Err("Bitte eine ganze Zahl eingeben.".into()) };
                let with_unit = |x: i64| if unit.is_empty() { x.to_string() } else { format!("{x} {unit}") };
                if let Some(lo) = *min
                    && n < lo
                {
                    return Err(format!("Der Wert muss mindestens {} sein.", with_unit(lo)));
                }
                if let Some(hi) = *max
                    && n > hi
                {
                    return Err(format!("Der Wert darf höchstens {} sein.", with_unit(hi)));
                }
                Ok(n.to_string())
            }
            Field::Choice { options, custom, pattern, max_len } => {
                if options.iter().any(|o| o.value == v) {
                    return Ok(v.to_string());
                }
                if !custom {
                    return Err("Bitte eine der angebotenen Möglichkeiten wählen.".into());
                }
                check_text(pattern, max_len)
            }
            Field::Toggle { on, off } if v == on || v == off => Ok(v.to_string()),
            Field::Toggle { .. } => Err("Bitte ein- oder ausschalten.".into()),
            Field::Text { pattern, max_len } | Field::Secret { pattern, max_len } => check_text(pattern, max_len),
        };
        checked.map(|v| device_form(&self.key, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(toml_text: &str) -> Result<Setting, String> {
        let raw: RawSetting = toml::from_str(toml_text).map_err(|e| e.to_string())?;
        raw.check()
    }

    fn ok(toml_text: &str) -> Setting {
        setting(toml_text).unwrap()
    }

    const TDC: &str = "key = \"TDC\"\nlabel = \"Sendeintervall\"\ngroup = \"Messen & Senden\"\ntype = \"duration\"\nunit = \"s\"\n\
                       presets = [900, 3600]\nmin = 60\nmax = 86400";

    #[test]
    fn a_duration_checks_range_and_reads_in_words() {
        let s = ok(TDC);
        assert_eq!(s.check(" 900 ").unwrap(), "900");
        assert_eq!(s.check("15 min").unwrap_err(), "Bitte eine ganze Zahl eingeben.");
        assert_eq!(s.check("30").unwrap_err(), "Bitte mindestens 60 Sekunden eingeben.");
        assert_eq!(s.check("90000").unwrap_err(), "Bitte höchstens 86400 Sekunden eingeben.");
        assert_eq!(s.check("-5").unwrap_err(), "Eine Dauer kann nicht negativ sein.");
        assert_eq!(s.shown("900"), "15 Minuten");
        assert_eq!(s.shown("7200"), "2 Stunden");
        assert_eq!(s.shown("90"), "90 Sekunden");
        assert!(s.same("0900", "900") && !s.same("900", "1800"));
        assert_eq!(duration_text(1, Unit::Min), "1 Minute");
        assert_eq!(duration_text(2000, Unit::Ms), "2 Sekunden");
        assert_eq!(duration_text(0, Unit::S), "0 Sekunden");
    }

    #[test]
    fn a_number_names_its_unit_in_errors() {
        let s = ok("key = \"NOUD\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"number\"\nmin = 1\nmax = 32");
        assert_eq!(s.check("33").unwrap_err(), "Der Wert darf höchstens 32 sein.");
        assert_eq!(s.check("0").unwrap_err(), "Der Wert muss mindestens 1 sein.");
        assert_eq!(s.check("8").unwrap(), "8");
        let t = ok("key = \"T\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"number\"\nunit = \"°C\"\nmin = -40");
        assert_eq!(t.check("-41").unwrap_err(), "Der Wert muss mindestens -40 °C sein.");
        assert_eq!(t.shown("20"), "20 °C");
    }

    #[test]
    fn a_choice_with_and_without_free_text() {
        let fixed = ok("key = \"MQOS\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"choice\"\n\
                        options = [{text = \"Höchstens einmal\", value = \"0\"}, {text = \"Mindestens einmal\", value = \"1\"}]");
        assert_eq!(fixed.check("1").unwrap(), "1");
        assert_eq!(fixed.check("2").unwrap_err(), "Bitte eine der angebotenen Möglichkeiten wählen.");
        assert_eq!(fixed.shown("0"), "Höchstens einmal");
        assert_eq!(fixed.shown("7"), "7", "an unknown value is shown as it is");
        let apn =
            ok("key = \"APN\"\nlabel = \"x\"\ngroup = \"g\"\nhelp = \"Zum Beispiel iot.1nce.net.\"\ntype = \"choice\"\ncustom = true\n\
                      pattern = '[A-Za-z0-9][A-Za-z0-9._-]*'\nmax_len = 20\noptions = [{text = \"1NCE\", value = \"iot.1nce.net\"}]");
        assert_eq!(apn.check("internet.m2m").unwrap(), "internet.m2m");
        assert_eq!(apn.check("bad apn").unwrap_err(), "Die Eingabe hat nicht das erwartete Format. Zum Beispiel iot.1nce.net.");
        assert_eq!(apn.check("").unwrap_err(), "Bitte einen Wert eingeben.");
        assert_eq!(apn.check("a".repeat(21).as_str()).unwrap_err(), "Höchstens 20 Zeichen erlaubt (eingegeben: 21).");
        assert_eq!(apn.check("x\r\nATZ").unwrap_err(), "Zeilenumbrüche sind nicht erlaubt.");
    }

    #[test]
    fn a_toggle_with_inverted_logic() {
        let s = ok("key = \"POWER_SAVE\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"toggle\"\non = \"0,8,16\"\noff = \"1,0,0\"");
        assert_eq!(s.check("1,0,0").unwrap(), "1,0,0");
        assert_eq!(s.check("1").unwrap_err(), "Bitte ein- oder ausschalten.");
        assert_eq!((s.shown("0,8,16").as_str(), s.shown("1,0,0").as_str()), ("Ein", "Aus"));
    }

    #[test]
    fn text_compares_loosely_but_secrets_exactly() {
        let dns = ok("key = \"DNSCFG\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"text\"");
        assert!(dns.same("\"8.8.8.8\",\"8.8.4.4\"", " \"8.8.8.8\" , \"8.8.4.4\" "));
        assert!(dns.same("\"8.8.8.8\",\"8.8.4.4\"", "8.8.8.8,8.8.4.4"));
        assert!(!dns.same("\"8.8.8.8\",\"8.8.4.4\"", "\"8.8.4.4\",\"8.8.8.8\""), "order counts");
        assert!(!dns.same("a b", "ab"), "spaces inside a part count");
        let secret = ok("key = \"TOKEN\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"secret\"");
        assert!(!secret.same("\"x\"", "x"), "a secret is compared exactly");
        assert_eq!(secret.compare(), Compare::Exact);
    }

    #[test]
    fn dns_servers_go_out_without_spaces_other_text_as_entered() {
        let dns = ok("key = \"DNSCFG\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"text\"");
        assert_eq!(dns.check(" \"1.1.1.1\" ,  \"8.8.4.4\" ").unwrap(), "\"1.1.1.1\",\"8.8.4.4\"");
        assert_eq!(dns.check("\"1.1.1.1\"").unwrap(), "\"1.1.1.1\"");
        let topic = ok("key = \"PUBTOPIC\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"text\"");
        assert_eq!(topic.check("a, b").unwrap(), "a, b", "only the listed keys");
    }

    #[test]
    fn a_clearable_field_takes_an_empty_input() {
        let s = ok("key = \"UNAME\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"text\"\nclearable = true\npattern = '[a-z]+'");
        assert!(s.clearable);
        assert_eq!(s.check("  ").unwrap(), "", "empty clears, the pattern does not apply");
        assert_eq!(s.check("abc").unwrap(), "abc");
        assert!(s.check("ABC").is_err());
        assert!(!ok("key = \"UNAME\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"text\"").clearable);
        assert!(setting("key = \"T\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"duration\"\nunit = \"s\"\nclearable = true").is_err());
    }

    #[test]
    fn distinct_parts_must_differ() {
        let s = ok("key = \"POWER_SAVE\"\nlabel = \"x\"\ngroup = \"g\"\nhelp = \"0,Start,Ende.\"\ntype = \"choice\"\ncustom = true\n\
                    pattern = '0,[0-9]{1,2},[0-9]{1,2}'\ndistinct = [2, 3]\noptions = [{text = \"Immer\", value = \"1,0,0\"}]");
        assert_eq!(s.check("0,8,8").unwrap_err(), "Zwei Angaben sind gleich (8), sie müssen sich unterscheiden. 0,Start,Ende.");
        assert_eq!(s.check("0,8,16").unwrap(), "0,8,16");
        assert_eq!(s.check("1,0,0").unwrap(), "1,0,0", "an offered option is not checked");
        let base = "key = \"X\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"text\"\n";
        for bad in ["distinct = [2]", "distinct = [0, 1]", "distinct = [2, 2]"] {
            assert!(setting(&format!("{base}{bad}")).is_err(), "{bad}");
        }
        assert!(
            setting("key = \"X\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"toggle\"\non = \"1\"\noff = \"0\"\ndistinct = [1, 2]").is_err()
        );
    }

    #[test]
    fn a_secret_is_never_shown_or_repeated() {
        let s = ok("key = \"PWORD\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"secret\"\npattern = '[a-z]{6}'");
        assert!(s.is_secret());
        assert_eq!(s.shown("kxqmzp"), "******");
        let e = s.check("ABCDEF").unwrap_err();
        assert!(!e.contains("ABCDEF"), "{e}");
        assert!(s.check("abcdefg").is_err(), "the pattern is anchored");
        let pwd_as_text = ok("key = \"PWD\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"text\"");
        assert!(pwd_as_text.is_secret(), "PWD is secret whatever the template says");
    }

    #[test]
    fn visibility_follows_a_glob() {
        let s = ok("key = \"UNAME\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"text\"\nvisible_if = \"PRO=3,*\"");
        assert!(s.visible(|k| (k == "PRO").then_some("3,5")));
        assert!(!s.visible(|k| (k == "PRO").then_some("2,5")));
        assert!(!s.visible(|k| (k == "PRO").then_some("13,5")), "anchored");
        assert!(!s.visible(|_| None), "no value, not shown");
        assert!(ok(TDC).visible(|_| None));
    }

    #[test]
    fn settings_that_cannot_work_are_refused() {
        let base = "label = \"x\"\ngroup = \"g\"\n";
        for (what, rest) in [
            ("lower-case key", "key = \"tdc\"\ntype = \"text\""),
            ("no key", "type = \"text\""),
            ("unknown type", "key = \"X\"\ntype = \"slider\""),
            ("duration without unit", "key = \"X\"\ntype = \"duration\""),
            ("bad unit", "key = \"X\"\ntype = \"duration\"\nunit = \"d\""),
            ("min above max", "key = \"X\"\ntype = \"number\"\nmin = 5\nmax = 1"),
            ("preset out of range", "key = \"X\"\ntype = \"duration\"\nunit = \"s\"\nmin = 60\npresets = [30]"),
            ("choice without options", "key = \"X\"\ntype = \"choice\""),
            ("option without value", "key = \"X\"\ntype = \"choice\"\noptions = [{text = \"a\"}]"),
            (
                "duplicate option",
                "key = \"X\"\ntype = \"choice\"\noptions = [{text = \"a\", value = \"1\"}, {text = \"b\", value = \"1\"}]",
            ),
            ("pattern on a closed choice", "key = \"X\"\ntype = \"choice\"\npattern = 'x'\noptions = [{text = \"a\", value = \"1\"}]"),
            ("toggle without off", "key = \"X\"\ntype = \"toggle\"\non = \"1\""),
            ("toggle on = off", "key = \"X\"\ntype = \"toggle\"\non = \"1\"\noff = \"1\""),
            ("bad pattern", "key = \"X\"\ntype = \"text\"\npattern = '('"),
            ("unit on text", "key = \"X\"\ntype = \"text\"\nunit = \"s\""),
            ("options on a toggle", "key = \"X\"\ntype = \"toggle\"\non = \"1\"\noff = \"0\"\noptions = []"),
            ("unknown risk", "key = \"X\"\ntype = \"text\"\nrisk = \"high\""),
            ("bad visible_if", "key = \"X\"\ntype = \"text\"\nvisible_if = \"PRO\""),
            ("typo in a field", "key = \"X\"\ntype = \"text\"\nneeds_rebot = true"),
            ("max_len 0", "key = \"X\"\ntype = \"text\"\nmax_len = 0"),
        ] {
            assert!(setting(&format!("{base}{rest}")).is_err(), "{what}");
        }
        assert!(setting("key = \"X\"\ngroup = \"g\"\ntype = \"text\"").is_err(), "no label");
        assert!(setting("key = \"X\"\nlabel = \"x\"\ntype = \"text\"").is_err(), "no group");
    }

    #[test]
    fn risk_decides_whether_to_ask() {
        assert!(Risk::Network.needs_confirm() && Risk::Irreversible.needs_confirm());
        assert!(!Risk::None.needs_confirm() && !Risk::Reboot.needs_confirm());
        let s = ok("key = \"APN\"\nlabel = \"x\"\ngroup = \"g\"\ntype = \"text\"\nrisk = \"network\"\nneeds_reboot = true\nexpert = true");
        assert_eq!(s.risk, Risk::Network);
        assert!(s.needs_reboot && s.expert);
    }
}
