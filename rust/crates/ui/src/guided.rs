//! The guided view, for people who do not know AT commands: on one screen the devices as cards on the left, and for
//! the connected one its status and its settings as a form, with one bar at the bottom that transfers what was
//! changed. It is German, as its users are; the terminal's own notes stay as they are in the expert view.
//!
//! After a connect the settings are read by themselves (`AT+CFG`), the template is chosen by the model in them, and
//! the status card fills from the lines the device prints while it starts ([`aircord_core::banner`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use aircord_core::banner;
use aircord_core::cfg::Cfg;
use aircord_core::lines::NoLine;
use aircord_core::link::{Device, Link};
use aircord_core::settings::{Field, Setting, Unit, duration_text};
use aircord_core::transfer;
use egui::{
    Align, Align2, CentralPanel, Color32, FontId, Frame, Id, Key, Label, Layout, Margin, Panel, Rect, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, TextEdit, Ui, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::form::{Edit, FieldState, Form};
use crate::icons::Icon;
use crate::inbox::Msg;
use crate::style::{self, RADIUS_SM, pal};
use crate::widgets::{self, Alert, Mark, Tone, button, button_named, icon_button};
use crate::{App, Conn, View};

/// How often the settings are asked for again once the password was accepted: a node that just took its password is
/// busy starting its modem for some twenty seconds and may not answer the first time.
const BOOT_READS: usize = 4;

/// The dropdown line that turns it into a text field.
const OWN_VALUE: &str = "Eigener Wert…";

/// What the brief's hint says when the node has been without network for minutes.
pub(crate) const NO_NETWORK_HINT: &str = "Kein Netz – Antenne angeschraubt? Gerät näher ans Fenster?";

/// Where reading the connected device's settings stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Read {
    #[default]
    Idle,
    Reading,
    /// Why not, in German, from [`transfer::read_cfg`].
    Failed(String),
    Ready,
}

/// The guided view's state for the connected device.
#[derive(Default)]
pub(crate) struct Guided {
    /// What the device's own start lines said.
    pub(crate) status: banner::Status,
    /// Since when (egui's time) the device reports no network.
    no_net_since: Option<f64>,
    pub(crate) read: Read,
    /// The settings form: there once the settings were read and a template with settings fits the model.
    pub(crate) form: Option<Form>,
    pub(crate) model: Option<String>,
    pub(crate) firmware: Option<String>,
    /// This view's own alert line, in German.
    pub(crate) notice: Alert,
    /// A password was sent by hand and the settings still did not come.
    pub(crate) password_tried: bool,
}

/// One line of the status card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StatusRow {
    pub(crate) mark: Mark,
    pub(crate) text: String,
}

/// The status card's lines from what the device said while starting. Values from an upload (battery, temperature,
/// humidity, the last upload's time) are further rows of the same kind: add them here.
pub(crate) fn status_rows(s: &banner::Status) -> Vec<StatusRow> {
    let row = |mark, text: String| StatusRow { mark, text };
    if *s == banner::Status::default() {
        return vec![row(Mark::Idle, "Noch keine Statusmeldung vom Gerät".into())];
    }
    let mut rows = Vec::new();
    if s.password_ok {
        rows.push(row(Mark::Ok, "Passwort angenommen".into()));
    }
    if s.imsi.is_some() {
        rows.push(row(Mark::Ok, "SIM-Karte erkannt".into()));
    }
    let band = s.band.as_ref().map(|b| format!(" · Band {b}")).unwrap_or_default();
    rows.push(match s.signal {
        Some(sig) if sig.has_network() => row(Mark::Ok, format!("Netz {}{band}", sig.text())),
        Some(_) => row(Mark::Warn, format!("Kein Netz{band}")),
        None => row(Mark::Idle, format!("Netz: noch keine Meldung{band}")),
    });
    if s.apn_set {
        rows.push(row(Mark::Ok, "Zugangspunkt gesetzt".into()));
    }
    rows
}

/// Waits for the device to say it took the password.
fn password_accepted(tap: &aircord_core::lines::Tap, wait: Duration, stop: &AtomicBool) -> bool {
    let end = Instant::now() + wait;
    let mut seen = banner::Status::default();
    while !stop.load(Ordering::Relaxed) {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match tap.recv(left.min(Duration::from_millis(100))) {
            Ok(line) => {
                seen.feed(&line);
                if seen.password_ok {
                    return true;
                }
            }
            Err(NoLine::Timeout) => {}
            Err(NoLine::Closed) => break,
        }
    }
    false
}

/// Sends the password, if there is one, and reads the settings. Waits: a job's thread calls it. The password goes out
/// even when `stop` is already set: stopping ends the reading (someone else needs the link), not the unlocking the
/// person asked for by typing a password.
pub(crate) fn read_settings(link: &dyn Link, pin: Option<&str>, wait: Duration, stop: &AtomicBool) -> Result<Cfg, String> {
    let mut tries = 1;
    if let Some(pin) = pin {
        let tap = link.lines();
        link.send_secret(pin);
        if password_accepted(&tap, wait, stop) {
            tries = BOOT_READS;
        }
    }
    let mut result = Err("Keine Verbindung zum Gerät.".to_string());
    for _ in 0..tries {
        if stop.load(Ordering::Relaxed) || !link.is_connected() {
            break;
        }
        result = transfer::read_cfg(link, wait, stop);
        if result.is_ok() {
            break;
        }
    }
    result
}

/// Why the settings did not come, as far as the form needs to tell the cases apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The device stayed silent or stopped mid-dump: what a device does that waits for its password.
    Timeout,
    Refused,
    /// It answered, but with nothing the form can use.
    Unusable,
    Lost,
    Stopped,
    Other,
}

impl Failure {
    /// Sorts a reason as `transfer::read_cfg` words it. The reason itself names commands and stays in the tooltip
    /// and the terminal.
    pub(crate) fn of(why: &str) -> Failure {
        if why.contains("Keine vollständige Antwort") {
            Failure::Timeout
        } else if why.contains("abgelehnt") {
            Failure::Refused
        } else if why.contains("keine Einstellungen") || why.contains("kein Gerätemodell") {
            Failure::Unusable
        } else if why.contains("Verbindung") {
            Failure::Lost
        } else if why.contains("Abgebrochen") {
            Failure::Stopped
        } else {
            Failure::Other
        }
    }

    /// The sentence for the person in front of the device.
    pub(crate) fn text(self) -> &'static str {
        match self {
            Failure::Timeout => "Das Gerät hat nicht vollständig geantwortet. Bitte erneut versuchen.",
            Failure::Refused => "Das Gerät hat die Abfrage abgelehnt. Bitte erneut versuchen.",
            Failure::Unusable => "Das Gerät hat keine verwertbaren Einstellungen gemeldet.",
            Failure::Lost => "Die Verbindung zum Gerät ist abgebrochen. Bitte neu verbinden.",
            Failure::Stopped => "Das Lesen wurde abgebrochen.",
            Failure::Other => "Die Einstellungen konnten nicht gelesen werden.",
        }
    }
}

/// A scan's signal in bars: at least one for a device that was heard at all.
fn signal_level(rssi: i16) -> u8 {
    match rssi {
        -60..=i16::MAX => 4,
        -72..=-61 => 3,
        -84..=-73 => 2,
        _ => 1,
    }
}

fn signal_word(level: u8) -> &'static str {
    match level {
        4 => "Signal sehr gut",
        3 => "Signal gut",
        2 => "Signal mittel",
        _ => "Signal schwach",
    }
}

fn unit_word(unit: Unit) -> &'static str {
    match unit {
        Unit::Ms => "Millisekunden",
        Unit::S => "Sekunden",
        Unit::Min => "Minuten",
        Unit::H => "Stunden",
    }
}

fn small(text: impl Into<String>, ink: Color32) -> Label {
    Label::new(RichText::new(text).size(11.5).color(ink)).wrap()
}

/// The settings in groups, in the order the template first names each group.
fn grouped<'a>(settings: &[&'a Setting]) -> Vec<(&'a str, Vec<&'a Setting>)> {
    let mut groups: Vec<(&str, Vec<&Setting>)> = Vec::new();
    for s in settings {
        match groups.iter_mut().find(|(g, _)| *g == s.group) {
            Some((_, list)) => list.push(s),
            None => groups.push((&s.group, vec![s])),
        }
    }
    groups
}

/// A text field for a setting's value; its edit goes to `edits`.
fn text_field(ui: &mut Ui, s: &Setting, value: &str, unit: Option<&str>, width: f32, password: bool, edits: &mut Vec<Edit>) {
    let mut text = value.to_string();
    let hint = if password { "leer = unverändert" } else { "" };
    let edit = TextEdit::singleline(&mut text).id(Id::new(("setting", &s.key))).password(password).hint_text(hint);
    let r = widgets::field(ui, edit, None, unit, width);
    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, format!("{}: Eingabe", s.label)));
    if r.changed() {
        edits.push(Edit::Set(s.key.clone(), text));
    }
}

/// The control for one setting, by its type.
fn control(ui: &mut Ui, form: &Form, s: &Setting, width: f32, edits: &mut Vec<Edit>) {
    let key = &s.key;
    let have = form.cfg.get(key).unwrap_or_default();
    let value = form.value_of(key).unwrap_or_default().to_string();
    if s.is_secret() {
        if form.is_cleared(key) {
            ui.add_sized([width, widgets::CONTROL_H], Label::new(RichText::new("Wird gelöscht").color(pal().warn)));
            return;
        }
        let clear = s.clearable && !have.is_empty();
        let field_w = if clear { width - 86.0 } else { width };
        text_field(ui, s, form.want(key).unwrap_or_default(), None, field_w, true, edits);
        if clear && button_named(ui, None, "Löschen", &format!("Löschen: {}", s.label), Tone::Danger).clicked() {
            edits.push(Edit::Clear(key.clone()));
        }
        return;
    }
    let own = form.is_custom(key);
    match &s.field {
        Field::Duration { unit, presets, .. } if !own => {
            let mut offered = presets.clone();
            if let Ok(n) = have.trim().parse::<i64>()
                && !offered.contains(&n)
            {
                offered.push(n);
            }
            offered.sort_unstable();
            widgets::dropdown(ui, &s.label, &s.shown(&value), width, |ui| {
                for n in offered {
                    let v = n.to_string();
                    if widgets::dropdown_item(ui, &duration_text(n, *unit), s.same(&value, &v)) {
                        edits.push(Edit::Pick(key.clone(), v));
                    }
                }
                if widgets::dropdown_item(ui, OWN_VALUE, false) {
                    edits.push(Edit::Custom(key.clone()));
                }
            });
        }
        Field::Choice { options, custom, .. } if !own => {
            widgets::dropdown(ui, &s.label, &s.shown(&value), width, |ui| {
                for o in options {
                    if widgets::dropdown_item(ui, &o.text, o.value == value.trim()) {
                        edits.push(Edit::Pick(key.clone(), o.value.clone()));
                    }
                }
                if *custom && widgets::dropdown_item(ui, OWN_VALUE, false) {
                    edits.push(Edit::Custom(key.clone()));
                }
            });
        }
        Field::Toggle { on, off } => {
            let is_on = value.trim() == on;
            if widgets::switch(ui, &s.label, is_on).clicked() {
                edits.push(Edit::Set(key.clone(), if is_on { off.clone() } else { on.clone() }));
            }
            // A device value that is neither position is shown, not silently read as "off".
            let text = if is_on || value.trim() == off { s.shown(&value) } else { format!("Aus (Gerät meldet {value})") };
            ui.label(RichText::new(text).color(pal().muted));
        }
        Field::Duration { unit, .. } => text_field(ui, s, &value, Some(unit_word(*unit)), width, false, edits),
        Field::Number { unit, .. } => text_field(ui, s, &value, Some(unit.as_str()).filter(|u| !u.is_empty()), width, false, edits),
        _ => text_field(ui, s, &value, None, width, false, edits),
    }
}

/// One setting: its name with a dot once it differs from the device, its control with the way back beside it, and
/// under it what is wrong with the input, or what to enter.
fn field_ui(ui: &mut Ui, form: &Form, s: &Setting, edits: &mut Vec<Edit>) {
    let state = form.state(s);
    let width = ui.available_width();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if state != FieldState::Same {
            let (r, dot) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
            dot.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, format!("geändert: {}", s.label)));
            ui.painter().circle_filled(r.center(), 3.5, pal().accent);
        }
        let label = ui.add(Label::new(RichText::new(&s.label).font(style::medium(12.5)).color(pal().text)).wrap());
        if !s.help.is_empty() {
            label.on_hover_text(&s.help);
        }
    });
    let touched = state != FieldState::Same || form.is_custom(&s.key) || form.is_cleared(&s.key);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        control(ui, form, s, width - 34.0, edits);
        if touched && icon_button(ui, Icon::Undo, &format!("Zurücksetzen: {}", s.label)).clicked() {
            edits.push(Edit::Reset(s.key.clone()));
        }
    });
    let typed = s.is_secret() || form.is_custom(&s.key) || matches!(s.field, Field::Text { .. } | Field::Number { .. });
    match state {
        FieldState::Invalid(why) => {
            ui.add(small(why, pal().error));
        }
        _ if typed && matches!(s.field, Field::Duration { .. }) => {
            ui.add(small(format!("entspricht {}", s.shown(form.value_of(&s.key).unwrap_or_default())), pal().muted));
        }
        _ if typed && !s.help.is_empty() => {
            ui.add(small(&s.help, pal().muted));
        }
        _ => {}
    }
}

fn group_card(ui: &mut Ui, form: &Form, name: &str, settings: &[&Setting], edits: &mut Vec<Edit>) {
    widgets::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        // A column is laid out justified, which would spread the words of a wrapped help text.
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            widgets::section(ui, name);
            for s in settings {
                ui.add_space(4.0);
                field_ui(ui, form, s, edits);
            }
        });
    });
    ui.add_space(2.0);
}

/// The groups as cards, side by side in two columns when there is room.
fn group_cards(ui: &mut Ui, form: &Form, settings: &[&Setting], edits: &mut Vec<Edit>) {
    let groups = grouped(settings);
    if ui.available_width() < 560.0 || groups.len() < 2 {
        for (name, list) in &groups {
            group_card(ui, form, name, list, edits);
        }
        return;
    }
    // Each group goes to the shorter column, so the two end about level.
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let (mut left_h, mut right_h) = (0, 0);
    for g in &groups {
        if left_h <= right_h {
            left_h += g.1.len() + 1;
            left.push(g);
        } else {
            right_h += g.1.len() + 1;
            right.push(g);
        }
    }
    ui.columns(2, |cols| {
        for (col, side) in [left, right].into_iter().enumerate() {
            for (name, list) in side {
                group_card(&mut cols[col], form, name, list, edits);
            }
        }
    });
}

/// A title and a sentence in the middle of an empty area.
fn empty_state(ui: &mut Ui, title: &str, text: &str) {
    ui.add_space((ui.available_height() * 0.28).min(120.0));
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).font(style::semibold(15.0)).color(pal().text));
        ui.add_space(4.0);
        ui.label(RichText::new(text).color(pal().muted));
    });
}

impl App {
    /// A fresh device: nothing known about it yet.
    pub(crate) fn guided_reset(&mut self, read: Read) {
        let notice = std::mem::take(&mut self.guided.notice);
        self.guided = Guided { read, notice, ..Guided::default() };
    }

    /// A reply line, for the status card.
    pub(crate) fn feed_status(&mut self, line: &str) {
        if !self.guided.status.feed(line) {
            return;
        }
        match self.guided.status.signal {
            Some(s) if !s.has_network() => {
                self.guided.no_net_since.get_or_insert(self.now);
            }
            _ => self.guided.no_net_since = None,
        }
    }

    /// Whether the device has been without network long enough to say what usually helps.
    pub(crate) fn no_network_for_long(&self) -> bool {
        self.guided.no_net_since.is_some_and(|since| self.now - since >= self.timing.no_network_hint.as_secs_f64())
    }

    /// When the no-network hint is due, for the frame that shows it.
    pub(crate) fn no_network_due_in(&self) -> Option<f64> {
        let since = self.guided.no_net_since?;
        let left = since + self.timing.no_network_hint.as_secs_f64() - self.now;
        (left > 0.0).then_some(left)
    }

    /// Whether to ask for the device's password: its settings did not come and it never said the password was right.
    /// A device does not announce that it wants one, so this is the one place that decides.
    pub(crate) fn wants_password(&self) -> bool {
        matches!(self.guided.read, Read::Failed(_)) && !self.guided.status.password_ok
    }

    /// The settings arrived, or did not.
    pub(crate) fn on_cfg(&mut self, epoch: u64, result: Result<Cfg, String>) {
        let Conn::Connected { name, .. } = &self.conn else { return };
        if epoch != self.epoch {
            return;
        }
        let name = name.clone();
        let mut cfg = match result {
            Ok(cfg) => cfg,
            Err(why) => {
                self.log_line(format!("-- settings not read: {why} --"));
                self.guided.read = Read::Failed(why);
                return;
            }
        };
        // The model beats the advertised name, which for an NB node is its IMEI.
        if let Some(model) = cfg.model_line().map(str::to_string) {
            self.model_known(&model);
        }
        let template = self.stores.templates.select(cfg.model_line(), &name).filter(|t| !t.settings.is_empty()).cloned();
        let what = cfg.model_line().unwrap_or_default().to_string();
        let with = template.as_ref().map_or("no template with settings".to_string(), |t| format!("template {}", t.name));
        self.log_line(format!("-- settings read: {what} — {with} --"));
        self.guided.model = cfg.model.clone();
        self.guided.firmware = cfg.firmware.clone();
        self.guided.read = Read::Ready;
        self.guided.password_tried = false;
        self.guided.form = match (template, self.guided.form.take()) {
            (Some(t), Some(mut form)) if form.template.name == t.name => {
                t.protect(&mut cfg);
                form.cfg = cfg;
                Some(form)
            }
            (Some(t), _) => {
                t.protect(&mut cfg);
                Some(Form::new(Arc::new(t), cfg))
            }
            (None, _) => None,
        };
    }

    /// Reads the settings again, after `pin` if one is given.
    fn spawn_read(&mut self, pin: Option<String>) {
        if !self.connected() || self.guided.read == Read::Reading || self.transfer_running() {
            return;
        }
        if self.running.is_some() || self.ota_running() {
            self.guided
                .notice
                .warn("Im Experten-Reiter läuft gerade ein Playbook oder ein Firmware-Update. Bitte warten, bis es fertig ist.");
            return;
        }
        self.guided.read = Read::Reading;
        self.epoch += 1;
        self.read_stop = Arc::new(AtomicBool::new(false));
        let (link, out, wait, epoch, stop) =
            (Arc::clone(&self.link), self.outbox(), self.timing.read_wait, self.epoch, Arc::clone(&self.read_stop));
        std::thread::spawn(move || {
            let result = read_settings(&*link, pin.as_deref(), wait, &stop);
            out.post(Msg::Cfg { epoch, result });
        });
    }

    /// Ends a read that is under way, before something else takes the link: a command typed by hand, whose reply the
    /// read would take for the dump's; a playbook, whose step would take the dump's `OK` for its own; a firmware
    /// update, whose bootloader must not be sent `AT+CFG`. Its late result is dropped.
    pub(crate) fn interrupt_read(&mut self) {
        if self.guided.read == Read::Reading {
            self.read_stop.store(true, Ordering::Relaxed);
            self.epoch += 1;
            self.guided.read = Read::Idle;
            self.log_line("-- reading the settings stopped: the link is needed for something else --");
        }
    }

    /// The model line of a dump, as it arrives: the template is switched at once, so its secrets further down the
    /// dump are masked before they reach the terminal.
    pub(crate) fn on_model_line(&mut self, reply: &str) {
        let Some(model) = reply.trim().strip_prefix("AT+MODEL=") else { return };
        if self.connected() {
            self.model_known(model.trim());
        }
    }

    pub(crate) fn read_again(&mut self) {
        self.guided.notice.clear();
        self.spawn_read(None);
    }

    /// Sends the password typed into the guided view and reads the settings.
    pub(crate) fn login(&mut self) {
        let pin = self.pin.trim().to_string();
        if pin.is_empty() {
            self.guided.notice.warn("Bitte das Passwort eingeben. Es steht auf dem Etikett des Geräts.");
            return;
        }
        self.guided.notice.clear();
        self.guided.password_tried = true;
        self.spawn_read(Some(pin));
    }

    // ---------- drawing ----------

    pub(crate) fn guided_view(&mut self, ui: &mut Ui) {
        let dock = |m: Margin| Frame::new().fill(pal().dock).inner_margin(m);
        Panel::left("guided-devices")
            .frame(dock(Margin { left: 8, right: 4, top: 8, bottom: 8 }))
            .show_separator_line(false)
            .resizable(false)
            .exact_size(248.0)
            .show(ui, |ui| {
                widgets::card().show(ui, |ui| self.device_cards(ui));
            });
        if self.connected() && self.guided.form.is_some() {
            Panel::bottom("guided-bar")
                .frame(widgets::chrome(Margin::symmetric(12, 8)))
                .show_separator_line(true)
                .show(ui, |ui| self.transfer_bar(ui));
        }
        CentralPanel::default().frame(dock(Margin { left: 4, right: 8, top: 8, bottom: 8 })).show(ui, |ui| self.guided_main(ui));
    }

    /// The devices as cards: the name, the signal as bars, the address only in the tooltip.
    fn device_cards(&mut self, ui: &mut Ui) {
        ui.set_min_size(ui.available_size());
        widgets::section(ui, "Geräte");
        ui.add_space(2.0);
        Panel::bottom("guided-connect")
            .frame(Frame::new().inner_margin(Margin { top: 8, ..Margin::ZERO }))
            .show_separator_line(true)
            .show(ui, |ui| self.guided_connect(ui));
        if self.display.is_empty() {
            ui.add_space(28.0);
            ui.vertical_centered(|ui| {
                let (title, text) = if self.link.is_scanning() {
                    ("Suche läuft …", "Das Gerät muss eingeschaltet\nund in der Nähe sein.")
                } else {
                    ("Noch keine Geräte", "Auf „Suchen“ drücken.\nDas Gerät muss eingeschaltet\nund in der Nähe sein.")
                };
                ui.label(RichText::new(title).font(style::semibold(13.0)).color(pal().text));
                ui.add_space(4.0);
                ui.label(RichText::new(text).color(pal().muted));
            });
            return;
        }
        let rows = self.display.clone();
        let selected = self.selected.as_ref().map(|s| s.addr.clone());
        let linked = match &self.conn {
            Conn::Connected { addr, .. } => Some(addr.clone()),
            _ => None,
        };
        // Another device is picked only while none is connected: the form belongs to the connected one.
        let pickable = self.conn == Conn::Idle;
        let mut clicked: Option<Device> = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for d in &rows {
                let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
                let name = if d.name.is_empty() { "Unbenanntes Gerät" } else { d.name.as_str() };
                let is_sel = selected.as_deref() == Some(d.addr.as_str());
                let is_linked = linked.as_deref() == Some(d.addr.as_str());
                r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, pickable, is_sel, name));
                let (fill, stroke) = if is_sel {
                    (pal().select_fill, Stroke::new(1.0, pal().accent_border))
                } else if r.hovered() && pickable {
                    (pal().hover_fill, Stroke::new(1.0, pal().field_border))
                } else {
                    (pal().field, Stroke::new(1.0, pal().field_border))
                };
                let p = ui.painter();
                p.rect(rect, RADIUS_SM, fill, stroke, StrokeKind::Inside);
                if r.has_focus() {
                    p.rect_stroke(rect, RADIUS_SM, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
                }
                let level = signal_level(d.rssi);
                let text_w = rect.width() - 20.0 - 30.0;
                let upper = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 28.0));
                let ink = if d.name.is_empty() { pal().muted } else { pal().text };
                crate::clip_text(p, upper, rect.left() + 10.0, text_w, name, style::medium(12.5), ink);
                let (word, word_ink) = if is_linked { ("Verbunden", pal().ok) } else { (signal_word(level), pal().muted) };
                p.text(pos2(rect.left() + 10.0, rect.bottom() - 14.0), Align2::LEFT_CENTER, word, FontId::proportional(11.0), word_ink);
                widgets::signal_bars(p, Rect::from_min_size(pos2(rect.right() - 28.0, rect.center().y - 7.0), vec2(18.0, 13.0)), level);
                if r.on_hover_text(format!("Adresse: {}", d.addr)).clicked() && pickable {
                    clicked = Some(d.clone());
                }
            }
        });
        if let Some(d) = clicked {
            self.guided.notice.clear();
            self.select(&d);
        }
    }

    /// Under the cards: search, and connect to the selected device or let go of the connected one.
    fn guided_connect(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let scanning = self.link.is_scanning();
            let (icon, label, tip) =
                if scanning { (Icon::Stop, "Stopp", "Suche beenden") } else { (Icon::Scan, "Suchen", "Geräte in der Nähe suchen") };
            let idle = self.conn == Conn::Idle;
            let tone = if self.selected.is_some() || !idle { Tone::Plain } else { Tone::Primary };
            if ui.add_enabled_ui(idle, |ui| button(ui, Some(icon), label, tone)).inner.on_hover_text(tip).clicked() {
                self.toggle_scan();
            }
            if matches!(self.conn, Conn::Connected { .. } | Conn::Bootloader { .. }) {
                if button(ui, None, "Trennen", Tone::Plain).on_hover_text("Verbindung zum Gerät beenden").clicked() {
                    self.disconnect();
                }
            } else {
                let can = self.selected.is_some() && idle && !self.ota_running();
                let tip =
                    self.selected.as_ref().map_or("Zuerst ein Gerät auswählen".to_string(), |s| format!("Mit {} verbinden", s.name));
                if ui.add_enabled_ui(can, |ui| button(ui, None, "Verbinden", Tone::Primary)).inner.on_hover_text(tip).clicked() {
                    self.guided.notice.clear();
                    self.connect();
                }
            }
        });
    }

    fn guided_main(&mut self, ui: &mut Ui) {
        self.guided.notice.show(ui);
        match self.conn.clone() {
            Conn::Bootloader { .. } => {
                empty_state(ui, "Die Firmware wird aktualisiert", "Bitte warten und das Gerät nicht ausschalten.");
            }
            Conn::Connecting => empty_state(ui, "Verbindung wird aufgebaut …", "Das dauert ein paar Sekunden."),
            Conn::Idle => {
                let (title, text) = match (&self.selected, self.display.is_empty()) {
                    (Some(s), _) => (format!("{} ist ausgewählt", s.name), "Links auf „Verbinden“ drücken."),
                    (None, true) => ("Gerät einstellen".to_string(), "Links auf „Suchen“ drücken und das Gerät auswählen."),
                    (None, false) => ("Gerät einstellen".to_string(), "Links ein Gerät auswählen und auf „Verbinden“ drücken."),
                };
                empty_state(ui, &title, text);
            }
            Conn::Connected { name, .. } => self.guided_device(ui, &name),
        }
    }

    /// The connected device: its head line, its status and what its settings allow.
    fn guided_device(&mut self, ui: &mut Ui, name: &str) {
        ui.horizontal(|ui| {
            ui.set_min_height(28.0);
            widgets::dot(ui, pal().ok);
            let title = self.guided.model.clone().unwrap_or_else(|| name.to_string());
            ui.label(RichText::new(title).font(style::semibold(15.0)).color(pal().text));
            if let Some(v) = &self.guided.firmware {
                ui.label(RichText::new(format!("v{v}")).color(pal().muted));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let free = self.guided.read != Read::Reading && !self.transfer_running();
                if ui.add_enabled_ui(free, |ui| icon_button(ui, Icon::Reload, "Einstellungen neu lesen")).inner.clicked() {
                    self.read_again();
                }
            });
        });
        ui.add_space(2.0);
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            self.status_card(ui);
            ui.add_space(2.0);
            match self.guided.read.clone() {
                Read::Idle => {
                    widgets::card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        widgets::section(ui, "Einstellungen");
                        ui.add_space(4.0);
                        ui.add(Label::new(RichText::new("Die Einstellungen wurden noch nicht gelesen.").color(pal().text)).wrap());
                        ui.add_space(6.0);
                        if button(ui, Some(Icon::Reload), "Jetzt lesen", Tone::Primary).clicked() {
                            self.read_again();
                        }
                    });
                }
                Read::Reading => {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.add_space(4.0);
                        ui.label(RichText::new("Die Einstellungen werden gelesen …").color(pal().muted));
                    });
                }
                Read::Failed(why) => self.read_failed_card(ui, &why),
                Read::Ready if self.guided.form.is_none() => self.no_template_card(ui),
                Read::Ready => self.form_cards(ui),
            }
        });
    }

    fn status_card(&mut self, ui: &mut Ui) {
        let rows = status_rows(&self.guided.status);
        let hint = self.no_network_for_long();
        if let Some(left) = self.no_network_due_in() {
            ui.ctx().request_repaint_after(Duration::from_secs_f64(left));
        }
        widgets::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section(ui, "Status");
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                let font = egui::TextStyle::Body.resolve(ui.style());
                for row in &rows {
                    // A mark and its text wrap together: the pair is one item of the wrapping row.
                    let text_w = ui.painter().layout_no_wrap(row.text.clone(), font.clone(), pal().text).size().x;
                    ui.allocate_ui_with_layout(vec2(16.0 + 4.0 + text_w + 14.0, 18.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        widgets::mark(ui, row.mark);
                        ui.label(RichText::new(&row.text).color(if row.mark == Mark::Idle { pal().muted } else { pal().text }));
                    });
                }
            });
            if hint {
                ui.add_space(2.0);
                let mut strip = Alert::default();
                strip.warn(NO_NETWORK_HINT);
                strip.show(ui);
            }
        });
    }

    /// The settings did not come: ask for the password when that is the likely reason, and always offer to read again
    /// or to go on in the expert view.
    fn read_failed_card(&mut self, ui: &mut Ui, why: &str) {
        let ask = self.wants_password();
        widgets::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section(ui, if ask { "Passwort" } else { "Einstellungen" });
            ui.add_space(4.0);
            if ask {
                let text = if self.guided.password_tried {
                    "Das Passwort wurde nicht angenommen. Bitte prüfen und noch einmal eingeben."
                } else {
                    "Das Gerät gibt seine Einstellungen erst nach dem Passwort heraus. Es steht auf dem Etikett des Geräts."
                };
                ui.add(Label::new(RichText::new(text).color(pal().text)).wrap());
                ui.add_space(4.0);
                let mut go = false;
                ui.horizontal(|ui| {
                    // Masked: this window gets screen-shared and photographed in the field.
                    let edit = TextEdit::singleline(&mut self.pin).id(Id::new("guided-password")).password(true).hint_text("Passwort");
                    let r = widgets::field(ui, edit, None, None, 200.0);
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, "Passwort: Eingabe"));
                    go = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    go |= button(ui, None, "Anmelden", Tone::Primary).clicked();
                });
                if go {
                    self.login();
                }
                ui.add_space(2.0);
                ui.add(small(Failure::of(why).text(), pal().muted)).on_hover_text(why);
            } else {
                // The reason as the device layer words it names commands: that is for the tooltip and the terminal.
                ui.add(Label::new(RichText::new(Failure::of(why).text()).color(pal().text)).wrap()).on_hover_text(why);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if button(ui, Some(Icon::Reload), "Erneut lesen", Tone::Plain).clicked() {
                    self.read_again();
                }
                if button(ui, None, "Experten-Ansicht öffnen", Tone::Plain).clicked() {
                    self.set_view(View::Expert);
                }
            });
        });
    }

    fn no_template_card(&mut self, ui: &mut Ui) {
        let model = self.guided.model.clone().unwrap_or_else(|| "unbekannt".into());
        widgets::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section(ui, "Einstellungen");
            ui.add_space(4.0);
            let text = format!(
                "Für das Modell {model} gibt es hier noch keine geführten Einstellungen. \
                 In der Experten-Ansicht lässt sich das Gerät wie bisher über Befehle einstellen."
            );
            ui.add(Label::new(RichText::new(text).color(pal().text)).wrap());
            ui.add_space(6.0);
            if button(ui, None, "Experten-Ansicht öffnen", Tone::Plain).clicked() {
                self.set_view(View::Expert);
            }
        });
    }

    /// The form: a card for each group of the template, the expert settings folded away under them.
    fn form_cards(&mut self, ui: &mut Ui) {
        let Some(form) = &self.guided.form else { return };
        let mut edits = Vec::new();
        let mut fold = false;
        ui.add_enabled_ui(!self.transfer_running(), |ui| {
            group_cards(ui, form, &form.shown(false), &mut edits);
            let more = form.shown(true);
            if !more.is_empty() {
                ui.add_space(4.0);
                fold = widgets::tab(ui, "Weitere Einstellungen", form.more)
                    .on_hover_text("Einstellungen für besondere Fälle. Im Normalfall bleiben sie, wie sie sind.")
                    .clicked();
                if form.more {
                    ui.add_space(4.0);
                    group_cards(ui, form, &more, &mut edits);
                }
            }
        });
        if let Some(form) = &mut self.guided.form {
            if fold {
                form.more = !form.more;
            }
            for e in edits {
                form.apply(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use aircord_core::banner::{Signal, Status};

    use super::*;

    const BOOT: &str = include_str!("../../core/testdata/s31-nb-v1.4.0-boot.txt");

    #[test]
    fn a_read_s_failure_is_said_without_commands() {
        let cases = [
            ("Keine vollständige Antwort auf AT+CFG.", Failure::Timeout),
            ("Das Gerät hat AT+CFG abgelehnt (AT_ERROR).", Failure::Refused),
            ("AT+CFG hat keine Einstellungen geliefert.", Failure::Unusable),
            ("AT+CFG nennt kein Gerätemodell (AT+MODEL fehlt).", Failure::Unusable),
            ("Verbindung verloren.", Failure::Lost),
            ("Keine Verbindung zum Gerät.", Failure::Lost),
            ("Abgebrochen.", Failure::Stopped),
            ("etwas Neues", Failure::Other),
        ];
        for (why, kind) in cases {
            assert_eq!(Failure::of(why), kind, "{why}");
            assert!(!kind.text().contains("AT"), "{}", kind.text());
        }
    }

    fn texts(s: &Status) -> Vec<(Mark, String)> {
        status_rows(s).into_iter().map(|r| (r.mark, r.text)).collect()
    }

    #[test]
    fn the_status_card_reads_the_real_boot_lines() {
        let mut s = Status::default();
        assert_eq!(texts(&s), [(Mark::Idle, "Noch keine Statusmeldung vom Gerät".to_string())]);
        s = Status::from_lines(BOOT.lines());
        assert_eq!(
            texts(&s),
            [
                (Mark::Ok, "Passwort angenommen".to_string()),
                (Mark::Ok, "SIM-Karte erkannt".to_string()),
                (Mark::Warn, "Kein Netz · Band 8,20".to_string()),
                (Mark::Ok, "Zugangspunkt gesetzt".to_string()),
            ]
        );
        s.signal = Some(Signal(17));
        assert_eq!(texts(&s)[2], (Mark::Ok, "Netz gut · Band 8,20".to_string()));
        s.signal = None;
        assert_eq!(texts(&s)[2].0, Mark::Idle);
    }

    #[test]
    fn groups_keep_the_template_s_order_and_bars_follow_the_signal() {
        let tpl = aircord_core::device::Templates::load_from(std::path::Path::new("/nonexistent")).get("Dragino S31-NB").cloned().unwrap();
        let lay: Vec<&Setting> = tpl.settings.iter().filter(|s| !s.expert).collect();
        let names: Vec<&str> = grouped(&lay).into_iter().map(|(g, _)| g).collect();
        assert_eq!(names, ["Netz", "Server", "MQTT-Zugang", "Messen & Senden", "Alarme"]);
        assert_eq!([signal_level(-40), signal_level(-65), signal_level(-80), signal_level(-99)], [4, 3, 2, 1]);
    }
}
