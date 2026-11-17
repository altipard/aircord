//! Transferring the form's changes: the bar under the form, and the dialog that first shows every change as before
//! and after, then the progress, then what became of each one. The sending itself is
//! [`aircord_core::transfer::apply`] on a thread of its own; a restart is only ever offered, never sent unasked.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aircord_core::device::Template;
use aircord_core::settings::Risk;
use aircord_core::transfer::{self, Change, Outcome, Progress, Status};
use egui::{Align, Context, Grid, Id, Label, Layout, Modal, RichText, ScrollArea, Ui};

use crate::confirm::Pending;
use crate::icons::Icon;
use crate::inbox::Msg;
use crate::style::{self, pal};
use crate::widgets::{self, Alert, Mark, Tone, button, button_named};
use crate::{App, View};

pub(crate) enum Stage {
    /// Before and after, waiting for the go.
    Review,
    Running {
        /// What is happening, in a sentence.
        step: String,
        /// How far, 0 to 1.
        part: f32,
    },
    Done(Outcome),
}

pub(crate) struct Transfer {
    template: Arc<Template>,
    pub(crate) changes: Vec<Change>,
    pub(crate) stage: Stage,
    stop: Arc<AtomicBool>,
}

/// A value as the dialog shows it.
fn shown(tpl: &Template, key: &str, value: &str) -> String {
    if value.trim().is_empty() {
        return "leer".into();
    }
    tpl.setting(key).map_or_else(|| value.to_string(), |s| s.shown(value))
}

/// A change's two sides. A secret shows neither.
fn sides(tpl: &Template, c: &Change) -> (String, String) {
    if c.secret {
        let to = if c.to.is_empty() { "wird gelöscht" } else { "neuer Wert (verdeckt)" };
        return ("verdeckt".into(), to.into());
    }
    (shown(tpl, &c.key, &c.from), shown(tpl, &c.key, &c.to))
}

/// What to say before a change of this risk goes out.
fn risk_text(risk: Risk) -> Option<&'static str> {
    match risk {
        Risk::None => None,
        Risk::Network => Some("Das Gerät kann danach das Netz verlieren. Ein falscher Wert lässt sich dann nur noch vor Ort beheben."),
        Risk::Reboot => Some("Das Gerät startet dabei neu."),
        Risk::Irreversible => Some("Diese Änderung lässt sich nicht rückgängig machen."),
    }
}

/// A field's result: its mark and its sentence.
pub(crate) fn result_text(tpl: &Template, key: &str, status: &Status) -> (Mark, String) {
    match status {
        Status::Verified => (Mark::Ok, "übernommen".into()),
        Status::Failed { why } => (Mark::Refused, format!("abgelehnt – {why}")),
        Status::Differs { actual } => (Mark::Refused, format!("abgelehnt – das Gerät meldet weiterhin: {}", shown(tpl, key, actual))),
        Status::Unverified { why } => (Mark::Unsure, format!("nicht bestätigt – {why}")),
    }
}

fn count_text(n: usize) -> String {
    match n {
        0 => "Keine Änderungen".into(),
        1 => "1 Änderung".into(),
        n => format!("{n} Änderungen"),
    }
}

impl App {
    pub(crate) fn transfer_running(&self) -> bool {
        matches!(self.transfer, Some(Transfer { stage: Stage::Running { .. }, .. }))
    }

    /// The bar under the form: how many settings differ, and the way to send them.
    pub(crate) fn transfer_bar(&mut self, ui: &mut Ui) {
        let Some(form) = &self.guided.form else { return };
        let (n, valid) = form.summary();
        let free = !self.transfer_running();
        // Only what was just read is a "before" to show and to compare against.
        let fresh = self.guided.read == crate::guided::Read::Ready;
        let (mut go, mut reset) = (false, false);
        ui.horizontal(|ui| {
            ui.set_min_height(30.0);
            ui.label(RichText::new(count_text(n)).font(style::medium(13.0)).color(if n > 0 { pal().text } else { pal().muted }));
            if !fresh {
                ui.label(RichText::new("– bitte zuerst die Einstellungen neu lesen").color(pal().warn));
            } else if !valid {
                ui.label(RichText::new("– bitte zuerst die rot markierten Eingaben berichtigen").color(pal().error));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can = n > 0 && valid && free && fresh;
                let tip = if !fresh {
                    "Die Einstellungen des Geräts sind nicht aktuell. Oben rechts neu lesen."
                } else if n == 0 {
                    "Erst etwas ändern, dann übertragen"
                } else {
                    "Zeigt die Änderungen und sendet sie nach Bestätigung"
                };
                go = ui
                    .add_enabled_ui(can, |ui| button(ui, Some(Icon::Play), "Übertragen", Tone::Primary))
                    .inner
                    .on_hover_text(tip)
                    .on_disabled_hover_text(tip)
                    .clicked();
                if n > 0 {
                    reset = ui
                        .add_enabled_ui(free, |ui| button(ui, Some(Icon::Undo), "Alle zurücksetzen", Tone::Plain))
                        .inner
                        .on_hover_text("Alle Eingaben verwerfen")
                        .clicked();
                }
            });
        });
        if reset && let Some(form) = &mut self.guided.form {
            form.reset_all();
        }
        if go {
            self.review_transfer();
        }
    }

    /// Opens the dialog on what the form would send.
    pub(crate) fn review_transfer(&mut self) {
        let Some(form) = &self.guided.form else { return };
        if self.guided.read != crate::guided::Read::Ready {
            self.guided.notice.warn("Bitte zuerst die Einstellungen neu lesen.");
            return;
        }
        match form.plan() {
            Ok(changes) if !changes.is_empty() => {
                self.guided.notice.clear();
                self.transfer = Some(Transfer {
                    template: Arc::clone(&form.template),
                    changes,
                    stage: Stage::Review,
                    stop: Arc::new(AtomicBool::new(false)),
                });
            }
            Ok(_) => self.guided.notice.warn("Es gibt nichts zu übertragen."),
            Err(errors) => {
                let what: Vec<String> = errors.iter().map(|e| format!("{}: {}", e.label, e.message)).collect();
                self.guided.notice.warn(what.join(" "));
            }
        }
    }

    /// Sends the reviewed changes, on a thread of its own.
    pub(crate) fn start_transfer(&mut self) {
        if !matches!(self.transfer, Some(Transfer { stage: Stage::Review, .. })) {
            return;
        }
        let busy = self.running.is_some() || self.ota_running() || self.guided.read == crate::guided::Read::Reading;
        if busy || !self.link.is_connected() {
            self.transfer = None;
            self.guided.notice.warn(if busy {
                "Das Gerät ist gerade beschäftigt. Bitte gleich noch einmal übertragen."
            } else {
                "Keine Verbindung zum Gerät. Bitte neu verbinden."
            });
            return;
        }
        let Some(t) = &mut self.transfer else { return };
        t.stage = Stage::Running { step: "Die Übertragung beginnt …".into(), part: 0.0 };
        let (tpl, changes, stop) = (Arc::clone(&t.template), t.changes.clone(), Arc::clone(&t.stop));
        let (link, out, wait) = (Arc::clone(&self.link), self.outbox(), self.timing.read_wait);
        self.log_line(format!("-- transfer: {} setting(s) --", changes.len()));
        std::thread::spawn(move || {
            let outcome = transfer::apply(&*link, &tpl, &changes, wait, &stop, &mut |p| out.post(Msg::Transfer(p)));
            out.post(Msg::TransferDone(outcome));
        });
    }

    pub(crate) fn on_transfer(&mut self, p: Progress) {
        let Some(Transfer { stage: stage @ Stage::Running { .. }, .. }) = &mut self.transfer else { return };
        *stage = match p {
            Progress::Sending { n, total, label, .. } => {
                Stage::Running { step: format!("{n} von {total}: {label}"), part: (n.saturating_sub(1)) as f32 / (total.max(1) + 1) as f32 }
            }
            Progress::Verifying => {
                Stage::Running { step: "Das Gerät wird noch einmal gelesen, um alles zu prüfen …".into(), part: 0.9 }
            }
        };
    }

    pub(crate) fn on_transfer_done(&mut self, outcome: Outcome) {
        let ok = outcome.fields.iter().filter(|f| f.status == Status::Verified).count();
        self.log_line(format!("-- transfer finished: {ok} of {} confirmed --", outcome.fields.len()));
        if let Some(form) = &mut self.guided.form {
            form.absorb(&outcome);
        }
        if let Some(t) = &mut self.transfer {
            t.stage = Stage::Done(outcome);
        }
    }

    /// Sends the template's restart, after the person said yes in the result dialog: that question is the confirmation.
    pub(crate) fn restart_device(&mut self, cmd: String) {
        self.restarting = true;
        self.confirmed(Pending::Send(cmd));
        self.guided.notice.ok("Das Gerät startet neu. Die Verbindung endet dabei – danach bitte neu verbinden.");
    }

    pub(crate) fn transfer_window(&mut self, ctx: &Context) {
        let Some(t) = &self.transfer else { return };
        let (mut go, mut close, mut stop, mut restart) = (false, false, false, None);
        let r = Modal::new(Id::new("transfer")).show(ctx, |ui| {
            ui.set_width(520.0);
            let title = match &t.stage {
                Stage::Review => "Änderungen übertragen?",
                Stage::Running { .. } => "Wird übertragen …",
                Stage::Done(_) => "Ergebnis",
            };
            ui.label(RichText::new(title).font(style::semibold(15.0)).color(pal().text));
            widgets::hairline(ui);
            ui.add_space(4.0);
            match &t.stage {
                Stage::Review => {
                    ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        Grid::new("transfer-review").num_columns(3).spacing([18.0, 6.0]).show(ui, |ui| {
                            for head in ["Einstellung", "Vorher", "Nachher"] {
                                ui.label(RichText::new(head).font(style::medium(11.5)).color(pal().muted));
                            }
                            ui.end_row();
                            for c in &t.changes {
                                let (from, to) = sides(&t.template, c);
                                ui.label(RichText::new(&c.label).font(style::medium(12.5)).color(pal().text));
                                ui.label(RichText::new(from).color(pal().muted));
                                ui.label(RichText::new(to).color(pal().text));
                                ui.end_row();
                            }
                        });
                    });
                    let mut risks: Vec<Risk> = t.changes.iter().map(|c| c.risk).filter(|r| risk_text(*r).is_some()).collect();
                    risks.dedup();
                    for risk in risks {
                        let names: Vec<&str> = t.changes.iter().filter(|c| c.risk == risk).map(|c| c.label.as_str()).collect();
                        ui.add_space(6.0);
                        let mut strip = Alert::default();
                        strip.warn(format!("{}: {}", names.join(", "), risk_text(risk).unwrap_or_default()));
                        strip.show(ui);
                    }
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        go = button_named(ui, Some(Icon::Play), "Übertragen", "Jetzt übertragen", Tone::Primary).clicked();
                        close = button(ui, None, "Abbrechen", Tone::Plain).clicked();
                    });
                }
                Stage::Running { step, part } => {
                    ui.add(Label::new(RichText::new(step).color(pal().text)).wrap());
                    ui.add_space(6.0);
                    ui.add(egui::ProgressBar::new(*part).desired_height(6.0));
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        stop = button(ui, Some(Icon::Stop), "Anhalten", Tone::Plain)
                            .on_hover_text("Was schon gesendet wurde, bleibt im Gerät")
                            .clicked();
                    });
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                }
                Stage::Done(outcome) => {
                    ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for f in &outcome.fields {
                            let (mark, text) = result_text(&t.template, &f.key, &f.status);
                            ui.horizontal_top(|ui| {
                                widgets::mark(ui, mark);
                                ui.add(Label::new(RichText::new(format!("{}: {text}", f.label)).color(pal().text)).wrap());
                            });
                        }
                    });
                    let reboot = outcome.reboot_needed.then(|| t.template.reboot_action()).flatten().filter(|_| self.link.is_connected());
                    ui.add_space(10.0);
                    if let Some(action) = reboot {
                        widgets::hairline(ui);
                        ui.add_space(4.0);
                        ui.label(RichText::new("Neu starten, damit alle Änderungen wirken?").font(style::medium(13.0)).color(pal().text));
                        ui.add(
                            Label::new(RichText::new("Beim Neustart endet die Verbindung. Danach bitte neu verbinden.").color(pal().muted))
                                .wrap(),
                        );
                        ui.add_space(10.0);
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            if button(ui, None, "Ja, neu starten", Tone::Primary).clicked() {
                                restart = Some(action.cmd.clone());
                            }
                            close = button(ui, None, "Später", Tone::Plain).clicked();
                        });
                    } else {
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            close = button(ui, None, "Schließen", Tone::Primary).clicked();
                        });
                    }
                }
            }
        });
        let running = matches!(t.stage, Stage::Running { .. });
        if stop {
            t.stop.store(true, Ordering::Relaxed);
        }
        if go {
            self.start_transfer();
        } else if let Some(cmd) = restart {
            self.transfer = None;
            self.restart_device(cmd);
        } else if close || (r.should_close() && !running) {
            self.transfer = None;
        }
    }

    /// Leaves the guided view for the expert one, or back.
    pub(crate) fn set_view(&mut self, view: View) {
        self.view = view;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_results_read_as_sentences() {
        assert_eq!([count_text(0), count_text(1), count_text(3)], ["Keine Änderungen", "1 Änderung", "3 Änderungen"]);
        let tpl = aircord_core::device::Templates::load_from(std::path::Path::new("/nonexistent")).get("Dragino S31-NB").cloned().unwrap();
        assert_eq!(result_text(&tpl, "TDC", &Status::Verified), (Mark::Ok, "übernommen".to_string()));
        let (mark, text) = result_text(&tpl, "TDC", &Status::Differs { actual: "900".into() });
        assert_eq!((mark, text.as_str()), (Mark::Refused, "abgelehnt – das Gerät meldet weiterhin: 15 Minuten"));
        assert_eq!(result_text(&tpl, "TDC", &Status::Unverified { why: "Nicht erneut gelesen.".into() }).0, Mark::Unsure);
        assert_eq!(shown(&tpl, "APN", ""), "leer");
    }
}
