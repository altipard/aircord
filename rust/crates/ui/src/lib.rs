//! `aircord-ui`: the desktop window, in egui, in PhotoCraft's look. A thin shell: what the app does lives in
//! `aircord-core`; this draws it, and runs the long jobs (connect, a playbook, a firmware update) on threads of their own
//! that report through [`inbox::Outbox`].
//!
//! The layout, PhotoCraft's frame: a title bar with the light/dark switch at its right; an options bar with Scan and
//! a hint for what to do next; a narrow icon rail on the left for the dialogs; then, on the dock, the device list card
//! with the PIN and Connect under it, the terminal and the command line in the middle, the device's control panel card
//! on the right; the status bar at the bottom. Dialogs are modal: sessions, playbooks, firmware, and the confirmations.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use aircord_core::device::Templates;
use aircord_core::history::History;
use aircord_core::link::{Device, Link};
use aircord_core::playbook::{self, Playbooks};
use aircord_core::scan::{Column, Sort};
use aircord_core::session::Sessions;
use aircord_core::{config, filter, ota, redact};
use egui::{
    Align, Align2, CentralPanel, Color32, Context, FontId, Frame, Id, Key, Layout, Margin, Modifiers, Panel, Rect, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, TextEdit, Ui, WidgetInfo, WidgetType, vec2,
};

mod confirm;
pub mod dialog;
mod firmware;
mod form;
mod guided;
pub mod icons;
pub mod inbox;
mod panel;
mod playbooks;
mod sessions;
pub mod style;
mod widgets;
mod xfer;

#[cfg(test)]
mod tests;

use confirm::Confirm;
use icons::Icon;
use inbox::{Inbox, Msg, Outbox};
use style::{RADIUS, RADIUS_SM, pal};
use widgets::{Alert, Tone, button, icon_button, rail_button};

/// How many terminal lines are kept.
pub const MAX_LOG_LINES: usize = 2000;

/// The device list repaints this often while scanning, not at each advertisement.
const SCAN_REFRESH: f64 = 0.5;

/// Where the app keeps its files: the user's config dir in the app, a temp dir in a test.
pub struct Stores {
    pub history: History,
    pub sessions: Sessions,
    pub playbooks: Playbooks,
    pub templates: Templates,
    pub logs: PathBuf,
}

impl Stores {
    /// The real ones, under `<config>/aircord`.
    pub fn open() -> Stores {
        Stores {
            history: History::open(),
            sessions: Sessions::open(),
            playbooks: Playbooks::open(),
            templates: Templates::load(),
            logs: config::dir("logs"),
        }
    }

    /// All of them under `dir`.
    pub fn at(dir: &std::path::Path) -> Stores {
        Stores {
            history: History::at(dir.join("history.json")),
            sessions: Sessions::at(dir.join("sessions")),
            playbooks: Playbooks::at(dir.join("playbooks")),
            templates: Templates::load_from(&dir.join("templates")),
            logs: dir.join("logs"),
        }
    }
}

/// How long the app waits, here and there. Tests shorten them.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// After a link comes up, before the PIN is sent: the device finishes its own start in that time, and a PIN sent
    /// earlier is swallowed.
    pub settle: Duration,
    /// For a playbook step's reply, and a read action's value.
    pub reply_wait: Duration,
    /// For each reply while the settings are read or transferred.
    pub read_wait: Duration,
    /// How long a device may report no network before the guided view says what usually helps.
    pub no_network_hint: Duration,
    pub ota: ota::Timing,
}

impl Timing {
    pub const REAL: Timing = Timing {
        settle: Duration::from_secs(4),
        reply_wait: playbook::WAIT_TIMEOUT,
        read_wait: Duration::from_secs(5),
        no_network_hint: Duration::from_secs(180),
        ota: ota::Timing::REAL,
    };
}

/// Which of the two faces the window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Settings as a form, for people who do not know AT commands.
    Guided,
    /// The terminal, the control panel, playbooks, sessions and firmware.
    Expert,
}

/// The selected device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected {
    pub addr: String,
    /// The name to show: the advertised one, else the address.
    pub name: String,
}

/// The console link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Conn {
    Idle,
    Connecting,
    Connected {
        name: String,
        addr: String,
        profile: String,
    },
    /// A firmware update holds the link: Disconnect works as an emergency stop, the console stays shut.
    Bootloader {
        name: String,
    },
}

/// One terminal line.
#[derive(Clone, Debug)]
pub struct LogLine {
    pub time: String,
    pub text: String,
}

/// Which dialog is open.
pub(crate) enum Modal {
    Sessions(sessions::Dialog),
    Playbooks(playbooks::Dialog),
    Firmware(firmware::Dialog),
}

/// A read action, or the firmware dialog's IMEI, waiting for its value in a reply line.
pub(crate) struct Capture {
    pub(crate) target: CaptureFor,
    /// Gives up at this time (egui's), reporting no reply, so a missed answer can't hijack a later line.
    pub(crate) until: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CaptureFor {
    /// The action at this index of the template shown.
    Action(usize),
    Imei,
}

/// The history picker over the command line.
pub(crate) struct Picker {
    filter: String,
    selected: usize,
    focus: bool,
}

/// The window.
pub struct App {
    link: Arc<dyn Link>,
    inbox: Inbox,
    pub stores: Stores,
    pub timing: Timing,
    styled: bool,
    now: f64,
    pub view: View,

    // the guided view
    pub(crate) guided: guided::Guided,
    pub(crate) transfer: Option<xfer::Transfer>,
    /// Counts the connects, so settings read for an earlier link are dropped.
    pub(crate) epoch: u64,
    /// Stops the reading of settings when the link goes.
    pub(crate) read_stop: Arc<AtomicBool>,
    /// Counts the links: a connect's thread that wakes after its link was replaced sends nothing to the new one.
    link_gen: Arc<std::sync::atomic::AtomicU64>,
    /// A restart was sent: the link going down is expected.
    pub(crate) restarting: bool,

    // discovery
    pub display: Vec<Device>,
    pub total: usize,
    pub filter: String,
    pub sort: Sort,
    last_refresh: f64,
    pub selected: Option<Selected>,
    /// A loaded session's target and template, made real once a scan finds the device and a connect happens.
    pub want_addr: Option<String>,
    pub want_template: Option<String>,
    /// The template was chosen by hand or by a loaded session: the device's model no longer changes it.
    pub(crate) template_fixed: bool,
    /// The template the device's model points to. Its secrets are masked even while another template is shown.
    pub(crate) model_template: Option<String>,
    pub pin: String,

    // the link and the console
    pub conn: Conn,
    pub log: VecDeque<LogLine>,
    pub cmd: String,
    history: Vec<String>,
    picker: Option<Picker>,
    focus_cmd: bool,
    pub status: String,
    pub alert: Alert,

    // the control panel
    pub template: Option<String>,
    /// The active template's secret keys beyond the always-secret ones: what [`App::log_line`] masks on top.
    secrets: Vec<String>,
    inputs: BTreeMap<usize, String>,
    pub(crate) values: BTreeMap<usize, String>,
    pub(crate) capture: Option<Capture>,

    // dialogs and jobs
    pub(crate) modal: Option<Modal>,
    pub(crate) confirm: Option<Confirm>,
    pub(crate) dialog: Option<dialog::Dialog>,
    pub(crate) ota: Option<firmware::Run>,
    pub(crate) running: Option<(String, Arc<AtomicBool>)>,
    pub(crate) ota_image: Option<Vec<u8>>,
    pub(crate) ota_file: String,
    pub(crate) ota_imei: String,
}

impl App {
    /// A window over `link`, whose events go to `inbox` (give the link `inbox.outbox()` first).
    pub fn new(link: Arc<dyn Link>, stores: Stores, inbox: Inbox) -> App {
        stores.playbooks.ensure_default(&playbook::dragino_1nce());
        let mut app = App {
            link,
            inbox,
            stores,
            timing: Timing::REAL,
            styled: false,
            now: 0.0,
            view: View::Guided,
            guided: guided::Guided::default(),
            transfer: None,
            epoch: 0,
            read_stop: Arc::new(AtomicBool::new(false)),
            link_gen: Arc::default(),
            restarting: false,
            display: Vec::new(),
            total: 0,
            filter: String::new(),
            sort: Sort::default(),
            last_refresh: f64::NEG_INFINITY,
            selected: None,
            want_addr: None,
            want_template: None,
            template_fixed: false,
            model_template: None,
            pin: String::new(),
            conn: Conn::Idle,
            log: VecDeque::new(),
            cmd: String::new(),
            history: Vec::new(),
            picker: None,
            focus_cmd: false,
            status: "idle".into(),
            alert: Alert::default(),
            template: None,
            secrets: Vec::new(),
            inputs: BTreeMap::new(),
            values: BTreeMap::new(),
            capture: None,
            modal: None,
            confirm: None,
            dialog: None,
            ota: None,
            running: None,
            ota_image: None,
            ota_file: String::new(),
            ota_imei: String::new(),
        };
        if let Some(e) = app.stores.history.load_error().map(str::to_string) {
            app.fail(format!("history load failed, backed up as .corrupt: {e}"));
        }
        for e in app.stores.templates.errors().to_vec() {
            app.fail(format!("template load skipped: {e}"));
        }
        app
    }

    fn outbox(&self) -> Outbox {
        self.inbox.outbox()
    }

    // ---------- the terminal's record ----------

    /// A timestamped line in the terminal, its passwords masked: the fixed ones and the active template's secrets. The
    /// terminal is what Copy and Save hand out, so nothing gets into it in clear.
    pub fn log_line(&mut self, text: impl Into<String>) {
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        let text = self.masked(&text.into());
        self.log.push_back(LogLine { time, text });
        while self.log.len() > MAX_LOG_LINES {
            self.log.pop_front();
        }
    }

    /// `line` with every `AT+<secret>=` value in it masked.
    fn masked(&self, line: &str) -> String {
        let extra: Vec<&str> = self.secrets.iter().map(String::as_str).collect();
        redact::redact_line(line, &extra).into_owned()
    }

    /// Makes `name` the template shown. Its secrets are masked from here on, and in what the terminal already holds:
    /// the lines that named the model came before the template was known.
    pub(crate) fn set_template(&mut self, name: Option<String>) {
        self.template = name;
        self.values.clear();
        self.inputs.clear();
        if matches!(self.capture, Some(Capture { target: CaptureFor::Action(_), .. })) {
            self.capture = None;
        }
        self.refresh_secrets();
    }

    /// The device named its model. The template follows it unless a person or a session chose one: then only the
    /// masking follows, since the device's secrets are those of its model whatever the panel shows.
    pub(crate) fn model_known(&mut self, model: &str) {
        let by_model = self.stores.templates.for_model(model).map(|t| t.name.clone());
        self.model_template.clone_from(&by_model);
        if by_model.is_some() && by_model != self.template && !self.template_fixed {
            self.set_template(by_model);
        } else {
            self.refresh_secrets();
        }
    }

    /// The keys to mask: the secrets of the template shown and of the one the model points to.
    fn refresh_secrets(&mut self) {
        let mut secrets: Vec<String> = Vec::new();
        for name in [&self.template, &self.model_template].into_iter().flatten() {
            let Some(t) = self.stores.templates.get(name) else { continue };
            for key in t.secret_keys() {
                if !redact::is_always_secret(key) && !secrets.iter().any(|s| s == key) {
                    secrets.push(key.to_string());
                }
            }
        }
        if secrets != self.secrets {
            self.secrets = secrets;
            if !self.secrets.is_empty() {
                let lines: Vec<String> = self.log.iter().map(|l| self.masked(&l.text)).collect();
                for (line, text) in self.log.iter_mut().zip(lines) {
                    line.text = text;
                }
            }
        }
    }

    /// The whole terminal as text, as Copy and Save give it.
    pub fn log_text(&self) -> String {
        self.log.iter().map(|l| format!("{} {}", l.time, l.text)).collect::<Vec<_>>().join("\n")
    }

    /// An error on the alert line, and in the terminal, which is the durable record: an alert can be superseded, the
    /// log scrolled back through and saved.
    pub fn fail(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        self.log_line(format!("!! {msg}"));
        self.alert.fail(msg);
    }

    fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    fn connected(&self) -> bool {
        matches!(self.conn, Conn::Connected { .. })
    }

    fn ota_running(&self) -> bool {
        self.ota.as_ref().is_some_and(|r| r.done.is_none())
    }

    // ---------- the frame ----------

    /// One frame of the window.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.inbox.attach(&ctx);
        if !self.styled {
            style::apply(&ctx);
            self.styled = true;
        }
        style::follow(&ctx);
        self.now = ctx.input(|i| i.time);
        for msg in self.inbox.drain() {
            self.on_msg(msg);
        }
        self.dialog_answered();
        self.tick(&ctx);

        Panel::top("titlebar").frame(widgets::chrome(Margin::symmetric(12, 5))).show(ui, |ui| self.title_bar(ui));
        if self.view == View::Guided {
            self.guided_view(ui);
            self.transfer_window(&ctx);
            self.confirm_window(&ctx);
            return;
        }
        Panel::top("options").frame(widgets::chrome(Margin::symmetric(12, 6))).show(ui, |ui| self.options_bar(ui));
        Panel::bottom("status").frame(widgets::chrome(Margin::symmetric(12, 3))).show(ui, |ui| self.status_bar(ui));
        Panel::left("rail")
            .frame(widgets::chrome(Margin::symmetric(4, 8)))
            .resizable(false)
            .exact_size(widgets::RAIL_W + 8.0)
            .show(ui, |ui| self.rail(ui));
        // The dock: the cards sit on it 8 pt apart, as PhotoCraft's panels do.
        let dock = |m: Margin| Frame::new().fill(pal().dock).inner_margin(m);
        // The docks start narrower in a small window, so the terminal keeps room beside them.
        let room = ui.available_width();
        Panel::left("devices")
            .frame(dock(Margin { left: 8, right: 4, top: 8, bottom: 8 }))
            .show_separator_line(false)
            .resizable(true)
            .default_size((room * 0.28).clamp(260.0, 330.0))
            .min_size(260.0)
            .show(ui, |ui| {
                widgets::card().show(ui, |ui| self.devices_panel(ui));
            });
        Panel::right("panel")
            .frame(dock(Margin { left: 4, right: 8, top: 8, bottom: 8 }))
            .show_separator_line(false)
            .resizable(true)
            .default_size((room * 0.26).clamp(240.0, 300.0))
            .min_size(240.0)
            .show(ui, |ui| {
                widgets::card().show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.control_panel(ui);
                });
            });
        CentralPanel::default().frame(dock(Margin::symmetric(4, 8))).show(ui, |ui| self.console(ui));

        self.picker_window(&ctx);
        self.modal_window(&ctx);
        self.ota_window(&ctx);
        self.confirm_window(&ctx);
    }

    /// What time brings: the device list while scanning, and a capture that waited too long.
    fn tick(&mut self, ctx: &Context) {
        if self.link.is_scanning() {
            if self.now - self.last_refresh >= SCAN_REFRESH {
                self.last_refresh = self.now;
                self.snapshot();
                self.resolve_wanted();
                let (shown, total) = (self.display.len(), self.total);
                if self.ota.is_none() {
                    self.set_status(format!("scanning… {shown}/{total} shown"));
                }
            }
            ctx.request_repaint_after(Duration::from_secs_f64(SCAN_REFRESH));
        }
        if let Some(c) = &self.capture {
            if self.now >= c.until {
                let target = c.target.clone();
                self.capture = None;
                self.captured(target, None);
            } else {
                ctx.request_repaint_after(Duration::from_secs_f64(c.until - self.now));
            }
        }
    }

    fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Log(line) => {
                if let Some(reply) = line.strip_prefix("<< ") {
                    self.try_capture(reply);
                    self.feed_status(reply);
                    self.on_model_line(reply);
                }
                self.log_line(line);
            }
            Msg::LinkDropped => {
                self.log_line("-- link dropped (device reset or out of range) — reconnect --");
                self.set_disconnected("link dropped — reconnect");
                if std::mem::take(&mut self.restarting) {
                    self.guided.notice.ok("Das Gerät startet neu. In etwa einer Minute lässt es sich wieder verbinden.");
                } else {
                    self.guided.notice.warn("Die Verbindung zum Gerät ist abgebrochen. Bitte neu verbinden.");
                }
            }
            Msg::ScanFailed(e) => {
                self.guided.notice.fail("Die Suche ist fehlgeschlagen. Ist Bluetooth eingeschaltet und für Aircord erlaubt?");
                self.fail(e);
            }
            Msg::Cfg { epoch, result } => self.on_cfg(epoch, result),
            Msg::Transfer(p) => self.on_transfer(p),
            Msg::TransferDone(outcome) => self.on_transfer_done(outcome),
            Msg::Connected { name, addr, profile } => {
                self.log_line(format!("-- connected via {profile} UART profile, ready --"));
                self.set_connected(name, addr, profile);
            }
            Msg::ConnectFailed => {
                self.conn = Conn::Idle;
                self.guided.notice.fail("Die Verbindung ist nicht zustande gekommen. Ist das Gerät eingeschaltet und in der Nähe?");
            }
            Msg::Status(s) => self.set_status(s),
            Msg::Playbook(playbook::Report::Step { n, total, cmd }) => {
                let name = self.running.as_ref().map(|r| r.0.clone()).unwrap_or_default();
                self.set_status(format!("playbook {name} — step {n}/{total}: {cmd}"));
                self.log_line(format!("-- playbook {name} [{n}/{total}]: {cmd} --"));
            }
            Msg::PlaybookDone { name, result } => {
                self.running = None;
                match result {
                    Ok(()) => {
                        self.log_line(format!("-- playbook {name} finished --"));
                        self.set_status(format!("playbook finished: {name}"));
                        self.alert.ok(format!("playbook finished: {name}"));
                    }
                    Err(e) => {
                        self.fail(e);
                        self.set_status(format!("playbook aborted: {name}"));
                    }
                }
            }
            Msg::Ota(r) => self.on_ota(r),
            Msg::OtaDone(result) => self.ota_done(result),
        }
    }

    // ---------- discovery ----------

    /// The discovered devices, filtered and sorted, into the list.
    fn snapshot(&mut self) {
        let all = self.link.devices();
        self.total = all.len();
        let mut shown = filter::matching(&all, &self.filter, |d| format!("{} {}", d.name, d.addr));
        self.sort.apply(&mut shown);
        self.display = shown;
    }

    pub fn toggle_scan(&mut self) {
        if self.link.is_scanning() {
            self.stop_scan();
        } else {
            self.link.scan();
            self.last_refresh = f64::NEG_INFINITY;
            self.set_status("scanning…");
        }
    }

    fn stop_scan(&mut self) {
        if !self.link.is_scanning() {
            return;
        }
        self.link.stop_scan();
        self.snapshot();
        self.resolve_wanted();
        self.set_status(format!("stopped — {}/{} shown", self.display.len(), self.total));
    }

    pub fn clear_list(&mut self) {
        self.link.clear_devices();
        self.display.clear();
        self.total = 0;
        self.set_status("cleared");
    }

    pub fn select(&mut self, d: &Device) {
        let name = if d.name.is_empty() { d.addr.clone() } else { d.name.clone() };
        self.set_status(format!("selected: {name}"));
        self.selected = Some(Selected { addr: d.addr.clone(), name });
        self.want_addr = None;
    }

    /// A session's target becomes the selection once a live scan finds it (a snapshot has no usable handle).
    fn resolve_wanted(&mut self) {
        let Some(want) = self.want_addr.clone() else { return };
        let Some(d) = self.display.iter().find(|d| d.addr == want).cloned() else { return };
        self.select(&d);
        let name = self.selected.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        self.log_line(format!("-- session target found: {name} — connect ready --"));
    }

    // ---------- the link ----------

    pub fn connect(&mut self) {
        if self.conn != Conn::Idle {
            return;
        }
        if self.ota_running() {
            self.alert.warn("a firmware update is running — it connects to the bootloader itself");
            return;
        }
        let Some(sel) = self.selected.clone() else {
            self.alert.warn("select a device in the list first");
            return;
        };
        // Scanning and connecting don't go together.
        self.stop_scan();
        self.conn = Conn::Connecting;
        self.set_status(format!("connecting to {}…", sel.name));
        self.epoch += 1;
        self.read_stop = Arc::new(AtomicBool::new(false));
        self.restarting = false;
        let (link, out, pin, settle) = (Arc::clone(&self.link), self.outbox(), self.pin.trim().to_string(), self.timing.settle);
        let (epoch, stop, read_wait) = (self.epoch, Arc::clone(&self.read_stop), self.timing.read_wait);
        let generation = Arc::clone(&self.link_gen);
        let mine = generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        std::thread::spawn(move || {
            out.log(format!("-- connecting to {} ({}) --", sel.name, sel.addr));
            let Some(profile) = link.connect(&sel.addr) else {
                out.post(Msg::ConnectFailed);
                return;
            };
            let addr = link.current_addr().unwrap_or(sel.addr.clone());
            out.post(Msg::Connected { name: sel.name.clone(), addr, profile });
            // Four silent seconds after "connected" read as a hang: say what is happening.
            let next = if pin.is_empty() { "reading the settings" } else { "sending PIN" };
            out.post(Msg::Status(format!("connected: {} — settling, {next} in {}s", sel.name, settle.as_secs())));
            out.log(format!("-- waiting {}s for the device to settle, then {next} --", settle.as_secs()));
            std::thread::sleep(settle);
            // Disconnected, and perhaps connected to another device meanwhile: this PIN is not for that one.
            if generation.load(std::sync::atomic::Ordering::Relaxed) != mine {
                return;
            }
            if !link.is_connected() {
                out.log("-- link dropped during init --");
                return;
            }
            // The PIN first, if there is one, then the settings: the guided view's form, and the model for the template.
            let result = guided::read_settings(&*link, Some(pin.as_str()).filter(|p| !p.is_empty()), read_wait, &stop);
            out.post(Msg::Status(format!("connected: {}", sel.name)));
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            out.post(Msg::Cfg { epoch, result });
        });
    }

    pub fn disconnect(&mut self) {
        if !self.link.disconnect() {
            return;
        }
        self.log_line("-- disconnected --");
        self.set_disconnected("disconnected");
    }

    fn set_connected(&mut self, name: String, addr: String, profile: String) {
        self.history = self.stores.history.get(&addr);
        self.set_status(format!("connected: {name}"));
        self.apply_template_for(&name);
        self.guided_reset(guided::Read::Reading);
        self.conn = Conn::Connected { name, addr, profile };
        self.focus_cmd = true;
    }

    fn set_disconnected(&mut self, status: &str) {
        self.conn = Conn::Idle;
        self.link_gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.read_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.guided_reset(guided::Read::Idle);
        if matches!(self.transfer, Some(xfer::Transfer { stage: xfer::Stage::Review, .. })) {
            self.transfer = None;
        }
        self.template_fixed = false;
        self.model_template = None;
        self.set_template(None);
        self.capture = None;
        self.picker = None;
        self.set_status(status);
    }

    /// Sends an AT command and keeps it in the device's history. The command line and the control panel both send
    /// through here. A command that sets a password is sent but not kept: the history is a file on disk, and a masked
    /// entry would send the mask as the new password when run again.
    pub fn send_cmd(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if !self.connected() || !self.link.is_connected() {
            self.alert.warn("not connected — connect to a device first");
            return;
        }
        // The settings being read by themselves give way: the reply would end up in the dump, or the dump in the reply.
        self.interrupt_read();
        let extra: Vec<&str> = self.secrets.iter().map(String::as_str).collect();
        if redact::is_secret_command(cmd, &extra) {
            self.link.send(cmd);
            return;
        }
        if let Some(addr) = self.link.current_addr()
            && let Err(e) = self.stores.history.append(&addr, cmd)
        {
            self.fail(format!("history save failed: {e}"));
        }
        if self.history.last().is_none_or(|l| l != cmd) {
            self.history.push(cmd.to_string());
        }
        self.link.send(cmd);
    }

    pub fn send_from_line(&mut self) {
        let cmd = self.cmd.trim().to_string();
        if cmd.is_empty() {
            return;
        }
        if !self.connected() {
            self.alert.warn("not connected — connect to a device first");
            return;
        }
        self.cmd.clear();
        self.send_cmd(&cmd);
    }

    // ---------- the terminal's tools ----------

    fn copy_log(&mut self, ctx: &Context) {
        ctx.copy_text(self.log_text());
        self.alert.ok("log copied to clipboard");
    }

    /// Writes the log to a timestamped file in the logs folder and says where.
    pub fn save_log(&mut self) {
        let text = self.log_text();
        if text.is_empty() {
            self.alert.warn("log is empty — nothing to save");
            return;
        }
        let path = self.stores.logs.join(format!("aircord-{}.log", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        match config::atomic_write(&path, format!("{text}\n").as_bytes(), 0o644) {
            Ok(()) => {
                self.alert.ok("log saved");
                self.log_line(format!("-- log saved: {} --", path.display()));
            }
            Err(e) => self.fail(format!("save failed: {e}")),
        }
    }

    // ---------- drawing ----------

    /// The title bar: the brand at the left, the light/dark switch at the right (a moon in light, a sun in dark).
    fn title_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(26.0);
            widgets::brand(ui);
            ui.add_space(10.0);
            if widgets::tab(ui, "Einstellungen", self.view == View::Guided)
                .on_hover_text("Das Gerät über ein Formular einstellen")
                .clicked()
            {
                self.set_view(View::Guided);
            }
            if widgets::tab(ui, "Experte", self.view == View::Expert)
                .on_hover_text("Terminal, Bedienfeld, Playbooks, Sessions und Firmware")
                .clicked()
            {
                self.set_view(View::Expert);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (icon, label) = if style::dark() { (Icon::Sun, "Light theme") } else { (Icon::Moon, "Dark theme") };
                if icon_button(ui, icon, label).clicked() {
                    style::toggle(ui.ctx());
                    ui.ctx().request_repaint();
                }
            });
        });
    }

    /// The options bar, PhotoCraft's tool options: the discovery actions, then a hint for what to do next.
    fn options_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let scanning = self.link.is_scanning();
            let (icon, label) = if scanning { (Icon::Stop, "Stop") } else { (Icon::Scan, "Scan") };
            if button(ui, Some(icon), label, Tone::Primary).on_hover_text("Discover nearby BLE devices").clicked() {
                self.toggle_scan();
            }
            if button(ui, Some(Icon::Clear), "Clear list", Tone::Plain).clicked() {
                self.clear_list();
            }
            widgets::vline(ui, 20.0);
            ui.label(RichText::new(self.hint()).color(pal().muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some((name, stop)) = &self.running {
                    let name = name.clone();
                    if button(ui, Some(Icon::Stop), "Stop playbook", Tone::Danger)
                        .on_hover_text(format!("Stop {name} after this step"))
                        .clicked()
                    {
                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            });
        });
    }

    /// What to do next, for the options bar.
    fn hint(&self) -> &'static str {
        match (&self.conn, self.link.is_scanning(), self.selected.is_some()) {
            (Conn::Bootloader { .. }, ..) => "A firmware update holds the link",
            (Conn::Connected { .. }, ..) => "Type an AT command below, or use the control panel",
            (Conn::Connecting, ..) => "Connecting…",
            (Conn::Idle, _, true) => "Connect to the selected device, under the list",
            (Conn::Idle, true, false) => "Scanning — pick a device in the list",
            (Conn::Idle, false, false) if self.total > 0 => "Pick a device in the list",
            (Conn::Idle, false, false) => "Scan to discover nearby BLE devices",
        }
    }

    /// The icon rail: the dialogs, the open one marked.
    fn rail(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let open = |m: &Option<Modal>| match m {
                Some(Modal::Sessions(_)) => 0,
                Some(Modal::Playbooks(_)) => 1,
                Some(Modal::Firmware(_)) => 2,
                None => usize::MAX,
            };
            let open = open(&self.modal);
            if rail_button(ui, Icon::Sessions, "Sessions", "Sessions — save or restore the device list, target and PIN", open == 0)
                .clicked()
            {
                self.open_sessions();
            }
            if rail_button(ui, Icon::Playbooks, "Playbooks", "Playbooks — replay a sequence of AT commands", open == 1).clicked() {
                self.open_playbooks();
            }
            if rail_button(ui, Icon::Firmware, "Firmware", "Firmware — flash a Dragino NB node over BLE", open == 2).clicked() {
                self.open_firmware();
            }
        });
    }

    /// The status bar, in PhotoCraft's segments: the link, the list, then what is happening.
    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(20.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            let (dot, text) = match &self.conn {
                Conn::Idle => (pal().disabled, "not connected".to_string()),
                Conn::Connecting => (pal().warn, "connecting…".to_string()),
                Conn::Connected { name, profile, .. } => (pal().ok, format!("{name} · {profile}")),
                Conn::Bootloader { name } => (pal().warn, format!("bootloader {name}")),
            };
            widgets::dot(ui, dot);
            ui.label(RichText::new(text).color(pal().text));
            widgets::vline(ui, 16.0);
            ui.label(RichText::new(format!("{}/{} devices", self.display.len(), self.total)).color(pal().text));
            widgets::vline(ui, 16.0);
            ui.label(RichText::new(&self.status).color(pal().muted));
        });
    }

    fn devices_panel(&mut self, ui: &mut Ui) {
        ui.set_min_size(ui.available_size());
        widgets::section(ui, "Devices");
        let edit = TextEdit::singleline(&mut self.filter).hint_text("filter — regex or substring (8606, ^D20)");
        if widgets::field(ui, edit, Some(Icon::Search), None, ui.available_width()).changed() {
            self.snapshot();
            self.set_status(format!("{}/{} shown", self.display.len(), self.total));
        }
        ui.add_space(2.0);

        // The connection, pinned to the panel's foot.
        Panel::bottom("connect")
            .frame(Frame::new().inner_margin(Margin { top: 8, ..Margin::ZERO }))
            .show_separator_line(true)
            .show(ui, |ui| self.connect_row(ui));
        self.device_table(ui);
    }

    fn device_table(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        let cols = [width * 0.44, width * 0.16, width * 0.40];
        // The header: each column title sorts by it, a second click reverses.
        let (head, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
        ui.painter().hline(head.x_range(), head.bottom(), Stroke::new(1.0, pal().separator));
        let mut x = head.left();
        for (i, column) in Column::ALL.into_iter().enumerate() {
            let rect = Rect::from_min_size(egui::pos2(x, head.top()), vec2(cols[i], head.height()));
            let r = ui.interact(rect, Id::new(("sort", i)), Sense::click());
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, column.title()));
            if r.hovered() {
                ui.painter().rect_filled(rect, RADIUS_SM, pal().hover_fill);
            }
            let active = self.sort.column == column;
            let ink = if active { pal().text } else { pal().muted };
            let text =
                ui.painter().text(rect.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, column.title(), style::medium(11.5), ink);
            if active {
                // The direction as a drawn triangle: the UI font has no arrow glyphs.
                let c = egui::pos2(text.right() + 8.0, rect.center().y);
                let (tip, base) = if self.sort.ascending { (-3.5, 2.5) } else { (3.5, -2.5) };
                let points = vec![egui::pos2(c.x - 4.0, c.y + base), egui::pos2(c.x + 4.0, c.y + base), egui::pos2(c.x, c.y + tip)];
                ui.painter().add(egui::Shape::convex_polygon(points, pal().accent, Stroke::NONE));
            }
            if r.clicked() {
                self.sort = self.sort.clicked(column);
                self.snapshot();
            }
            x += cols[i];
        }

        if self.display.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No devices yet").font(style::semibold(13.0)).color(pal().text));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Press Scan to discover nearby devices.\nmacOS asks for Bluetooth permission on the first scan.")
                        .color(pal().muted),
                );
            });
            return;
        }
        let row_h = 26.0;
        let rows = self.display.clone();
        let mut clicked = None;
        let selected = self.selected.as_ref().map(|s| s.addr.clone());
        ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, rows.len(), |ui, range| {
            for i in range {
                let d = &rows[i];
                let (rect, r) = ui.allocate_exact_size(vec2(width, row_h), Sense::click());
                let label = if d.name.is_empty() { d.addr.clone() } else { d.name.clone() };
                let is_sel = selected.as_deref() == Some(d.addr.as_str());
                r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, is_sel, &label));
                // Flat rows, as PhotoCraft's lists: the selection a soft accent pill, hover a grey one.
                let (fill, stroke) = if is_sel {
                    (pal().select_fill, Stroke::new(1.0, pal().accent_border))
                } else if r.hovered() {
                    (pal().hover_fill, Stroke::NONE)
                } else {
                    (Color32::TRANSPARENT, Stroke::NONE)
                };
                let p = ui.painter();
                p.rect(rect.shrink2(vec2(0.0, 1.0)), RADIUS_SM, fill, stroke, StrokeKind::Inside);
                let mut x = rect.left() + 6.0;
                let (name, name_ink) =
                    if d.name.is_empty() { ("(no name)".to_string(), pal().muted) } else { (d.name.clone(), pal().text) };
                clip_text(p, rect, x, cols[0] - 10.0, &name, FontId::proportional(13.0), name_ink);
                x += cols[0];
                p.text(
                    egui::pos2(x + cols[1] - 14.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    d.rssi.to_string(),
                    FontId::monospace(12.0),
                    signal_ink(d.rssi),
                );
                x += cols[1];
                clip_text(p, rect, x, cols[2] - 8.0, &d.addr, FontId::monospace(11.5), pal().muted);
                if r.clicked() {
                    clicked = Some(d.clone());
                }
            }
        });
        if let Some(d) = clicked {
            self.select(&d);
        }
    }

    fn connect_row(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("PIN").color(pal().muted));
            // A password field: this window gets screen-shared and photographed in the field.
            let pin = TextEdit::singleline(&mut self.pin).password(true).hint_text("optional, sent on connect");
            widgets::field(ui, pin, None, None, ui.available_width())
                .on_hover_text("Sent once the link has settled; never written to the log");
        });
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let can_connect = self.selected.is_some() && self.conn == Conn::Idle && !self.ota_running();
            let target = self.selected.as_ref().map_or("select a device first".to_string(), |s| format!("Connect to {}", s.name));
            if ui
                .add_enabled_ui(can_connect, |ui| button(ui, Some(Icon::Connect), "Connect", Tone::Primary))
                .inner
                .on_hover_text(target)
                .clicked()
            {
                self.connect();
            }
            let can_drop = matches!(self.conn, Conn::Connected { .. } | Conn::Bootloader { .. });
            if ui.add_enabled_ui(can_drop, |ui| button(ui, Some(Icon::Disconnect), "Disconnect", Tone::Plain)).inner.clicked() {
                self.disconnect();
            }
        });
    }

    fn console(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            // The terminal as PhotoCraft's open document: a tab naming it and the link it talks over.
            let detail = match &self.conn {
                Conn::Connected { profile, .. } => format!("{profile} UART"),
                Conn::Bootloader { .. } => "bootloader".into(),
                Conn::Connecting => "connecting…".into(),
                Conn::Idle => "offline".into(),
            };
            // A narrow column keeps the title and the tools; the detail is in the status bar too.
            let detail = if ui.available_width() < 320.0 { String::new() } else { detail };
            widgets::doc_tab(ui, "Terminal", &detail);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, Icon::Delete, "Clear log").clicked() {
                    self.log.clear();
                }
                if icon_button(ui, Icon::Save, "Save log").clicked() {
                    self.save_log();
                }
                if icon_button(ui, Icon::Copy, "Copy log").clicked() {
                    self.copy_log(&ctx);
                }
            });
        });
        self.alert.show(ui);
        ui.add_space(4.0);

        Panel::bottom("command")
            .frame(Frame::new().inner_margin(Margin { top: 8, ..Margin::ZERO }))
            .show_separator_line(false)
            .show(ui, |ui| self.command_row(ui));

        let term =
            Frame::new().fill(pal().card).stroke(Stroke::new(1.0, pal().card_border)).corner_radius(RADIUS).inner_margin(Margin::same(10));
        term.show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.spacing_mut().item_spacing.y = 0.0;
            let font = FontId::monospace(12.0);
            let row_h = ui.fonts_mut(|f| f.row_height(&font)) + 4.0;
            let n = self.log.len();
            ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, row_h, n, |ui, range| {
                for i in range {
                    let Some(line) = self.log.get(i) else { continue };
                    // One label a line, the time faint before it: each line selectable, rows a fixed height.
                    let mut job = egui::text::LayoutJob::default();
                    let fmt = |color| egui::TextFormat { font_id: font.clone(), color, line_height: Some(row_h), ..Default::default() };
                    job.append(&line.time, 0.0, fmt(pal().faint));
                    job.append(&line.text, 10.0, fmt(line_ink(&line.text)));
                    ui.add(egui::Label::new(job).extend().selectable(true));
                }
            });
        });
    }

    fn command_row(&mut self, ui: &mut Ui) {
        let open = self.connected();
        let id = Id::new("command-line");
        ui.horizontal(|ui| {
            let focused = ui.memory(|m| m.has_focus(id));
            if open && focused && self.picker.is_none() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                self.open_picker();
            }
            let send_w = 96.0 + 34.0;
            let line = TextEdit::singleline(&mut self.cmd)
                .id(id)
                .font(egui::TextStyle::Monospace)
                .hint_text("AT command (e.g. AT+CFG) — Enter sends, Up opens the history");
            let width = ui.available_width() - send_w;
            let edit = ui.add_enabled_ui(open, |ui| widgets::field(ui, line, None, None, width)).inner;
            if std::mem::take(&mut self.focus_cmd) && open {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                self.send_from_line();
                edit.request_focus();
            }
            ui.add_enabled_ui(open && !self.history.is_empty(), |ui| {
                if icon_button(ui, Icon::History, "Command history").clicked() {
                    self.open_picker();
                }
            });
            if ui.add_enabled_ui(open, |ui| button(ui, Some(Icon::Send), "Send", Tone::Primary)).inner.clicked() {
                self.send_from_line();
            }
        });
    }

    // ---------- the history picker ----------

    fn open_picker(&mut self) {
        if !self.history.is_empty() {
            self.picker = Some(Picker { filter: String::new(), selected: 0, focus: true });
        }
    }

    /// Newest first, filtered.
    fn picker_items(&self) -> Vec<String> {
        let newest: Vec<String> = self.history.iter().rev().cloned().collect();
        filter::matching(&newest, self.picker.as_ref().map_or("", |p| &p.filter), Clone::clone)
    }

    fn picker_window(&mut self, ctx: &Context) {
        if self.picker.is_none() {
            return;
        }
        let items = self.picker_items();
        let Some(p) = self.picker.as_mut() else { return };
        p.selected = p.selected.min(items.len().saturating_sub(1));
        // Navigation keys go to the list, not the filter field.
        let (up, down, enter, right, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::ArrowRight),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if up {
            p.selected = p.selected.saturating_sub(1);
        }
        if down && p.selected + 1 < items.len() {
            p.selected += 1;
        }
        let mut pick: Option<(String, bool)> = None;
        if (enter || right)
            && let Some(item) = items.get(p.selected)
        {
            pick = Some((item.clone(), enter));
        }
        let mut close = esc || pick.is_some();
        let screen = ctx.content_rect();
        let at = egui::pos2(screen.center().x - 260.0, screen.bottom() - 120.0);
        egui::Area::new(Id::new("history-picker")).order(egui::Order::Foreground).fixed_pos(at).pivot(Align2::LEFT_BOTTOM).show(
            ctx,
            |ui| {
                Frame::popup(ui.style()).fill(pal().surface).show(ui, |ui| {
                    ui.set_width(520.0);
                    ui.label(RichText::new("History — Up/Down select · Enter runs · Right edits · Esc closes").color(pal().muted));
                    let filter = TextEdit::singleline(&mut p.filter).hint_text("filter history — regex/substring");
                    let f = widgets::field(ui, filter, Some(Icon::Search), None, ui.available_width());
                    if std::mem::take(&mut p.focus) {
                        f.request_focus();
                    }
                    if f.changed() {
                        p.selected = 0;
                    }
                    ui.add_space(4.0);
                    ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        if items.is_empty() {
                            ui.label(RichText::new("nothing matches").color(pal().muted));
                        }
                        for (i, item) in items.iter().enumerate() {
                            let r = ui.add(
                                egui::Button::selectable(i == p.selected, RichText::new(item).monospace()).min_size(vec2(500.0, 22.0)),
                            );
                            if i == p.selected && (up || down) {
                                r.scroll_to_me(None);
                            }
                            if r.clicked() {
                                pick = Some((item.clone(), false));
                                close = true;
                            }
                        }
                    });
                });
            },
        );
        if close {
            self.picker = None;
            self.focus_cmd = true;
            if let Some((cmd, run)) = pick {
                self.cmd = cmd;
                if run {
                    self.send_from_line();
                }
            }
        }
    }
}

/// Text cut at `width` with an ellipsis, so a long name never runs into the next column.
fn clip_text(p: &egui::Painter, row: Rect, x: f32, width: f32, text: &str, font: FontId, ink: Color32) {
    let galley = p.layout_no_wrap(text.to_string(), font.clone(), ink);
    if galley.size().x <= width {
        p.galley(egui::pos2(x, row.center().y - galley.size().y / 2.0), galley, ink);
        return;
    }
    let mut cut: String = text.to_string();
    while !cut.is_empty() && p.layout_no_wrap(format!("{cut}…"), font.clone(), ink).size().x > width {
        cut.pop();
    }
    p.text(egui::pos2(x, row.center().y), Align2::LEFT_CENTER, format!("{cut}…"), font, ink);
}

/// A signal's colour: strong green, fair default, weak muted.
fn signal_ink(rssi: i16) -> Color32 {
    match rssi {
        -65..=0 => pal().ok,
        -85..=-66 => pal().text,
        _ => pal().muted,
    }
}

/// A terminal line's colour by what it is: sent, an error, Aircord's own note, or what the device said.
fn line_ink(text: &str) -> Color32 {
    if text.starts_with(">> ") {
        pal().sent
    } else if text.starts_with("!! ") {
        pal().error
    } else if text.starts_with("-- ") || text.starts_with("  ") {
        pal().note
    } else {
        pal().text
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}
