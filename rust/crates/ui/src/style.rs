//! Look and feel, taken from PhotoCraft's Studio themes: calm grey surfaces, rounded cards on a dock, hairline
//! separators, a dark primary button on light (inverted on dark), Inter for the interface and JetBrains Mono for the
//! terminal. Light is the default; the sun/moon switch in the title bar flips to dark for the session. Aircord adds the
//! colours a console needs: what was sent, what came back, warnings, success.
//!
//! The surface, text, accent and primary tokens, the radii and the egui visuals in [`apply`] come from PhotoCraft's
//! `crates/ui-egui/src/theme.rs` (`ThemeKind::StudioLight` and `ThemeKind::Studio`; MIT, see THIRD-PARTY-NOTICES.md
//! in the repository root). Translucent accent tints are flattened onto the card colour here; text and status colours
//! are darkened (light) or lightened (dark) where needed to hold 4.5:1, which the tests check.

use std::cell::Cell;
use std::sync::Arc;

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme,
    ThemePreference, vec2,
};

/// A theme's colours.
pub struct Palette {
    /// Title bar, icon rail, options bar, status bar.
    pub chrome: Color32,
    /// What the cards sit on, and the terminal's surround.
    pub dock: Color32,
    /// Cards, dialogs, the terminal.
    pub card: Color32,
    pub card_border: Color32,
    /// Fields, plain buttons, combo boxes.
    pub field: Color32,
    pub field_border: Color32,
    /// The focus ring and the sort marker.
    pub accent: Color32,
    /// The outline of a selected row or rail button.
    pub accent_border: Color32,
    /// A selected row or rail button.
    pub select_fill: Color32,
    pub hover_fill: Color32,
    pub pressed: Color32,
    pub text: Color32,
    pub muted: Color32,
    /// Timestamps and info text: still 4.5:1 on every surface.
    pub faint: Color32,
    /// Disabled labels and icons only: the one tone allowed below 4.5:1.
    pub disabled: Color32,
    pub icon: Color32,
    /// Hairlines between areas.
    pub separator: Color32,
    /// The one primary action of an area: dark on light, light on dark.
    pub primary: Color32,
    pub primary_text: Color32,
    pub shadow: Color32,
    pub error: Color32,
    pub warn: Color32,
    pub ok: Color32,
    pub ok_fill: Color32,
    pub warn_fill: Color32,
    pub error_fill: Color32,
    /// The firmware dialog's hint box.
    pub bar: Color32,
    /// What fields and popups sit on.
    pub surface: Color32,
    /// The terminal's `>>` lines: what this side sent.
    pub sent: Color32,
    /// The terminal's `--` lines: what Aircord itself says.
    pub note: Color32,
}

/// Small radius: buttons, fields, rows.
pub const RADIUS_SM: u8 = 6;
/// Cards and menus.
pub const RADIUS: u8 = 8;
/// Dialogs.
pub const RADIUS_LG: u8 = 12;

const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// PhotoCraft's Studio Light.
pub const LIGHT: Palette = Palette {
    chrome: rgb(0xF6F6F8),
    dock: rgb(0xF0F0F3),
    card: rgb(0xFCFCFD),
    card_border: rgb(0xDEDEE4),
    field: rgb(0xF2F2F5),
    field_border: rgb(0xD6D6DC),
    accent: rgb(0x6C5CE7),
    accent_border: rgb(0xB8B1F3),
    select_fill: rgb(0xE8E5FA),
    hover_fill: rgb(0xE8E8ED),
    pressed: rgb(0xDCDCE2),
    text: rgb(0x18181C),
    muted: rgb(0x60606A),
    faint: rgb(0x6C6C76),
    disabled: rgb(0xA4A4AC),
    icon: rgb(0x3C3C44),
    separator: rgb(0xE2E2E8),
    primary: rgb(0x141418),
    primary_text: rgb(0xFAFAFC),
    shadow: Color32::from_black_alpha(50),
    error: rgb(0xB82E2E),
    warn: rgb(0x8A5A00),
    ok: rgb(0x13723A),
    ok_fill: rgb(0xE6F5EA),
    warn_fill: rgb(0xFBF1DC),
    error_fill: rgb(0xFBE6E6),
    bar: rgb(0xF2F2F5),
    surface: rgb(0xFCFCFD),
    sent: rgb(0x5040C8),
    note: rgb(0x60606A),
};

/// PhotoCraft's Studio (dark).
pub const DARK: Palette = Palette {
    chrome: rgb(0x141415),
    dock: rgb(0x111112),
    card: rgb(0x1A1A1C),
    card_border: rgb(0x28282C),
    field: rgb(0x232326),
    field_border: rgb(0x343439),
    accent: rgb(0x8B7CF6),
    accent_border: rgb(0x544F7E),
    select_fill: rgb(0x2E2C43),
    hover_fill: rgb(0x2C2C30),
    pressed: rgb(0x38383E),
    text: rgb(0xECECF0),
    muted: rgb(0x96969E),
    faint: rgb(0x8A8A92),
    disabled: rgb(0x55555C),
    icon: rgb(0xC4C4CC),
    separator: rgb(0x26262A),
    primary: rgb(0xF6F6F8),
    primary_text: rgb(0x0C0C0E),
    shadow: Color32::from_black_alpha(140),
    error: rgb(0xF06060),
    warn: rgb(0xF0BE5A),
    ok: rgb(0x4ADE80),
    ok_fill: rgb(0x16271C),
    warn_fill: rgb(0x2E2614),
    error_fill: rgb(0x341A1A),
    bar: rgb(0x232326),
    surface: rgb(0x1A1A1C),
    sent: rgb(0xD6D0FF),
    note: rgb(0x96969E),
};

thread_local! {
    /// Whether this frame is drawn dark.
    static DARK_ON: Cell<bool> = const { Cell::new(false) };
    /// Whether the named weights are registered yet: fonts set in a frame arrive in the next.
    static WEIGHTS: Cell<bool> = const { Cell::new(false) };
}

/// The colours of the theme this frame is drawn in.
pub fn pal() -> &'static Palette {
    if dark() { &DARK } else { &LIGHT }
}

pub fn dark() -> bool {
    DARK_ON.with(Cell::get)
}

/// Draw this frame in the theme egui shows: light, unless the user switched.
pub fn follow(ctx: &Context) {
    DARK_ON.with(|d| d.set(ctx.theme() == Theme::Dark));
    let ready = ctx.fonts(|f| f.definitions().families.contains_key(&FontFamily::Name(MEDIUM.into())));
    WEIGHTS.with(|w| w.set(ready));
}

/// Flips between light and dark for the rest of the session.
pub fn toggle(ctx: &Context) {
    ctx.set_theme(if ctx.theme() == Theme::Dark { Theme::Light } else { Theme::Dark });
}

const MEDIUM: &str = "medium";
const SEMIBOLD: &str = "semibold";

fn weight(name: &'static str, size: f32) -> FontId {
    if WEIGHTS.with(Cell::get) { FontId::new(size, FontFamily::Name(name.into())) } else { FontId::proportional(size) }
}

/// Inter Medium: buttons, tabs, headers.
pub fn medium(size: f32) -> FontId {
    weight(MEDIUM, size)
}

/// Inter SemiBold: titles.
pub fn semibold(size: f32) -> FontId {
    weight(SEMIBOLD, size)
}

/// Inter for the interface and JetBrains Mono for the terminal, egui's own fonts behind them for the glyphs they lack.
/// Both are under the SIL Open Font License; the texts are next to the files.
fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add("Inter", include_bytes!("../assets/fonts/Inter-Regular.ttf"));
    add("Inter-Medium", include_bytes!("../assets/fonts/Inter-Medium.ttf"));
    add("Inter-SemiBold", include_bytes!("../assets/fonts/Inter-SemiBold.ttf"));
    add("JetBrainsMono", include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "Inter".into());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "JetBrainsMono".into());
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (family, face) in [(MEDIUM, "Inter-Medium"), (SEMIBOLD, "Inter-SemiBold")] {
        let mut stack = vec![face.to_owned()];
        stack.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(family.into()), stack);
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &Context) {
    install_fonts(ctx);
    ctx.options_mut(|o| {
        o.theme_preference = ThemePreference::Light;
        o.fallback_theme = Theme::Light;
    });
    for (theme, pal) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
        ctx.style_mut_of(theme, |s| {
            // Regular weights only: a named weight in a text style would be used before the fonts arrive.
            s.text_styles = [
                (TextStyle::Small, FontId::proportional(10.5)),
                (TextStyle::Body, FontId::proportional(12.5)),
                (TextStyle::Button, FontId::proportional(12.5)),
                (TextStyle::Heading, FontId::proportional(15.0)),
                (TextStyle::Monospace, FontId::monospace(12.0)),
            ]
            .into();
            s.spacing.item_spacing = vec2(8.0, 6.0);
            s.spacing.button_padding = vec2(10.0, 4.0);
            s.spacing.interact_size = vec2(24.0, 24.0);
            s.spacing.combo_width = 120.0;
            // egui's modals take their padding from the menus: 10 gives the dialogs room and keeps menus compact.
            s.spacing.menu_margin = Margin::same(10);
            s.spacing.window_margin = Margin::same(16);
            s.spacing.icon_width = 14.0;
            s.spacing.tooltip_width = 280.0;
            // Thin scroll bars whose thumb stays visible while the pointer is on it.
            let mut scroll = egui::style::ScrollStyle::thin();
            scroll.interact_handle_opacity = 1.0;
            scroll.interact_background_opacity = 0.4;
            scroll.floating_width = 4.0;
            s.spacing.scroll = scroll;
            s.interaction.tooltip_delay = 0.35;
            let v = &mut s.visuals;
            v.dark_mode = theme == Theme::Dark;
            v.panel_fill = pal.chrome;
            v.window_fill = pal.card;
            v.window_stroke = Stroke::new(1.0, pal.card_border);
            v.extreme_bg_color = pal.field;
            v.text_edit_bg_color = Some(pal.field);
            v.faint_bg_color = pal.dock;
            v.code_bg_color = pal.field;
            v.hyperlink_color = pal.accent;
            v.warn_fg_color = pal.warn;
            v.error_fg_color = pal.error;
            v.window_corner_radius = CornerRadius::same(RADIUS_LG);
            v.menu_corner_radius = CornerRadius::same(RADIUS);
            v.window_shadow = Shadow { offset: [0, 10], blur: 32, spread: 0, color: pal.shadow };
            v.popup_shadow = Shadow { offset: [0, 6], blur: 20, spread: 0, color: pal.shadow };
            v.selection.bg_fill = pal.select_fill;
            v.selection.stroke = Stroke::new(1.0, pal.accent);
            v.handle_shape = egui::style::HandleShape::Circle;
            v.striped = false;
            v.indent_has_left_vline = false;
            let radius = CornerRadius::same(RADIUS_SM);
            let w = &mut v.widgets;
            w.noninteractive.bg_fill = pal.card;
            w.noninteractive.weak_bg_fill = pal.card;
            w.noninteractive.bg_stroke = Stroke::new(1.0, pal.separator);
            w.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
            w.noninteractive.corner_radius = radius;
            for (wv, fill, stroke) in [
                (&mut w.inactive, pal.field, pal.field_border),
                (&mut w.hovered, pal.hover_fill, pal.field_border),
                (&mut w.active, pal.pressed, pal.accent_border),
                (&mut w.open, pal.hover_fill, pal.field_border),
            ] {
                wv.bg_fill = fill;
                wv.weak_bg_fill = fill;
                wv.bg_stroke = Stroke::new(1.0, stroke);
                wv.fg_stroke = Stroke::new(1.0, pal.text);
                wv.corner_radius = radius;
                wv.expansion = 0.0;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_is_the_default_and_the_switch_flips_it() {
        let ctx = Context::default();
        apply(&ctx);
        assert_eq!(ctx.theme(), Theme::Light);
        toggle(&ctx);
        assert_eq!(ctx.theme(), Theme::Dark);
        toggle(&ctx);
        assert_eq!(ctx.theme(), Theme::Light);
    }

    #[test]
    fn the_primary_button_inverts_between_the_themes() {
        // Dark on light, light on dark, as PhotoCraft's Studio themes draw it.
        assert!(LIGHT.primary.r() < 64 && LIGHT.primary_text.r() > 200);
        assert!(DARK.primary.r() > 200 && DARK.primary_text.r() < 64);
    }

    #[test]
    fn the_named_weights_wait_for_their_fonts() {
        WEIGHTS.with(|w| w.set(false));
        assert_eq!(medium(13.0), FontId::proportional(13.0), "a named family used before it is bound panics");
        WEIGHTS.with(|w| w.set(true));
        assert_eq!(medium(13.0).family, FontFamily::Name(MEDIUM.into()));
        WEIGHTS.with(|w| w.set(false));
    }

    /// WCAG 2 contrast ratio of two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let f = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        };
        let (hi, lo) = if lum(a) > lum(b) { (lum(a), lum(b)) } else { (lum(b), lum(a)) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn text_holds_4_5_to_1_and_the_focus_ring_3_to_1_in_both_themes() {
        for (name, p) in [("light", &LIGHT), ("dark", &DARK)] {
            for (what, ink) in [("text", p.text), ("muted", p.muted), ("faint", p.faint)] {
                for (on, bg) in [("chrome", p.chrome), ("dock", p.dock), ("card", p.card), ("field", p.field)] {
                    let c = contrast(ink, bg);
                    assert!(c >= 4.5, "{name}: {what} on {on} is {c:.2}:1");
                }
            }
            for (what, ink, bg) in [
                ("ok on its fill", p.ok, p.ok_fill),
                ("warn on its fill", p.warn, p.warn_fill),
                ("error on its fill", p.error, p.error_fill),
                ("error on a field", p.error, p.field),
                ("sent on the terminal", p.sent, p.card),
                ("note on the terminal", p.note, p.card),
                ("primary text", p.primary_text, p.primary),
            ] {
                let c = contrast(ink, bg);
                assert!(c >= 4.5, "{name}: {what} is {c:.2}:1");
            }
            for (on, bg) in [("chrome", p.chrome), ("card", p.card), ("field", p.field)] {
                let c = contrast(p.accent, bg);
                assert!(c >= 3.0, "{name}: the focus ring on {on} is {c:.2}:1");
            }
        }
    }
}
