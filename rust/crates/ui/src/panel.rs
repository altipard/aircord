//! The control panel: a device template's actions as labelled controls, so a user need not know the AT syntax. It is
//! empty until a device is connected; the template is suggested from the device's name and can be changed by hand.

use aircord_core::device::{Action, Kind};
use egui::{Align, ComboBox, Grid, Layout, RichText, TextEdit, Ui};

use crate::style::pal;
use crate::widgets::{self, Tone, button_named};
use crate::{App, Capture, CaptureFor};

const NO_TEMPLATE: &str = "— no template —";

impl App {
    /// On connect: a loaded session's template wins, else the first that fits the name.
    pub(crate) fn apply_template_for(&mut self, device_name: &str) {
        let want = self.want_template.take().filter(|t| self.stores.templates.get(t).is_some());
        let name = want.or_else(|| self.stores.templates.suggest(device_name).map(|t| t.name.clone()));
        self.set_template(name);
    }

    /// Sends an action's command, asking first when the template marks it destructive: a panel is a column of similar
    /// buttons, and one stray click can reboot or wipe a sensor already mounted in the field.
    pub(crate) fn send_action(&mut self, a: &Action, cmd: String) {
        if a.confirm {
            let body = format!("This sends:\n\n{cmd}\n\nThe template marks this action as destructive.");
            self.ask(format!("{}?", a.label), body, "Send", true, crate::confirm::Pending::Send(cmd));
        } else {
            self.send_cmd(&cmd);
        }
    }

    /// Waits for the next reply line a matcher finds a value in, at most the reply wait.
    pub(crate) fn arm_capture(&mut self, target: CaptureFor) {
        self.capture = Some(Capture { target, until: self.now + self.timing.reply_wait.as_secs_f64() });
    }

    /// Called for each reply line: the first value found is delivered and the capture ends.
    pub(crate) fn try_capture(&mut self, line: &str) {
        let Some(c) = &self.capture else { return };
        let value = match &c.target {
            CaptureFor::Action(i) => self.shown_action(*i).and_then(|a| a.extract(line)),
            CaptureFor::Imei => aircord_core::ota::extract_imei(line),
        };
        if let Some(v) = value {
            let target = c.target.clone();
            self.capture = None;
            self.captured(target, Some(v));
        }
    }

    pub(crate) fn captured(&mut self, target: CaptureFor, value: Option<String>) {
        match target {
            CaptureFor::Action(i) => {
                self.values.insert(i, value.unwrap_or_else(|| "(no reply)".into()));
            }
            CaptureFor::Imei => self.imei_captured(value),
        }
    }

    fn shown_action(&self, i: usize) -> Option<&Action> {
        self.template.as_ref().and_then(|t| self.stores.templates.get(t)).and_then(|t| t.actions.get(i))
    }

    pub(crate) fn control_panel(&mut self, ui: &mut Ui) {
        widgets::section(ui, "CONTROL PANEL");
        ui.add_space(4.0);
        let connected = self.connected();
        let names = self.stores.templates.names();
        let mut pick = None;
        ui.add_enabled_ui(connected, |ui| {
            ComboBox::from_id_salt("template")
                .width(ui.available_width())
                .selected_text(self.template.clone().unwrap_or_else(|| NO_TEMPLATE.into()))
                .show_ui(ui, |ui| {
                    if ui.selectable_label(self.template.is_none(), NO_TEMPLATE).clicked() {
                        pick = Some(None);
                    }
                    for n in &names {
                        if ui.selectable_label(self.template.as_deref() == Some(n), n).clicked() {
                            pick = Some(Some(n.clone()));
                        }
                    }
                });
        });
        if let Some(p) = pick {
            self.set_template(p);
        }
        ui.add_space(10.0);

        if !connected {
            ui.label(RichText::new("Connect a device to use its control panel.").color(pal().muted));
            return;
        }
        let Some(tpl) = self.template.as_ref().and_then(|t| self.stores.templates.get(t)).cloned() else {
            ui.label(RichText::new("No template — pick one above, or add one to the templates folder.").color(pal().muted));
            return;
        };
        let mut send: Option<(Action, String)> = None;
        let mut read: Option<(usize, Action)> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            Grid::new("actions").num_columns(2).spacing([10.0, 8.0]).min_col_width(90.0).show(ui, |ui| {
                for (i, a) in tpl.actions.iter().enumerate() {
                    ui.add(egui::Label::new(RichText::new(&a.label).color(pal().text)).wrap());
                    let tone = if a.confirm { Tone::Danger } else { Tone::Plain };
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| match a.kind {
                        Kind::Button => {
                            if button_named(ui, None, "Send", &format!("Send: {}", a.label), tone).on_hover_text(&a.cmd).clicked() {
                                send = Some((a.clone(), a.cmd.clone()));
                            }
                        }
                        Kind::Input => {
                            let value = self.inputs.entry(i).or_default();
                            let edit = ui.add_sized([110.0, 24.0], TextEdit::singleline(value).hint_text("value"));
                            edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, format!("Value: {}", a.label)));
                            let enter = edit.lost_focus() && ui.input(|inp| inp.key_pressed(egui::Key::Enter));
                            if button_named(ui, None, "Set", &format!("Set: {}", a.label), tone).on_hover_text(&a.cmd).clicked() || enter {
                                send = Some((a.clone(), a.render(value.trim())));
                            }
                        }
                        Kind::Select => {
                            ComboBox::from_id_salt(("select", i)).selected_text("choose…").width(150.0).show_ui(ui, |ui| {
                                for o in &a.options {
                                    if ui.selectable_label(false, &o.text).on_hover_text(&o.cmd).clicked() {
                                        send = Some((a.clone(), o.cmd.clone()));
                                    }
                                }
                            });
                        }
                        Kind::Read => {
                            let value = self.values.get(&i).cloned().unwrap_or_else(|| "—".into());
                            ui.add_sized([110.0, 24.0], egui::Label::new(RichText::new(value).monospace()));
                            if button_named(ui, None, "Read", &format!("Read: {}", a.label), tone).on_hover_text(&a.cmd).clicked() {
                                read = Some((i, a.clone()));
                            }
                        }
                    });
                    ui.end_row();
                }
            });
        });
        if let Some((i, a)) = read {
            self.values.insert(i, "…".into());
            self.arm_capture(CaptureFor::Action(i));
            self.send_action(&a, a.cmd.clone());
        }
        if let Some((a, cmd)) = send {
            self.send_action(&a, cmd);
        }
    }
}
