//! The Sessions dialog: save the working state under a name, load or delete a saved one. A loaded session fills the
//! list from its snapshot and the PIN; its device and template become real once a scan finds the device again.

use aircord_core::link::Device;
use aircord_core::session::{DeviceSnapshot, Session};
use egui::{Align, Context, Id, Layout, Modal, RichText, ScrollArea, TextEdit};

use crate::confirm::Pending;
use crate::icons::Icon;
use crate::style::pal;
use crate::widgets::{Alert, Tone, button, icon_button};
use crate::{App, Modal as Open};

pub(crate) struct Dialog {
    pub(crate) name: String,
    pub(crate) names: Vec<String>,
    pub(crate) alert: Alert,
}

impl App {
    pub fn open_sessions(&mut self) {
        let name = self.selected.as_ref().map_or_else(|| "session".to_string(), |s| s.name.clone());
        self.modal = Some(Open::Sessions(Dialog { name, names: self.stores.sessions.list(), alert: Alert::default() }));
    }

    /// The working state as a session named `name`.
    pub fn capture_session(&self, name: &str) -> Session {
        Session {
            name: name.to_string(),
            selected_addr: self.selected.as_ref().map(|s| s.addr.clone()).unwrap_or_default(),
            selected_name: self.selected.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            pin: self.pin.clone(),
            template: self.template.clone().unwrap_or_default(),
            devices: self.display.iter().map(|d| DeviceSnapshot { addr: d.addr.clone(), name: d.name.clone(), rssi: d.rssi }).collect(),
        }
    }

    /// Restores a session: its (stale) list for reference, the PIN, and its target and template as pending.
    pub fn apply_session(&mut self, s: Session) {
        let now = std::time::Instant::now();
        self.display = s.devices.iter().map(|d| Device { addr: d.addr.clone(), name: d.name.clone(), rssi: d.rssi, seen: now }).collect();
        self.total = self.display.len();
        self.pin = s.pin.clone();
        // A snapshot can't connect: wait for a live scan to find the device.
        self.selected = None;
        self.want_addr = Some(s.selected_addr.clone()).filter(|a| !a.is_empty());
        self.want_template = Some(s.template.clone()).filter(|t| !t.is_empty());
        let mut msg = format!("session loaded: {}", s.name);
        if !s.selected_name.is_empty() {
            msg.push_str(&format!(" — target {} (press Scan to reconnect)", s.selected_name));
        }
        self.set_status(msg.clone());
        self.alert.ok(msg.clone());
        self.log_line(format!("-- {msg} --"));
    }

    pub(crate) fn delete_session(&mut self, name: &str) {
        let result = self.stores.sessions.delete(name);
        let names = self.stores.sessions.list();
        if let Some(Open::Sessions(d)) = &mut self.modal {
            match result {
                Ok(()) => d.alert.ok(format!("deleted: {name}")),
                Err(e) => d.alert.fail(format!("delete failed: {e}")),
            }
            d.names = names;
        }
    }

    pub(crate) fn sessions_window(&mut self, ctx: &Context) {
        let Some(Open::Sessions(d)) = &mut self.modal else { return };
        let (mut save, mut load, mut delete, mut close) = (false, None, None, false);
        let r = Modal::new(Id::new("sessions")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sessions").heading().strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| close = icon_button(ui, Icon::Close, "Close").clicked());
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut d.name).hint_text("Session name").desired_width(ui.available_width() - 130.0));
                save = button(ui, Some(Icon::Save), "Save current", Tone::Primary).clicked();
            });
            d.alert.show(ui);
            ui.add_space(6.0);
            ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                if d.names.is_empty() {
                    ui.label(RichText::new("No saved sessions.").color(pal().muted));
                }
                for n in &d.names {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(n).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if button(ui, Some(Icon::Delete), "Delete", Tone::Danger).clicked() {
                                delete = Some(n.clone());
                            }
                            if button(ui, Some(Icon::Sessions), "Load", Tone::Primary).clicked() {
                                load = Some(n.clone());
                            }
                        });
                    });
                    ui.separator();
                }
            });
        });
        if save {
            let name = d.name.trim().to_string();
            if name.is_empty() {
                d.alert.warn("enter a name first");
            } else {
                let session = self.capture_session(&name);
                let result = self.stores.sessions.save(&session);
                let names = self.stores.sessions.list();
                if let Some(Open::Sessions(d)) = &mut self.modal {
                    match result {
                        Ok(()) => d.alert.ok(format!("saved: {name}")),
                        Err(e) => d.alert.fail(format!("save failed: {e}")),
                    }
                    d.names = names;
                }
            }
        }
        if let Some(n) = load {
            match self.stores.sessions.load(&n) {
                Ok(s) => {
                    // The restored list is behind this dialog.
                    self.modal = None;
                    self.apply_session(s);
                }
                Err(e) => {
                    if let Some(Open::Sessions(d)) = &mut self.modal {
                        d.alert.fail(format!("load failed: {e}"));
                    }
                }
            }
        }
        if let Some(n) = delete {
            self.ask(
                "Delete session?",
                format!("Delete the saved session \"{n}\"?\n\nThis cannot be undone."),
                "Delete",
                true,
                Pending::DeleteSession(n),
            );
        }
        if close || (r.should_close() && self.confirm.is_none()) {
            self.modal = None;
        }
    }

    pub(crate) fn modal_window(&mut self, ctx: &Context) {
        match &self.modal {
            Some(Open::Sessions(_)) => self.sessions_window(ctx),
            Some(Open::Playbooks(_)) => self.playbooks_window(ctx),
            Some(Open::Firmware(_)) => self.firmware_window(ctx),
            None => {}
        }
    }
}
