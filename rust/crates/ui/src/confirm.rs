//! Questions before what can't be undone: a destructive panel action, a playbook that reboots the device, a delete, a
//! replace, a flash. The question names exactly what is about to happen.

use aircord_core::ota;
use aircord_core::playbook::Playbook;
use egui::{Align, Context, Id, Layout, Modal, RichText};

use crate::App;
use crate::style::{self, pal};
use crate::widgets::{self, Tone, button};

/// What a yes does.
#[derive(Clone, Debug)]
pub(crate) enum Pending {
    Send(String),
    RunPlaybook(Playbook),
    DeletePlaybook(String),
    ReplacePlaybook(Playbook),
    DeleteSession(String),
    Flash(Box<ota::Job>),
}

pub(crate) struct Confirm {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) yes: &'static str,
    pub(crate) danger: bool,
    pub(crate) then: Pending,
}

impl App {
    pub(crate) fn ask(&mut self, title: impl Into<String>, body: impl Into<String>, yes: &'static str, danger: bool, then: Pending) {
        self.confirm = Some(Confirm { title: title.into(), body: body.into(), yes, danger, then });
    }

    pub(crate) fn confirm_window(&mut self, ctx: &Context) {
        let Some(c) = &self.confirm else { return };
        let (mut yes, mut no) = (false, false);
        // PhotoCraft's dialog: a semibold title over a hairline, the body, the answers at the bottom right.
        let r = Modal::new(Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new(&c.title).font(style::semibold(15.0)).color(pal().text));
            widgets::hairline(ui);
            ui.add_space(4.0);
            ui.label(RichText::new(&c.body).color(pal().text));
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                yes = button(ui, None, c.yes, if c.danger { Tone::Danger } else { Tone::Primary }).clicked();
                no = button(ui, None, "Cancel", Tone::Plain).clicked();
            });
        });
        if yes && let Some(c) = self.confirm.take() {
            self.confirmed(c.then);
        } else if no || r.should_close() {
            self.confirm = None;
        }
    }

    /// Does what was asked about. Tests call it to answer yes.
    pub(crate) fn confirmed(&mut self, then: Pending) {
        match then {
            Pending::Send(cmd) => self.send_cmd(&cmd),
            Pending::RunPlaybook(pb) => {
                self.modal = None;
                self.run_playbook(pb);
            }
            Pending::DeletePlaybook(name) => self.delete_playbook(&name),
            Pending::ReplacePlaybook(pb) => self.store_import(pb),
            Pending::DeleteSession(name) => self.delete_session(&name),
            Pending::Flash(job) => {
                self.modal = None;
                self.run_firmware_update(*job);
            }
        }
    }
}
