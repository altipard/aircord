//! Pieces every part of the window uses: the buttons (BIMCraft's drawn by hand, so they carry an icon and a focus
//! ring), the alert strip, tabs.

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType, vec2};

use crate::icons::{self, Icon};
use crate::style::pal;

/// How a button stands out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// The usual flat tool button.
    Plain,
    /// The one primary action of its area: accent fill.
    Primary,
    /// A destructive action: it reads as one before it is clicked, not only in the confirmation.
    Danger,
}

/// A button with an icon before its label, named for screen readers and tests by the label.
pub fn button(ui: &mut Ui, icon: Option<Icon>, label: &str, tone: Tone) -> Response {
    button_named(ui, icon, label, label, tone)
}

/// [`button`] whose accessible name says more than its short label: a panel's "Send" is "Send: Reboot device".
pub fn button_named(ui: &mut Ui, icon: Option<Icon>, label: &str, name: &str, tone: Tone) -> Response {
    let font = FontId::proportional(13.0);
    let text_w = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size().x;
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let (rect, r) = ui.allocate_exact_size(vec2(text_w + icon_w + 20.0, 28.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name));
    let hovered = r.hovered() && enabled;
    let pressed = r.is_pointer_button_down_on() && enabled;
    let (fill, stroke, ink) = match tone {
        Tone::Primary => {
            let fill = if pressed {
                pal().accent.gamma_multiply(0.85)
            } else if hovered {
                pal().hover
            } else {
                pal().accent
            };
            (fill, Stroke::NONE, Color32::WHITE)
        }
        Tone::Danger => {
            let fill = if hovered { pal().error_fill } else { Color32::TRANSPARENT };
            (fill, Stroke::new(1.0, pal().error.gamma_multiply(0.6)), pal().error)
        }
        Tone::Plain => {
            let (fill, stroke) = match (pressed, hovered) {
                (true, _) => (pal().select_fill, Stroke::new(1.0, pal().accent)),
                (false, true) => (pal().hover_fill, Stroke::new(1.0, pal().hover)),
                (false, false) => (pal().bar, Stroke::new(1.0, pal().border)),
            };
            (fill, stroke, pal().text)
        }
    };
    // Disabled, every tone looks the same quiet way: a washed-out accent reads as broken, not as unavailable.
    let (fill, stroke, ink) =
        if enabled { (fill, stroke, ink) } else { (pal().bar, Stroke::new(1.0, pal().border), pal().muted.gamma_multiply(0.7)) };
    let stroke = if r.has_focus() { Stroke::new(2.0, pal().accent) } else { stroke };
    let p = ui.painter();
    p.rect(rect, 4.0, fill, stroke, StrokeKind::Inside);
    let mut x = rect.left() + 10.0;
    if let Some(icon) = icon {
        icons::paint(p, icon, Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), vec2(16.0, 16.0)), ink);
        x += icon_w;
    }
    p.text(egui::pos2(x, rect.center().y), Align2::LEFT_CENTER, label, font, ink);
    r
}

/// A small square button: an icon alone, its name in its tooltip and for screen readers.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(28.0, 26.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if r.hovered() && enabled {
        ui.painter().rect_filled(rect, 4.0, pal().hover_fill);
    }
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    }
    let ink = if enabled { pal().icon } else { pal().icon.gamma_multiply(0.35) };
    icons::paint(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(17.0, 17.0)), ink);
    r.on_hover_text(label)
}

/// A tab (BIMCraft's): the open one sits on the surface it opens, the others are flat.
pub fn tab(ui: &mut Ui, label: &str, active: bool) -> Response {
    let font = FontId::proportional(13.0);
    let size = ui.painter().layout_no_wrap(label.to_string(), font.clone(), pal().text).size();
    let (rect, r) = ui.allocate_exact_size(size + vec2(24.0, 10.0), Sense::click());
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, label));
    let (fill, ink) = match (active, r.hovered()) {
        (true, _) => (pal().surface, pal().text),
        (false, true) => (pal().hover_fill, pal().text),
        (false, false) => (Color32::TRANSPARENT, pal().muted),
    };
    let p = ui.painter();
    let radius = CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 };
    p.rect(rect, radius, fill, if active { Stroke::new(1.0, pal().border) } else { Stroke::NONE }, StrokeKind::Inside);
    if active {
        p.line_segment([rect.left_top() + vec2(2.0, 1.0), rect.right_top() + vec2(-2.0, 1.0)], Stroke::new(2.0, pal().accent));
    }
    if r.has_focus() {
        p.rect_stroke(rect, radius, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    }
    p.text(rect.center(), Align2::CENTER_CENTER, label, font, ink);
    r
}

/// A section's title: small caps-like muted heading, as BIMCraft's palettes have.
pub fn section(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).strong().size(12.0).color(pal().muted));
}

/// How bad an alert is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

/// A one-line, severity-coloured message that stays put. The status line is the wrong place for errors: the scan
/// rewrites it twice a second. A failure never clears itself (look away, look back, it is still there); a success
/// does after a few seconds. The window has one, and each dialog its own, so a failure in a dialog shows in it.
#[derive(Clone, Debug, Default)]
pub struct Alert {
    pub text: String,
    pub level: Option<Level>,
    /// When a success clears itself (egui's time); set at the first frame it is shown.
    until: Option<f64>,
    timed: bool,
}

/// How long a success stays.
pub const OK_TTL: f64 = 6.0;

impl Alert {
    pub fn ok(&mut self, text: impl Into<String>) {
        *self = Alert { text: text.into(), level: Some(Level::Ok), until: None, timed: true };
    }

    pub fn warn(&mut self, text: impl Into<String>) {
        *self = Alert { text: text.into(), level: Some(Level::Warn), until: None, timed: false };
    }

    pub fn fail(&mut self, text: impl Into<String>) {
        *self = Alert { text: text.into(), level: Some(Level::Fail), until: None, timed: false };
    }

    pub fn clear(&mut self) {
        *self = Alert::default();
    }

    /// The strip, if there is something to say.
    pub fn show(&mut self, ui: &mut Ui) {
        let Some(level) = self.level else { return };
        let now = ui.input(|i| i.time);
        if self.timed {
            let until = *self.until.get_or_insert(now + OK_TTL);
            if now >= until {
                self.clear();
                return;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(until - now));
        }
        let (icon, ink, fill) = match level {
            Level::Ok => (Icon::Check, pal().ok, pal().ok_fill),
            Level::Warn => (Icon::Warning, pal().warn, pal().warn_fill),
            Level::Fail => (Icon::Warning, pal().error, pal().error_fill),
        };
        egui::Frame::new().fill(fill).corner_radius(4).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::paint(ui.painter(), icon, r, ink);
                ui.add(egui::Label::new(RichText::new(&self.text).color(ink)).wrap());
            });
        });
    }
}
