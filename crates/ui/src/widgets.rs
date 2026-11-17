//! Pieces every part of the window uses, in PhotoCraft's hand: buttons drawn by hand (an icon, a focus ring, the dark
//! primary), pill tabs, cards on the dock, quiet fields with an icon or a unit, the icon rail's buttons, hairlines, the
//! alert strip. Each is named for screen readers and tests by its label. The shapes follow PhotoCraft's
//! `crates/ui-egui/src/widgets.rs` (`pill_tab`, `card`, `button_impl`, `text_field`, `hairline`; MIT, see
//! THIRD-PARTY-NOTICES.md in the repository root).

use egui::{
    Align, Color32, CornerRadius, CursorIcon, Frame, Layout, Margin, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit,
    Ui, UiBuilder, WidgetInfo, WidgetType, epaint::RectShape, pos2, vec2,
};

use crate::icons::{self, Icon};
use crate::style::{self, RADIUS, RADIUS_SM, pal};

/// How a button stands out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// The usual quiet button: field fill, hairline border.
    Plain,
    /// The one primary action of its area: dark on light, light on dark.
    Primary,
    /// A destructive action: it reads as one before it is clicked, not only in the confirmation.
    Danger,
}

/// Height of a button and a field.
pub const CONTROL_H: f32 = 28.0;

/// A button with an icon before its label, named for screen readers and tests by the label.
pub fn button(ui: &mut Ui, icon: Option<Icon>, label: &str, tone: Tone) -> Response {
    button_named(ui, icon, label, label, tone)
}

/// [`button`] whose accessible name says more than its short label: a panel's "Send" is "Send: Reboot device".
pub fn button_named(ui: &mut Ui, icon: Option<Icon>, label: &str, name: &str, tone: Tone) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), style::medium(12.5), pal().text);
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let (rect, r) = ui.allocate_exact_size(vec2(galley.size().x + icon_w + 24.0, CONTROL_H), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name));
    let hovered = r.hovered() && enabled;
    let pressed = r.is_pointer_button_down_on() && enabled;
    let quiet = |rest: Color32| {
        if pressed {
            pal().pressed
        } else if hovered {
            pal().hover_fill
        } else {
            rest
        }
    };
    let (fill, stroke, ink) = match tone {
        Tone::Primary => {
            let fill = if pressed {
                pal().primary.gamma_multiply(0.85)
            } else if hovered {
                pal().primary.gamma_multiply(0.93)
            } else {
                pal().primary
            };
            (fill, Stroke::NONE, pal().primary_text)
        }
        Tone::Danger => {
            let fill = if hovered && !pressed { pal().error_fill } else { quiet(pal().field) };
            (fill, Stroke::new(1.0, pal().error.gamma_multiply(0.55)), pal().error)
        }
        Tone::Plain => (quiet(pal().field), Stroke::new(1.0, pal().field_border), pal().text),
    };
    // Disabled, every tone looks the same quiet way: a washed-out primary reads as broken, not as unavailable.
    let (fill, stroke, ink) = if enabled { (fill, stroke, ink) } else { (pal().field, Stroke::new(1.0, pal().separator), pal().disabled) };
    let p = ui.painter();
    if r.has_focus() {
        p.rect_stroke(rect.expand(2.0), RADIUS_SM + 2, Stroke::new(2.0, pal().accent), StrokeKind::Outside);
    }
    p.rect(rect, RADIUS_SM, fill, stroke, StrokeKind::Inside);
    let mut x = rect.center().x - (galley.size().x + icon_w) / 2.0;
    if let Some(icon) = icon {
        icons::paint(p, icon, Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(15.0, 15.0)), ink);
        x += icon_w;
    }
    p.galley_with_override_text_color(pos2(x, rect.center().y - galley.size().y / 2.0), galley, ink);
    r
}

/// A small square button: an icon alone, its name in its tooltip and for screen readers.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    icon_toggle(ui, icon, label, label, false, 28.0)
}

/// Width of the icon rail's content: room for the longest short label under its icon.
pub const RAIL_W: f32 = 58.0;

/// An icon rail tile: the icon with its short name under it, so it is found without the tooltip; tinted with the
/// accent while what it opens is open (as PhotoCraft's tool rail marks the active tool).
pub fn rail_button(ui: &mut Ui, icon: Icon, label: &str, tip: &str, active: bool) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(RAIL_W, 48.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    let p = ui.painter();
    if active {
        p.rect(rect, RADIUS_SM, pal().select_fill, Stroke::new(1.0, pal().accent_border), StrokeKind::Inside);
    } else if r.hovered() && enabled {
        p.rect_filled(rect, RADIUS_SM, pal().hover_fill);
    }
    if r.has_focus() {
        p.rect_stroke(rect, RADIUS_SM, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    }
    let (ink, text) = match (enabled, active) {
        (false, _) => (pal().disabled, pal().disabled),
        (true, true) => (pal().accent, pal().text),
        (true, false) => (pal().icon, pal().muted),
    };
    icons::paint(p, icon, Rect::from_center_size(pos2(rect.center().x, rect.top() + 17.0), vec2(20.0, 20.0)), ink);
    let name = p.layout_no_wrap(label.to_string(), style::medium(10.5), text);
    p.galley_with_override_text_color(pos2(rect.center().x - name.size().x / 2.0, rect.bottom() - 6.0 - name.size().y), name, text);
    r.on_hover_text(tip)
}

fn icon_toggle(ui: &mut Ui, icon: Icon, label: &str, tip: &str, active: bool, side: f32) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    let p = ui.painter();
    if active {
        p.rect(rect, RADIUS_SM, pal().select_fill, Stroke::new(1.0, pal().accent_border), StrokeKind::Inside);
    } else if r.hovered() && enabled {
        p.rect_filled(rect, RADIUS_SM, pal().hover_fill);
    }
    if r.has_focus() {
        p.rect_stroke(rect, RADIUS_SM, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    }
    let ink = match (enabled, active) {
        (false, _) => pal().disabled,
        (true, true) => pal().accent,
        (true, false) => pal().icon,
    };
    let glyph = (side * 0.6).round();
    icons::paint(p, icon, Rect::from_center_size(rect.center(), vec2(glyph, glyph)), ink);
    r.on_hover_text(tip)
}

/// A pill tab (PhotoCraft's): the open one a raised grey pill with a hairline, the others flat and dimmer.
pub fn tab(ui: &mut Ui, label: &str, active: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), style::medium(12.5), pal().text);
    let (rect, r) = ui.allocate_exact_size(vec2(galley.size().x + 20.0, 24.0), Sense::click());
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, label));
    paint_pill(ui, rect, active, r.hovered());
    if r.has_focus() {
        ui.painter().rect_stroke(rect, RADIUS_SM, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    }
    let ink = if active { pal().text } else { pal().muted };
    ui.painter().galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, ink);
    r
}

fn paint_pill(ui: &Ui, rect: Rect, active: bool, hovered: bool) {
    let p = ui.painter();
    if active {
        p.rect(rect, RADIUS_SM, pal().hover_fill, Stroke::new(1.0, pal().field_border), StrokeKind::Inside);
    } else if hovered {
        p.rect_filled(rect, RADIUS_SM, pal().hover_fill.gamma_multiply(0.6));
    }
}

/// A card's header: its title as the one open pill tab, in sentence case ("CONTROL PANEL" reads "Control panel"),
/// a label for screen readers.
pub fn section(ui: &mut Ui, title: &str) {
    let title = sentence_case(title);
    let galley = ui.painter().layout_no_wrap(title.clone(), style::medium(12.5), pal().text);
    let (rect, r) = ui.allocate_exact_size(vec2(galley.size().x + 20.0, 24.0), Sense::hover());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &title));
    paint_pill(ui, rect, true, false);
    ui.painter().galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, pal().text);
}

fn sentence_case(s: &str) -> String {
    if s.chars().any(char::is_lowercase) {
        return s.to_string();
    }
    let lower = s.to_lowercase();
    let mut chars = lower.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// A card on the dock: the card fill, a hairline border, PhotoCraft's card radius and padding.
pub fn card() -> Frame {
    Frame::new().fill(pal().card).stroke(Stroke::new(1.0, pal().card_border)).corner_radius(RADIUS).inner_margin(Margin {
        left: 8,
        right: 8,
        top: 6,
        bottom: 10,
    })
}

/// A quiet field: field fill, hairline border, an optional icon before the text and a dimmed unit after it. Hovered it
/// shades a step; typing, it carries the same 2 pt accent ring as a focused button; a click anywhere in it, the
/// padding and the icon too, puts the cursor in it. The caller builds the edit (id, hint, font, password).
pub fn field(ui: &mut Ui, edit: TextEdit<'_>, icon: Option<Icon>, unit: Option<&str>, width: f32) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(width.max(48.0), CONTROL_H), Sense::hover());
    let enabled = ui.is_enabled();
    // The fill goes under the edit, but whether it shows hover and focus is known only after the edit is laid out.
    let fill = ui.painter().add(Shape::Noop);
    let mut inner = rect.shrink2(vec2(8.0, 2.0));
    let ink = if enabled { pal().muted } else { pal().disabled };
    if let Some(icon) = icon {
        icons::paint(ui.painter(), icon, Rect::from_center_size(pos2(inner.left() + 7.0, rect.center().y), vec2(14.0, 14.0)), ink);
        inner.min.x += 20.0;
    }
    if let Some(unit) = unit {
        let g = ui.painter().layout_no_wrap(unit.to_string(), egui::FontId::proportional(11.5), ink);
        inner.max.x -= g.size().x + 6.0;
        ui.painter().galley(pos2(inner.right() + 6.0, rect.center().y - g.size().y / 2.0), g, ink);
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
    child.spacing_mut().interact_size.y = inner.height();
    let r = child.add(edit.frame(Frame::NONE).desired_width(inner.width()).vertical_align(Align::Center));
    let over = enabled && ui.rect_contains_pointer(rect);
    if over && !r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Text);
        if ui.input(|i| i.pointer.primary_clicked()) {
            r.request_focus();
        }
    }
    let focused = r.has_focus();
    let shade = if over && !focused { pal().hover_fill } else { pal().field };
    ui.painter().set(fill, RectShape::filled(rect, RADIUS_SM, shade));
    if focused {
        ui.painter().rect_stroke(rect, RADIUS_SM, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    } else {
        ui.painter().rect_stroke(rect, RADIUS_SM, Stroke::new(1.0, pal().field_border), StrokeKind::Inside);
    }
    r
}

/// A dropdown in the field's look: the chosen text and a chevron; a click opens `items` under it (build them with
/// [`dropdown_item`]) and a click on one closes it. Named `"<name>: <text>"`, so a form's two dropdowns that both
/// show "Aus" stay apart for screen readers and tests.
pub fn dropdown(ui: &mut Ui, name: &str, text: &str, width: f32, items: impl FnOnce(&mut Ui)) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(width.max(80.0), CONTROL_H), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, enabled, format!("{name}: {text}")));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&r));
    let fill = if enabled && (r.hovered() || open) { pal().hover_fill } else { pal().field };
    let ink = if enabled { pal().text } else { pal().disabled };
    let p = ui.painter();
    p.rect_filled(rect, RADIUS_SM, fill);
    if r.has_focus() || open {
        p.rect_stroke(rect, RADIUS_SM, Stroke::new(2.0, pal().accent), StrokeKind::Inside);
    } else {
        p.rect_stroke(rect, RADIUS_SM, Stroke::new(1.0, pal().field_border), StrokeKind::Inside);
    }
    let chevron = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(12.0, 12.0));
    icons::paint(p, Icon::Chevron, chevron, if enabled { pal().muted } else { pal().disabled });
    let galley = p.layout_no_wrap(text.to_string(), egui::FontId::proportional(12.5), ink);
    let room = Rect::from_min_max(rect.min + vec2(10.0, 0.0), rect.max - vec2(26.0, 0.0));
    p.with_clip_rect(room).galley_with_override_text_color(pos2(room.left(), rect.center().y - galley.size().y / 2.0), galley, ink);
    egui::Popup::menu(&r).width(rect.width()).show(|ui| {
        ui.set_min_width(rect.width() - 20.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        items(ui);
    });
    r
}

/// One line of a [`dropdown`]; whether it was picked.
pub fn dropdown_item(ui: &mut Ui, text: &str, selected: bool) -> bool {
    let width = ui.available_width();
    ui.add(egui::Button::selectable(selected, text).min_size(vec2(width, 24.0))).clicked()
}

/// A switch, named `"<name>: Ein"` or `"<name>: Aus"`; the caller flips the value on a click.
pub fn switch(ui: &mut Ui, name: &str, on: bool) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(38.0, 22.0), Sense::click());
    let enabled = ui.is_enabled();
    let state = if on { "Ein" } else { "Aus" };
    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, on, format!("{name}: {state}")));
    let p = ui.painter();
    let (track, knob) = match (enabled, on) {
        (false, _) => (pal().field, pal().disabled),
        (true, true) => (pal().accent, pal().card),
        (true, false) => (pal().pressed, pal().icon),
    };
    if r.has_focus() {
        p.rect_stroke(rect.expand(2.0), 13.0, Stroke::new(2.0, pal().accent), StrokeKind::Outside);
    }
    p.rect(rect, 11.0, track, Stroke::new(1.0, if on && enabled { pal().accent } else { pal().field_border }), StrokeKind::Inside);
    let x = if on { rect.right() - 11.0 } else { rect.left() + 11.0 };
    p.circle_filled(pos2(x, rect.center().y), 7.0, knob);
    r
}

/// Signal strength as four bars, `level` of them filled: a number in dBm says nothing to most people.
pub fn signal_bars(p: &egui::Painter, at: Rect, level: u8) {
    for i in 0..4u8 {
        let h = 4.0 + 3.0 * f32::from(i);
        let x = at.left() + 5.0 * f32::from(i);
        let bar = Rect::from_min_max(pos2(x, at.bottom() - h), pos2(x + 3.0, at.bottom()));
        p.rect_filled(bar, 1.0, if i < level { pal().icon } else { pal().field_border });
    }
}

/// How a line of a status or a result reads at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Ok,
    Warn,
    Refused,
    /// Sent, but not confirmed.
    Unsure,
    /// Nothing known yet.
    Idle,
}

/// A [`Mark`]'s drawing, 16 pt.
pub fn mark(ui: &mut Ui, mark: Mark) {
    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    let (icon, ink) = match mark {
        Mark::Ok => (Icon::Check, pal().ok),
        Mark::Warn => (Icon::Warning, pal().warn),
        Mark::Refused => (Icon::Refused, pal().error),
        Mark::Unsure => (Icon::Unsure, pal().warn),
        Mark::Idle => {
            ui.painter().circle_stroke(r.center(), 4.0, Stroke::new(1.5, pal().disabled));
            return;
        }
    };
    icons::paint(ui.painter(), icon, r, ink);
}

/// A hairline across the width.
pub fn hairline(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, pal().separator));
}

/// A vertical hairline in a row, `height` tall.
pub fn vline(ui: &mut Ui, height: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(9.0, height), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, pal().separator));
}

/// A chrome bar's frame (title bar, options bar, icon rail, status bar).
pub fn chrome(margin: Margin) -> Frame {
    Frame::new().fill(pal().chrome).inner_margin(margin)
}

/// The tab over the terminal, as PhotoCraft's document tab: a title and a dimmed detail on the card colour. Screen
/// readers get the title alone, so the detail never doubles a name shown elsewhere.
pub fn doc_tab(ui: &mut Ui, title: &str, detail: &str) {
    let t = ui.painter().layout_no_wrap(title.to_string(), style::medium(12.5), pal().text);
    let d = ui.painter().layout_no_wrap(detail.to_string(), egui::FontId::proportional(11.0), pal().muted);
    let gap = if detail.is_empty() { 0.0 } else { 8.0 };
    let (rect, r) = ui.allocate_exact_size(vec2(t.size().x + gap + d.size().x + 22.0, 26.0), Sense::hover());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, title));
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(RADIUS_SM), pal().card, Stroke::new(1.0, pal().card_border), StrokeKind::Inside);
    let x = rect.left() + 11.0;
    let tw = t.size().x;
    p.galley_with_override_text_color(pos2(x, rect.center().y - t.size().y / 2.0), t, pal().text);
    p.galley_with_override_text_color(pos2(x + tw + gap, rect.center().y - d.size().y / 2.0), d, pal().muted);
}

/// The brand in the title bar: the primary tile with the scan waves in it, and the name.
pub fn brand(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
    ui.painter().rect_filled(r, 5.0, pal().primary);
    icons::paint(ui.painter(), Icon::Scan, r.shrink(3.0), pal().primary_text);
    ui.label(RichText::new("Aircord").font(style::semibold(13.0)).color(pal().text));
}

/// A status dot.
pub fn dot(ui: &mut Ui, ink: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 3.5, ink);
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
        Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, ink.gamma_multiply(0.25)))
            .corner_radius(RADIUS_SM)
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    icons::paint(ui.painter(), icon, r, ink);
                    ui.add(egui::Label::new(RichText::new(&self.text).color(ink)).wrap());
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::sentence_case;

    #[test]
    fn a_shouted_title_reads_in_sentence_case() {
        assert_eq!(sentence_case("CONTROL PANEL"), "Control panel");
        assert_eq!(sentence_case("Devices"), "Devices");
        assert_eq!(sentence_case(""), "");
    }
}
