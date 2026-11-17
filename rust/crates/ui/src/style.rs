//! Look and feel, taken from BIMCraft's desktop: a dense tool, light or dark (the system's theme until one is picked),
//! slate chrome, one blue accent for the primary action, selection and focus, an 8 px rhythm. Aircord adds the colours
//! a console needs: what was sent, what came back, warnings, success.

use std::cell::Cell;

use egui::{Color32, Context, CornerRadius, FontId, Stroke, TextStyle, Theme, ThemePreference, vec2};

/// A theme's colours.
pub struct Palette {
    pub accent: Color32,
    pub hover: Color32,
    pub select_fill: Color32,
    pub hover_fill: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub icon: Color32,
    pub panel: Color32,
    pub bar: Color32,
    pub border: Color32,
    pub error: Color32,
    pub warn: Color32,
    pub ok: Color32,
    pub ok_fill: Color32,
    pub warn_fill: Color32,
    pub error_fill: Color32,
    /// What fields and popups sit on, and the terminal.
    pub surface: Color32,
    /// The terminal's `>>` lines: what this side sent.
    pub sent: Color32,
    /// The terminal's `--` lines: what Aircord itself says.
    pub note: Color32,
}

const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

pub const LIGHT: Palette = Palette {
    accent: rgb(0x2563EB),
    hover: rgb(0x60A5FA),
    select_fill: rgb(0xBFDBFE),
    hover_fill: rgb(0xDBEAFE),
    text: rgb(0x0F172A),
    muted: rgb(0x475569),
    icon: rgb(0x334155),
    panel: rgb(0xF8FAFC),
    bar: rgb(0xF1F5F9),
    border: rgb(0xE2E8F0),
    error: rgb(0xDC2626),
    warn: rgb(0xB45309),
    ok: rgb(0x15803D),
    ok_fill: rgb(0xDCFCE7),
    warn_fill: rgb(0xFEF3C7),
    error_fill: rgb(0xFEE2E2),
    surface: rgb(0xFFFFFF),
    sent: rgb(0x1D4ED8),
    note: rgb(0x64748B),
};

pub const DARK: Palette = Palette {
    accent: rgb(0x3B82F6),
    hover: rgb(0x60A5FA),
    select_fill: rgb(0x1E3A5F),
    hover_fill: rgb(0x24334A),
    text: rgb(0xE2E8F0),
    muted: rgb(0x94A3B8),
    icon: rgb(0xCBD5E1),
    panel: rgb(0x1E2228),
    bar: rgb(0x262B33),
    border: rgb(0x3A414B),
    error: rgb(0xF87171),
    warn: rgb(0xFBBF24),
    ok: rgb(0x4ADE80),
    ok_fill: rgb(0x14341F),
    warn_fill: rgb(0x3A2E12),
    error_fill: rgb(0x3B1A1A),
    surface: rgb(0x171A1F),
    sent: rgb(0x93C5FD),
    note: rgb(0x8B98AB),
};

thread_local! {
    /// Whether this frame is drawn dark.
    static DARK_ON: Cell<bool> = const { Cell::new(false) };
}

/// The colours of the theme this frame is drawn in.
pub fn pal() -> &'static Palette {
    if dark() { &DARK } else { &LIGHT }
}

pub fn dark() -> bool {
    DARK_ON.with(Cell::get)
}

/// Draw this frame in the theme egui shows: the system's, unless the user picked one.
pub fn follow(ctx: &Context) {
    DARK_ON.with(|d| d.set(ctx.theme() == Theme::Dark));
}

pub fn apply(ctx: &Context) {
    ctx.options_mut(|o| {
        o.theme_preference = ThemePreference::System;
        o.fallback_theme = Theme::Dark;
    });
    for (theme, pal) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
        ctx.style_mut_of(theme, |s| {
            for (style, size) in [(TextStyle::Heading, 15.0), (TextStyle::Body, 13.0), (TextStyle::Button, 13.0), (TextStyle::Small, 11.0)]
            {
                s.text_styles.insert(style, FontId::proportional(size));
            }
            s.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.5));
            s.spacing.item_spacing = vec2(8.0, 4.0);
            s.spacing.button_padding = vec2(8.0, 3.0);
            s.spacing.interact_size.y = 24.0;
            let v = &mut s.visuals;
            v.panel_fill = pal.panel;
            v.window_fill = pal.surface;
            v.extreme_bg_color = pal.surface;
            v.faint_bg_color = pal.bar;
            v.window_stroke = Stroke::new(1.0, pal.border);
            v.hyperlink_color = pal.accent;
            v.selection.bg_fill = pal.select_fill;
            v.selection.stroke = Stroke::new(1.0, pal.accent);
            let radius = CornerRadius::same(4);
            for w in
                [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open]
            {
                w.corner_radius = radius;
            }
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal.border);
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.inactive.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.inactive.weak_bg_fill = pal.bar;
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, pal.border);
            v.widgets.hovered.weak_bg_fill = pal.hover_fill;
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, pal.hover);
            v.widgets.hovered.fg_stroke = Stroke::new(1.0, pal.text);
            v.widgets.active.weak_bg_fill = pal.select_fill;
            v.widgets.active.bg_stroke = Stroke::new(1.0, pal.accent);
            v.widgets.active.fg_stroke = Stroke::new(1.0, pal.text);
        });
    }
}
