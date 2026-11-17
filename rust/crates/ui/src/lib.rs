//! `aircord-ui`: the desktop window, in egui, in BIMCraft's look. A thin shell: what the app does lives in
//! `aircord-core`; this draws it, and runs the long jobs (connect, a playbook, a firmware update) on threads of their own
//! that report through [`inbox::Outbox`].
//!
//! The layout: a tool bar over everything; the device list on the left with the PIN and Connect under it; the terminal
//! and the command line in the middle; the device's control panel on the right; the status bar at the bottom. Dialogs
//! are modal: sessions, playbooks, firmware, and the confirmations.

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
use aircord_core::{config, filter, ota};
use egui::{
    Align, Align2, CentralPanel, Color32, Context, FontId, Frame, Id, Key, Layout, Margin, Modifiers, Panel, Rect, RichText, ScrollArea,
    Sense, Stroke, TextEdit, Theme, Ui, WidgetInfo, WidgetType, vec2,
};

mod confirm;
pub mod dialog;
mod firmware;
pub mod icons;
pub mod inbox;
mod panel;
mod playbooks;
mod sessions;
pub mod style;
mod widgets;

#[cfg(test)]
mod tests;

use confirm::Confirm;
use icons::Icon;
use inbox::{Inbox, Msg, Outbox};
use style::pal;
use widgets::{Alert, Tone, button, icon_button};

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
    pub ota: ota::Timing,
}

impl Timing {
    pub const REAL: Timing = Timing { settle: Duration::from_secs(4), reply_wait: playbook::WAIT_TIMEOUT, ota: ota::Timing::REAL };
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
            display: Vec::new(),
            total: 0,
            filter: String::new(),
            sort: Sort::default(),
            last_refresh: f64::NEG_INFINITY,
            selected: None,
            want_addr: None,
            want_template: None,
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

    /// A timestamped line in the terminal.
    pub fn log_line(&mut self, text: impl Into<String>) {
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        self.log.push_back(LogLine { time, text: text.into() });
        while self.log.len() > MAX_LOG_LINES {
            self.log.pop_front();
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

        let bar = Frame::new().fill(pal().bar).inner_margin(Margin::symmetric(8, 6)).stroke(Stroke::new(1.0, pal().border));
        Panel::top("toolbar").frame(bar).show(ui, |ui| self.toolbar(ui));
        Panel::bottom("status")
            .frame(Frame::new().fill(pal().bar).inner_margin(Margin::symmetric(10, 4)))
            .show(ui, |ui| self.status_bar(ui));
        let side = Frame::new().fill(pal().panel).inner_margin(Margin::same(10));
        Panel::left("devices").frame(side).resizable(true).default_size(330.0).min_size(260.0).show(ui, |ui| self.devices_panel(ui));
        Panel::right("panel").frame(side).resizable(true).default_size(300.0).min_size(240.0).show(ui, |ui| self.control_panel(ui));
        CentralPanel::default().frame(Frame::new().fill(pal().panel).inner_margin(Margin::same(10))).show(ui, |ui| self.console(ui));

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
                }
                self.log_line(line);
            }
            Msg::LinkDropped => {
                self.log_line("-- link dropped (device reset or out of range) — reconnect --");
                self.set_disconnected("link dropped — reconnect");
            }
            Msg::ScanFailed(e) => self.fail(e),
            Msg::Connected { name, addr, profile } => {
                self.log_line(format!("-- connected via {profile} UART profile, ready --"));
                self.set_connected(name, addr, profile);
            }
            Msg::ConnectFailed => self.conn = Conn::Idle,
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
        let (link, out, pin, settle) = (Arc::clone(&self.link), self.outbox(), self.pin.trim().to_string(), self.timing.settle);
        std::thread::spawn(move || {
            out.log(format!("-- connecting to {} ({}) --", sel.name, sel.addr));
            let Some(profile) = link.connect(&sel.addr) else {
                out.post(Msg::ConnectFailed);
                return;
            };
            let addr = link.current_addr().unwrap_or(sel.addr.clone());
            out.post(Msg::Connected { name: sel.name.clone(), addr, profile });
            if pin.is_empty() {
                return;
            }
            // Four silent seconds after "connected" read as a hang: say what is happening.
            out.post(Msg::Status(format!("connected: {} — settling, PIN in {}s", sel.name, settle.as_secs())));
            out.log(format!("-- waiting {}s for the device to settle, then sending PIN --", settle.as_secs()));
            std::thread::sleep(settle);
            if link.is_connected() {
                link.send_secret(&pin);
                out.post(Msg::Status(format!("connected: {}", sel.name)));
            } else {
                out.log("-- PIN skipped: link dropped during init --");
            }
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
        self.conn = Conn::Connected { name, addr, profile };
        self.focus_cmd = true;
    }

    fn set_disconnected(&mut self, status: &str) {
        self.conn = Conn::Idle;
        self.template = None;
        self.capture = None;
        self.picker = None;
        self.set_status(status);
    }

    /// Sends an AT command and keeps it in the device's history. The command line and the control panel both send
    /// through here.
    pub fn send_cmd(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }
        if !self.connected() || !self.link.is_connected() {
            self.alert.warn("not connected — connect to a device first");
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

    fn toolbar(&mut self, ui: &mut Ui) {
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
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
            if button(ui, Some(Icon::Sessions), "Sessions", Tone::Plain)
                .on_hover_text("Save or restore the device list, target and PIN")
                .clicked()
            {
                self.open_sessions();
            }
            if button(ui, Some(Icon::Playbooks), "Playbooks", Tone::Plain).on_hover_text("Replay a sequence of AT commands").clicked() {
                self.open_playbooks();
            }
            if button(ui, Some(Icon::Firmware), "Firmware", Tone::Plain).on_hover_text("Flash a Dragino NB node over BLE").clicked() {
                self.open_firmware();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let dark = style::dark();
                let label = if dark { "Light theme" } else { "Dark theme" };
                if icon_button(ui, Icon::Theme, label).clicked() {
                    ui.ctx().set_theme(if dark { Theme::Light } else { Theme::Dark });
                }
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

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.status).color(pal().muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (dot, text) = match &self.conn {
                    Conn::Idle => (pal().muted, "not connected".to_string()),
                    Conn::Connecting => (pal().warn, "connecting…".to_string()),
                    Conn::Connected { name, profile, .. } => (pal().ok, format!("{name} · {profile}")),
                    Conn::Bootloader { name } => (pal().warn, format!("bootloader {name}")),
                };
                ui.label(RichText::new(text).color(pal().text));
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, dot);
                ui.add_space(12.0);
                ui.label(RichText::new(format!("{}/{} devices", self.display.len(), self.total)).monospace().color(pal().muted));
            });
        });
    }

    fn devices_panel(&mut self, ui: &mut Ui) {
        widgets::section(ui, "DEVICES");
        ui.add_space(4.0);
        let field = ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 24.0), Sense::hover());
            icons::paint(ui.painter(), Icon::Search, Rect::from_center_size(r.center(), vec2(15.0, 15.0)), pal().muted);
            ui.add(
                TextEdit::singleline(&mut self.filter).hint_text("filter — regex or substring (8606, ^D20)").desired_width(f32::INFINITY),
            )
        });
        if field.inner.changed() {
            self.snapshot();
            self.set_status(format!("{}/{} shown", self.display.len(), self.total));
        }
        ui.add_space(6.0);

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
        ui.painter().hline(head.x_range(), head.bottom(), Stroke::new(1.0, pal().border));
        let mut x = head.left();
        for (i, column) in Column::ALL.into_iter().enumerate() {
            let rect = Rect::from_min_size(egui::pos2(x, head.top()), vec2(cols[i], head.height()));
            let r = ui.interact(rect, Id::new(("sort", i)), Sense::click());
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, column.title()));
            if r.hovered() {
                ui.painter().rect_filled(rect, 3.0, pal().hover_fill);
            }
            let active = self.sort.column == column;
            let ink = if active { pal().text } else { pal().muted };
            let text = ui.painter().text(
                rect.left_center() + vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                column.title(),
                FontId::proportional(12.0),
                ink,
            );
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
                ui.label(RichText::new("No devices yet").strong().color(pal().text));
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
                let fill = if is_sel {
                    pal().select_fill
                } else if r.hovered() {
                    pal().hover_fill
                } else if i % 2 == 1 {
                    pal().bar
                } else {
                    Color32::TRANSPARENT
                };
                let p = ui.painter();
                p.rect_filled(rect, 3.0, fill);
                if is_sel {
                    p.line_segment(
                        [rect.left_top() + vec2(1.0, 4.0), rect.left_bottom() + vec2(1.0, -4.0)],
                        Stroke::new(3.0, pal().accent),
                    );
                }
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
            let pin =
                TextEdit::singleline(&mut self.pin).password(true).hint_text("optional, sent on connect").desired_width(f32::INFINITY);
            // A password field: this window gets screen-shared and photographed in the field.
            ui.add(pin).on_hover_text("Sent once the link has settled; never written to the log");
        });
        ui.add_space(4.0);
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
            widgets::section(ui, "TERMINAL");
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

        let term = Frame::new().fill(pal().surface).stroke(Stroke::new(1.0, pal().border)).corner_radius(4).inner_margin(Margin::same(8));
        term.show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.spacing_mut().item_spacing.y = 0.0;
            let font = FontId::monospace(12.5);
            let row_h = ui.fonts_mut(|f| f.row_height(&font)) + 3.0;
            let n = self.log.len();
            ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, row_h, n, |ui, range| {
                for i in range {
                    let Some(line) = self.log.get(i) else { continue };
                    // One label a line, the time muted before it: each line selectable, rows a fixed height.
                    let mut job = egui::text::LayoutJob::default();
                    let fmt = |color| egui::TextFormat { font_id: font.clone(), color, line_height: Some(row_h), ..Default::default() };
                    job.append(&line.time, 0.0, fmt(pal().muted.gamma_multiply(0.8)));
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
            let edit = ui.add_enabled(
                open,
                TextEdit::singleline(&mut self.cmd)
                    .id(id)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("AT command (e.g. AT+CFG) — Enter sends, Up opens the history")
                    .desired_width(ui.available_width() - send_w),
            );
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
                    let f = ui.add(
                        TextEdit::singleline(&mut p.filter).hint_text("filter history — regex/substring").desired_width(f32::INFINITY),
                    );
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
