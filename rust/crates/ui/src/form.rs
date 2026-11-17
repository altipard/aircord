//! The settings form's state: what the device reported, and what the person changed on top. Nothing here draws; the
//! guided view turns it into controls and [`aircord_core::transfer::plan`] into commands. What counts as a change, and
//! which settings show, follows the same rules as the plan, so the bar's count is what a transfer sends.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aircord_core::cfg::Cfg;
use aircord_core::device::Template;
use aircord_core::settings::Setting;
use aircord_core::transfer::{self, Change, FieldError, Outcome, Status};

/// A setting against the device's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FieldState {
    Same,
    Changed,
    /// Changed to something that can't be sent, and why, in words for the person in the field.
    Invalid(String),
}

/// What a control did this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    Set(String, String),
    /// An offered value picked from a dropdown: the text field, if it was open, closes again.
    Pick(String, String),
    /// Free text instead of the offered values.
    Custom(String),
    /// Back to the device's value.
    Reset(String),
    /// Empty a clearable secret.
    Clear(String),
}

pub(crate) struct Form {
    pub(crate) template: Arc<Template>,
    pub(crate) cfg: Cfg,
    /// Key to what was entered; a key is here once its control was touched.
    edits: BTreeMap<String, String>,
    /// Keys whose dropdown gave way to a text field.
    custom: BTreeSet<String>,
    /// Secrets to empty: an empty secret field means "leave it", so clearing is said out loud.
    cleared: BTreeSet<String>,
    /// Whether the expert settings are unfolded.
    pub(crate) more: bool,
}

impl Form {
    pub(crate) fn new(template: Arc<Template>, cfg: Cfg) -> Form {
        Form { template, cfg, edits: BTreeMap::new(), custom: BTreeSet::new(), cleared: BTreeSet::new(), more: false }
    }

    /// What the person wants for `key`, if they touched it.
    pub(crate) fn want(&self, key: &str) -> Option<&str> {
        if self.cleared.contains(key) {
            return Some("");
        }
        self.edits.get(key).map(String::as_str)
    }

    /// The form's value for `key`: the wanted one over the device's.
    pub(crate) fn value_of(&self, key: &str) -> Option<&str> {
        self.want(key).or_else(|| self.cfg.get(key))
    }

    pub(crate) fn is_custom(&self, key: &str) -> bool {
        self.custom.contains(key)
    }

    pub(crate) fn is_cleared(&self, key: &str) -> bool {
        self.cleared.contains(key)
    }

    /// Every touched value, as [`transfer::plan`] takes them.
    pub(crate) fn wanted(&self) -> BTreeMap<String, String> {
        let mut all = self.edits.clone();
        all.extend(self.cleared.iter().map(|k| (k.clone(), String::new())));
        all
    }

    /// The settings to show: those the device reported and whose condition holds, lay or expert ones.
    pub(crate) fn shown(&self, expert: bool) -> Vec<&Setting> {
        self.template.settings_for(&self.cfg, true).filter(|s| s.expert == expert && s.visible(|k| self.value_of(k))).collect()
    }

    pub(crate) fn state(&self, s: &Setting) -> FieldState {
        let (Some(have), Some(want)) = (self.cfg.get(&s.key), self.want(&s.key)) else { return FieldState::Same };
        if !s.visible(|k| self.value_of(k)) || s.same(have, want) {
            return FieldState::Same;
        }
        match s.check(want) {
            Ok(_) => FieldState::Changed,
            Err(why) => FieldState::Invalid(why),
        }
    }

    /// How many settings differ from the device, and whether all of them can be sent.
    pub(crate) fn summary(&self) -> (usize, bool) {
        let states: Vec<FieldState> = self.template.settings.iter().map(|s| self.state(s)).filter(|s| *s != FieldState::Same).collect();
        (states.len(), states.iter().all(|s| *s == FieldState::Changed))
    }

    pub(crate) fn apply(&mut self, edit: Edit) {
        match edit {
            Edit::Set(key, value) => {
                self.cleared.remove(&key);
                // An empty secret field is an untouched one, and so is one that holds only blanks: nobody sees them
                // in a masked field, and they must never empty a password. Emptying is `Edit::Clear`.
                if value.trim().is_empty() && self.template.setting(&key).is_some_and(Setting::is_secret) {
                    self.edits.remove(&key);
                } else {
                    self.edits.insert(key, value);
                }
            }
            Edit::Pick(key, value) => {
                self.custom.remove(&key);
                self.apply(Edit::Set(key, value));
            }
            Edit::Custom(key) => {
                let start = self.value_of(&key).unwrap_or_default().to_string();
                self.edits.entry(key.clone()).or_insert(start);
                self.custom.insert(key);
            }
            Edit::Reset(key) => {
                self.edits.remove(&key);
                self.custom.remove(&key);
                self.cleared.remove(&key);
            }
            Edit::Clear(key) => {
                self.edits.remove(&key);
                self.cleared.insert(key);
            }
        }
    }

    pub(crate) fn reset_all(&mut self) {
        self.edits.clear();
        self.custom.clear();
        self.cleared.clear();
    }

    /// The commands a transfer would send.
    pub(crate) fn plan(&self) -> Result<Vec<Change>, Vec<FieldError>> {
        transfer::plan(&self.template, &self.cfg, &self.wanted())
    }

    /// After a transfer: the device's values as read back, and the entries that arrived are no longer changes. What
    /// was refused or stayed open keeps its entry, so it can be corrected and sent again.
    pub(crate) fn absorb(&mut self, outcome: &Outcome) {
        if let Some(cfg) = &outcome.readback {
            self.cfg = cfg.clone();
        }
        for f in outcome.fields.iter().filter(|f| f.status == Status::Verified) {
            self.apply(Edit::Reset(f.key.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use aircord_core::device::Templates;

    use super::*;

    const S31: &str = include_str!("../../core/testdata/s31-nb-v1.4.0-cfg.txt");

    fn form() -> Form {
        let tpl = Templates::load_from(Path::new("/nonexistent")).get("Dragino S31-NB").cloned().unwrap();
        Form::new(Arc::new(tpl), Cfg::parse_text(S31))
    }

    fn set(f: &mut Form, key: &str, value: &str) {
        f.apply(Edit::Set(key.into(), value.into()));
    }

    fn keys(settings: &[&Setting]) -> Vec<String> {
        settings.iter().map(|s| s.key.clone()).collect()
    }

    #[test]
    fn the_lay_form_holds_the_six_lay_settings_and_mqtt_only_with_mqtt() {
        let mut f = form();
        assert_eq!(keys(&f.shown(false)), ["APN", "PRO", "SERVADDR", "TDC", "SHTEMP", "SHHUM"]);
        assert!(f.shown(true).iter().all(|s| s.expert));
        assert!(!keys(&f.shown(true)).contains(&"MQOS".to_string()), "MQTT's expert setting waits for MQTT too");
        set(&mut f, "PRO", "3,5");
        assert!(keys(&f.shown(false)).contains(&"UNAME".to_string()), "the form's value decides, not the device's");
    }

    #[test]
    fn a_change_counts_once_it_differs_and_blocks_while_it_is_wrong() {
        let mut f = form();
        assert_eq!(f.summary(), (0, true));
        set(&mut f, "TDC", "0900");
        assert_eq!(f.summary(), (0, true), "the same value in other digits is no change");
        set(&mut f, "TDC", "3600");
        set(&mut f, "SHTEMP", "zwanzig");
        assert_eq!(f.summary(), (2, false));
        assert!(matches!(f.state(f.template.setting("SHTEMP").unwrap()), FieldState::Invalid(m) if m.starts_with("Die Eingabe")));
        assert!(f.plan().is_err());
        f.apply(Edit::Reset("SHTEMP".into()));
        assert_eq!(f.summary(), (1, true));
        assert_eq!(f.plan().unwrap().iter().map(Change::cmd).collect::<Vec<_>>(), ["AT+TDC=3600"]);
    }

    #[test]
    fn a_hidden_setting_is_not_counted_or_sent() {
        let mut f = form();
        set(&mut f, "PRO", "3,5");
        set(&mut f, "UNAME", "sensor-7");
        assert_eq!(f.summary().0, 2);
        f.apply(Edit::Reset("PRO".into()));
        assert_eq!(f.summary(), (0, true), "back on UDP the MQTT user is neither shown nor sent");
        assert!(f.plan().unwrap().is_empty());
    }

    #[test]
    fn an_empty_secret_is_untouched_and_clearing_is_explicit() {
        let mut f = form();
        f.cfg = Cfg::parse_text(&S31.replace("AT+PRO=2,5", "AT+PRO=3,5").replace("AT+PWD=NULL", "AT+PWD=alt"));
        set(&mut f, "PWD", "neu");
        assert_eq!(f.summary(), (1, true));
        set(&mut f, "PWD", "");
        assert_eq!((f.want("PWD"), f.summary()), (None, (0, true)), "emptied again: unchanged");
        set(&mut f, "PWD", "   ");
        assert_eq!((f.want("PWD"), f.summary()), (None, (0, true)), "blanks alone are no entry, and clear nothing");
        assert!(f.plan().unwrap().is_empty());
        f.apply(Edit::Clear("PWD".into()));
        assert!(f.is_cleared("PWD"));
        assert_eq!(f.plan().unwrap().iter().map(Change::shown_cmd).collect::<Vec<_>>(), ["AT+PWD=******"]);
        assert_eq!(f.plan().unwrap()[0].cmd(), "AT+PWD=NULL");
        set(&mut f, "PWD", "x");
        assert!(!f.is_cleared("PWD"), "typing takes the clearing back");
    }

    #[test]
    fn free_text_starts_from_the_value_and_a_pick_closes_it() {
        let mut f = form();
        f.apply(Edit::Custom("TDC".into()));
        assert!(f.is_custom("TDC"));
        assert_eq!((f.want("TDC"), f.summary()), (Some("900"), (0, true)));
        f.apply(Edit::Pick("TDC".into(), "3600".into()));
        assert!(!f.is_custom("TDC"));
        assert_eq!(f.value_of("TDC"), Some("3600"));
        f.reset_all();
        assert_eq!(f.value_of("TDC"), Some("900"));
    }
}
