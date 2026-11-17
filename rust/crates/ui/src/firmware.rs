//! The firmware update: the dialog that picks the image, names the node by its IMEI and says how it gets reset; and
//! the progress window of a running update, with Cancel. The update itself is `aircord_core::ota::run`, on a thread.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aircord_core::ota::{self, Report};
use egui::{Align, Context, Grid, Id, Layout, Modal, ProgressBar, RichText, TextEdit};

use crate::confirm::Pending;
use crate::dialog::{Then, read_capped};
use crate::icons::Icon;
use crate::inbox::Msg;
use crate::style::pal;
use crate::widgets::{Alert, Tone, button, icon_button};
use crate::{App, CaptureFor, Conn, Modal as Open};

pub(crate) struct Dialog {
    pub(crate) imei: String,
    /// ATZ over the open link; otherwise the user presses RESET.
    pub(crate) reset_atz: bool,
    pub(crate) alert: Alert,
}

/// A running (or finished) update, for its progress window.
pub(crate) struct Run {
    pub(crate) phase: String,
    pub(crate) sent: usize,
    pub(crate) total: usize,
    pub(crate) done: Option<Result<(), String>>,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) hidden: bool,
}

impl App {
    pub fn open_firmware(&mut self) {
        if self.ota_running() {
            self.alert.warn("a firmware update is already running");
            return;
        }
        let imei = self.default_imei();
        self.modal = Some(Open::Firmware(Dialog { imei, reset_atz: self.connected(), alert: Alert::default() }));
    }

    /// The last IMEI used, else the selected device's name when it already is one (a node sitting in its bootloader).
    pub(crate) fn default_imei(&self) -> String {
        if !self.ota_imei.is_empty() {
            return self.ota_imei.clone();
        }
        self.selected.as_ref().filter(|s| ota::looks_like_imei(&s.name)).map(|s| s.name.clone()).unwrap_or_default()
    }

    fn firmware_alert(&mut self) -> Option<&mut Alert> {
        match &mut self.modal {
            Some(Open::Firmware(d)) => Some(&mut d.alert),
            _ => None,
        }
    }

    /// A chosen `.bin`, read and checked as an application image.
    pub(crate) fn firmware_file(&mut self, path: &Path) {
        let read = read_capped(path, ota::IMAGE_LIMIT, "a firmware image")
            .and_then(|data| aircord_core::otanb::load_firmware(&data).map(|_| data).map_err(|e| e.to_string()));
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match read {
            Ok(data) => {
                self.ota_image = Some(data);
                self.ota_file = file.clone();
                if let Some(a) = self.firmware_alert() {
                    a.ok(format!("image loaded: {file}"));
                }
            }
            Err(e) => {
                if let Some(a) = self.firmware_alert() {
                    a.fail(e);
                }
            }
        }
    }

    pub(crate) fn imei_captured(&mut self, value: Option<String>) {
        let Some(Open::Firmware(d)) = &mut self.modal else { return };
        match value {
            Some(v) => {
                d.imei = v;
                d.alert.ok("IMEI read from device");
            }
            None => d.alert.warn("no IMEI in the reply — type it from the node's label"),
        }
    }

    fn file_caption(&self) -> String {
        match &self.ota_image {
            Some(img) => format!("{} ({} bytes)", self.ota_file, img.len()),
            None => "(no image chosen)".into(),
        }
    }

    pub(crate) fn firmware_window(&mut self, ctx: &Context) {
        let caption = self.file_caption();
        let connected = self.connected();
        let Some(Open::Firmware(d)) = &mut self.modal else { return };
        let (mut choose, mut read, mut start, mut close) = (false, false, false, false);
        let r = Modal::new(Id::new("firmware")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Firmware update").heading().strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| close = icon_button(ui, Icon::Close, "Close").clicked());
            });
            ui.add_space(8.0);
            Grid::new("firmware-form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(RichText::new("Image").color(pal().muted));
                ui.horizontal(|ui| {
                    ui.add_sized([330.0, 24.0], egui::Label::new(RichText::new(&caption).monospace()).truncate());
                    choose = button(ui, Some(Icon::Sessions), "Choose .bin…", Tone::Plain).clicked();
                });
                ui.end_row();
                ui.label(RichText::new("IMEI").color(pal().muted));
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [330.0, 24.0],
                        TextEdit::singleline(&mut d.imei).hint_text("the bootloader advertises under it").font(egui::TextStyle::Monospace),
                    );
                    read = button(ui, Some(Icon::Import), "Read", Tone::Plain).on_hover_text("Ask the connected node (AT+CFG)").clicked();
                });
                ui.end_row();
                ui.label(RichText::new("Reset").color(pal().muted));
                ui.vertical(|ui| {
                    ui.radio_value(&mut d.reset_atz, true, "Send ATZ over the current connection");
                    ui.radio_value(&mut d.reset_atz, false, "I press the RESET key on the node");
                });
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Password: the PIN from the main window (bootloader v1.3+ checks it).").color(pal().muted));
            ui.add_space(6.0);
            egui::Frame::new().fill(pal().bar).corner_radius(4).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(ota::HINT).monospace().color(pal().text));
            });
            ui.add_space(6.0);
            d.alert.show(ui);
            ui.add_space(6.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                start = button(ui, Some(Icon::Firmware), "Start update", Tone::Primary).clicked();
            });
        });
        if read {
            if connected {
                self.arm_capture(CaptureFor::Imei);
                self.send_cmd(ota::READ_IMEI_CMD);
            } else if let Some(a) = self.firmware_alert() {
                a.warn("not connected — connect to the node first, or type the IMEI from its label");
            }
        }
        if choose {
            self.ask_file(ctx, rfd::AsyncFileDialog::new().add_filter("Firmware", &["bin"]), false, Then::Firmware);
        }
        if start {
            self.start_firmware();
        }
        if close || (r.should_close() && self.confirm.is_none() && self.dialog.is_none()) {
            self.modal = None;
        }
    }

    /// Checks the dialog's inputs and asks before flashing.
    pub(crate) fn start_firmware(&mut self) {
        let Some(Open::Firmware(d)) = &self.modal else { return };
        let job = ota::build_job(self.ota_image.as_deref(), &self.ota_file, &d.imei, d.reset_atz, self.connected(), &self.pin);
        match job {
            Err(e) => {
                if let Some(a) = self.firmware_alert() {
                    a.fail(e);
                }
            }
            Ok(job) => {
                self.ota_imei = job.imei.clone();
                let body = format!(
                    "This erases and rewrites the application firmware of node\n\n{}\n\nusing {}. The node will reboot. Aircord only writes the application region, so a failed attempt can be retried.",
                    job.imei, job.file
                );
                self.ask(format!("Flash {}?", job.file), body, "Flash", true, Pending::Flash(Box::new(job)));
            }
        }
    }

    /// Runs the update on a thread; the progress window follows it.
    pub(crate) fn run_firmware_update(&mut self, job: ota::Job) {
        if self.ota_running() {
            self.alert.warn("a firmware update is already running");
            return;
        }
        self.alert.clear();
        let stop = Arc::new(AtomicBool::new(false));
        self.ota = Some(Run { phase: "starting…".into(), sent: 0, total: 0, done: None, stop: Arc::clone(&stop), hidden: false });
        let (link, out, timing) = (Arc::clone(&self.link), self.outbox(), self.timing.ota);
        std::thread::spawn(move || {
            let result = ota::run(&*link, &job, timing, &stop, &mut |r| out.post(Msg::Ota(r)));
            out.post(Msg::OtaDone(result));
        });
    }

    /// Cancel: ends the bootloader wait and drops the link, which fails the next round-trip.
    pub(crate) fn cancel_firmware(&mut self) {
        let Some(run) = &self.ota else { return };
        if run.done.is_some() || run.stop.swap(true, Ordering::Relaxed) {
            return;
        }
        self.link.disconnect();
        self.log_line("-- firmware update cancelled by user --");
    }

    pub(crate) fn on_ota(&mut self, r: Report) {
        match r {
            Report::Log(l) => self.log_line(l),
            Report::Phase(p) => {
                if let Some(run) = &mut self.ota {
                    run.phase = p;
                }
            }
            Report::Progress { sent, total } => {
                if let Some(run) = &mut self.ota {
                    (run.sent, run.total) = (sent, total);
                }
            }
            Report::Disconnected(s) => self.set_disconnected(&s),
            Report::Bootloader(name) => {
                self.set_status(format!("firmware: connected to bootloader {name}"));
                self.conn = Conn::Bootloader { name };
            }
            Report::Scanning(on) => {
                if on {
                    self.set_status("firmware: waiting for the bootloader");
                }
            }
        }
    }

    pub(crate) fn ota_done(&mut self, result: Result<(), String>) {
        self.set_disconnected("firmware update finished");
        match &result {
            Ok(()) => {
                self.log_line("-- firmware update successful — node rebooting --");
                self.alert.ok("firmware update successful");
                self.set_status("firmware update successful");
            }
            Err(e) => {
                self.fail(format!("firmware update failed: {e}"));
                self.set_status("firmware update failed");
            }
        }
        if let Some(run) = &mut self.ota {
            if result.is_ok() {
                run.sent = run.total;
            }
            run.done = Some(result);
            run.hidden = false;
        }
    }

    pub(crate) fn ota_window(&mut self, ctx: &Context) {
        let Some(run) = &self.ota else { return };
        if run.hidden {
            return;
        }
        let fraction = if run.total > 0 { run.sent as f32 / run.total as f32 } else { 0.0 };
        let (mut cancel, mut close) = (false, false);
        Modal::new(Id::new("ota-progress")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(RichText::new("Firmware update").heading().strong());
            ui.add_space(8.0);
            let (text, ink) = match &run.done {
                None => (run.phase.clone(), pal().text),
                Some(Ok(())) => ("update successful — the node is rebooting into the new firmware".into(), pal().ok),
                Some(Err(e)) => (format!("failed: {e}"), pal().error),
            };
            ui.add(egui::Label::new(RichText::new(text).color(ink)).wrap());
            ui.add_space(6.0);
            let bar = ProgressBar::new(if matches!(run.done, Some(Ok(()))) { 1.0 } else { fraction }).show_percentage();
            ui.add(bar.animate(run.done.is_none() && run.total == 0));
            if run.total > 0 {
                ui.label(RichText::new(format!("{} / {} bytes", run.sent, run.total)).monospace().color(pal().muted));
            }
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if run.done.is_none() {
                    cancel = button(ui, Some(Icon::Stop), "Cancel", Tone::Danger).clicked();
                } else {
                    close = button(ui, None, "Close", Tone::Primary).clicked();
                }
            });
        });
        if cancel {
            self.cancel_firmware();
        }
        if close {
            self.ota = None;
        }
    }
}
