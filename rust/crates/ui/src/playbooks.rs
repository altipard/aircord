//! The Playbooks dialog: the saved ones (Run, Edit, Export, Delete, and Import below them) and the editor, as two tabs,
//! so the dialog is as big as the larger of the two, not their sum. Running closes the dialog: the run reports into the
//! terminal, which it would otherwise cover.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use aircord_core::playbook::{self, Playbook};
use egui::{Align, Context, Id, Layout, Modal, RichText, ScrollArea, TextEdit};

use crate::confirm::Pending;
use crate::dialog::{Then, read_capped};
use crate::icons::Icon;
use crate::inbox::Msg;
use crate::style::pal;
use crate::widgets::{Alert, Tone, button, icon_button, tab};
use crate::{App, Modal as Open};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Saved,
    Editor,
}

pub(crate) struct Dialog {
    pub(crate) tab: Tab,
    pub(crate) name: String,
    pub(crate) text: String,
    /// Each saved playbook's name and step count.
    pub(crate) rows: Vec<(String, usize)>,
    pub(crate) alert: Alert,
}

impl App {
    pub fn open_playbooks(&mut self) {
        let mut d = Dialog { tab: Tab::Saved, name: String::new(), text: String::new(), rows: Vec::new(), alert: Alert::default() };
        self.refresh_playbooks(&mut d);
        self.modal = Some(Open::Playbooks(d));
    }

    fn refresh_playbooks(&self, d: &mut Dialog) {
        let s = &self.stores.playbooks;
        d.rows = s.list().into_iter().map(|n| (n.clone(), s.load(&n).map_or(0, |p| p.steps.len()))).collect();
    }

    fn playbooks_alert(&mut self) -> Option<&mut Alert> {
        match &mut self.modal {
            Some(Open::Playbooks(d)) => Some(&mut d.alert),
            _ => None,
        }
    }

    fn with_playbooks(&mut self, f: impl FnOnce(&App, &mut Dialog)) {
        if let Some(Open::Playbooks(mut d)) = self.modal.take() {
            f(self, &mut d);
            self.modal = Some(Open::Playbooks(d));
        }
    }

    /// The dialog's message, or the window's when it was closed meanwhile.
    fn playbook_note(&mut self, ok: bool, msg: String) {
        let refresh = ok;
        match self.playbooks_alert() {
            Some(a) if ok => a.ok(msg),
            Some(a) => a.fail(msg),
            None if ok => self.alert.ok(msg),
            None => self.fail(msg),
        }
        if refresh {
            self.with_playbooks(|app, d| app.refresh_playbooks(d));
        }
    }

    /// Confirms a playbook that reboots or resets the device, naming the commands, then runs it.
    pub fn start_playbook(&mut self, pb: Playbook) {
        let risky = playbook::risky_steps(&pb.steps);
        if risky.is_empty() {
            self.modal = None;
            self.run_playbook(pb);
            return;
        }
        let body = format!(
            "This playbook contains commands that reboot or reset the device:\n\n{}\n\nA device already deployed in the field will drop its link and restart.",
            risky.join("\n")
        );
        self.ask(format!("Run \"{}\"?", pb.name), body, "Run anyway", true, Pending::RunPlaybook(pb));
    }

    /// Replays the steps on a thread of its own; a step with a reply pattern waits for it, or the run stops there.
    pub fn run_playbook(&mut self, pb: Playbook) {
        if !self.connected() || !self.link.is_connected() {
            self.alert.warn("not connected — connect to a device first");
            return;
        }
        if self.ota_running() {
            self.alert.warn("a firmware update is running — the link belongs to the bootloader");
            return;
        }
        if self.running.is_some() {
            self.alert.warn("a playbook is already running");
            return;
        }
        if pb.steps.is_empty() {
            self.alert.warn("playbook has no steps");
            return;
        }
        self.set_status(format!("playbook running: {}", pb.name));
        self.alert.clear();
        let stop = Arc::new(AtomicBool::new(false));
        self.running = Some((pb.name.clone(), Arc::clone(&stop)));
        let (link, out, wait) = (Arc::clone(&self.link), self.outbox(), self.timing.reply_wait);
        std::thread::spawn(move || {
            let result = playbook::run(&*link, &pb, wait, &stop, &mut |r| out.post(Msg::Playbook(r)));
            out.post(Msg::PlaybookDone { name: pb.name.clone(), result });
        });
    }

    pub(crate) fn delete_playbook(&mut self, name: &str) {
        match self.stores.playbooks.delete(name) {
            Ok(()) => self.playbook_note(true, format!("deleted: {name}")),
            Err(e) => self.playbook_note(false, format!("delete failed: {e}")),
        }
    }

    /// An imported file, checked; one whose name is taken is stored only after a yes.
    pub(crate) fn import_playbook_file(&mut self, path: &Path) {
        let parsed = read_capped(path, playbook::IMPORT_LIMIT, "a playbook").and_then(|data| playbook::decode(&data));
        let pb = match parsed {
            Ok(pb) => pb,
            Err(e) => return self.playbook_note(false, format!("import failed: {e}")),
        };
        if self.stores.playbooks.exists(&pb.name) {
            let body = format!("A playbook named \"{}\" already exists.\n\nReplace it with the imported one?", pb.name);
            self.ask("Replace playbook?", body, "Replace", true, Pending::ReplacePlaybook(pb));
        } else {
            self.store_import(pb);
        }
    }

    /// Stores an import through the store, not by copying the file, so its file name always matches its name.
    pub(crate) fn store_import(&mut self, pb: Playbook) {
        match self.stores.playbooks.save(&pb) {
            Ok(()) => self.playbook_note(true, format!("imported: {}  ·  {} steps", pb.name, pb.steps.len())),
            Err(e) => self.playbook_note(false, format!("import failed: {e}")),
        }
    }

    pub(crate) fn export_playbook_to(&mut self, name: &str, path: &Path) {
        let written = self.stores.playbooks.load(name).and_then(|pb| std::fs::write(path, playbook::encode(&pb)));
        match written {
            Ok(()) => {
                let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                self.playbook_note(true, format!("exported: {name} → {file}"));
            }
            Err(e) => self.playbook_note(false, format!("export failed: {e}")),
        }
    }

    fn save_edited(&mut self) {
        let Some(Open::Playbooks(d)) = &mut self.modal else { return };
        let name = d.name.trim().to_string();
        if name.is_empty() {
            return d.alert.warn("enter a name first");
        }
        let pb = Playbook { name: name.clone(), steps: playbook::parse_steps(&d.text) };
        if pb.steps.is_empty() {
            return d.alert.warn("no steps — add at least one command");
        }
        match self.stores.playbooks.save(&pb) {
            Ok(()) => self.playbook_note(true, format!("saved: {name}")),
            Err(e) => self.playbook_note(false, format!("save failed: {e}")),
        }
    }

    pub(crate) fn playbooks_window(&mut self, ctx: &Context) {
        let Some(Open::Playbooks(d)) = &mut self.modal else { return };
        enum Do {
            Run(String),
            Edit(String),
            Export(String),
            Delete(String),
            Import,
            Save,
            Close,
        }
        let mut act = None;
        let r = Modal::new(Id::new("playbooks")).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Playbooks").heading().strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, "Close").clicked() {
                        act = Some(Do::Close);
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if tab(ui, "Saved", d.tab == Tab::Saved).clicked() {
                    d.tab = Tab::Saved;
                }
                if tab(ui, "Editor", d.tab == Tab::Editor).clicked() {
                    d.tab = Tab::Editor;
                }
            });
            ui.separator();
            // Outside the tabs, so a message raised on one is still there after switching.
            d.alert.show(ui);
            ui.add_space(4.0);
            match d.tab {
                Tab::Saved => {
                    ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                        if d.rows.is_empty() {
                            ui.label(RichText::new("No playbooks yet — create one in the Editor tab.").color(pal().muted));
                        }
                        for (name, steps) in &d.rows {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(name).strong());
                                ui.label(RichText::new(format!("{steps} steps")).color(pal().muted));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if button(ui, Some(Icon::Delete), "Delete", Tone::Danger).clicked() {
                                        act = Some(Do::Delete(name.clone()));
                                    }
                                    if button(ui, Some(Icon::Export), "Export", Tone::Plain).clicked() {
                                        act = Some(Do::Export(name.clone()));
                                    }
                                    if button(ui, Some(Icon::Edit), "Edit", Tone::Plain).clicked() {
                                        act = Some(Do::Edit(name.clone()));
                                    }
                                    if button(ui, Some(Icon::Play), "Run", Tone::Primary).clicked() {
                                        act = Some(Do::Run(name.clone()));
                                    }
                                });
                            });
                            ui.separator();
                        }
                    });
                    ui.add_space(6.0);
                    if button(ui, Some(Icon::Import), "Import…", Tone::Plain).on_hover_text("Read a playbook from a .json file").clicked()
                    {
                        act = Some(Do::Import);
                    }
                }
                Tab::Editor => {
                    ui.add(TextEdit::singleline(&mut d.name).hint_text("Playbook name").desired_width(f32::INFINITY));
                    ui.add_space(4.0);
                    // Shown for as long as the editor is open: a placeholder vanishes at the first keystroke, just when it is needed.
                    ui.label(RichText::new(playbook::SYNTAX).monospace().color(pal().muted));
                    ui.add_space(4.0);
                    ui.add(
                        TextEdit::multiline(&mut d.text)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(10)
                            .desired_width(f32::INFINITY)
                            .hint_text("AT+CFG\nAT+APN=iot.1nce.net ?OK\nAT+TDC=3600 ? @1500"),
                    );
                    ui.add_space(6.0);
                    if button(ui, Some(Icon::Save), "Save", Tone::Primary).clicked() {
                        act = Some(Do::Save);
                    }
                }
            }
        });
        if r.should_close() && act.is_none() && self.confirm.is_none() && self.dialog.is_none() {
            act = Some(Do::Close);
        }
        let load = |app: &App, name: &str| app.stores.playbooks.load(name);
        match act {
            Some(Do::Close) => self.modal = None,
            Some(Do::Run(n)) => match load(self, &n) {
                Ok(pb) => self.start_playbook(pb),
                Err(e) => self.playbook_note(false, format!("load failed: {e}")),
            },
            Some(Do::Edit(n)) => match load(self, &n) {
                Ok(pb) => {
                    if let Some(Open::Playbooks(d)) = &mut self.modal {
                        d.name = pb.name.clone();
                        d.text = playbook::format_steps(&pb.steps);
                        d.tab = Tab::Editor;
                        d.alert.ok(format!("editing: {}", pb.name));
                    }
                }
                Err(e) => self.playbook_note(false, format!("load failed: {e}")),
            },
            Some(Do::Export(n)) => {
                let dialog = rfd::AsyncFileDialog::new().set_file_name(playbook::file_name(&n)).add_filter("Playbook", &["json"]);
                self.ask_file(ctx, dialog, true, Then::ExportPlaybook(n));
            }
            Some(Do::Delete(n)) => self.ask(
                "Delete playbook?",
                format!("Delete the playbook \"{n}\"?\n\nThis cannot be undone."),
                "Delete",
                true,
                Pending::DeletePlaybook(n),
            ),
            Some(Do::Import) => {
                self.ask_file(ctx, rfd::AsyncFileDialog::new().add_filter("Playbook", &["json"]), false, Then::ImportPlaybook)
            }
            Some(Do::Save) => self.save_edited(),
            None => {}
        }
    }
}
