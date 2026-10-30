//! Colours, type and shapes. One set of [`Tokens`] for the dark look and one for the light, picked from the egui visuals
//! in use, and the few painting helpers every floating panel shares.

use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Painter, Rect, Stroke, StrokeKind};

/// Corner radius of the cards and panels floating over the map.
pub const R_CARD: f32 = 12.0;
/// Corner radius of buttons and fields.
pub const R_CONTROL: f32 = 7.0;
/// Corner radius of small tiles and chips.
pub const R_TILE: f32 = 8.0;

/// The family text of a heavier weight is set in (see [`install_fonts`]).
const SEMIBOLD: &str = "semibold";

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    /// The page behind everything.
    pub canvas: Color32,
    /// Panels and bars.
    pub chrome: Color32,
    /// Behind modal cards.
    pub backdrop: Color32,
    /// Wells: fields, tracks, tiles.
    pub inset: Color32,
    /// The inside of a text field.
    pub field: Color32,
    /// Shadow under floating panels.
    pub shadow: Color32,
    /// A good sign (a clear night, a fine shower).
    pub pick: Color32,
    pub track: Color32,
    pub field_border: Color32,
    pub card_stroke: Color32,
    pub divider: Color32,
    pub hover: Color32,
    /// A tool or tab that is on.
    pub tool_active: Color32,
    pub accent: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_label: Color32,
    pub text_disabled: Color32,
    /// Something that is wrong or will be lost.
    pub reject: Color32,
    pub caution: Color32,
}

impl Tokens {
    pub fn dark() -> Tokens {
        Tokens {
            dark: true,
            canvas: Color32::from_rgb(20, 20, 22),
            chrome: Color32::from_rgb(29, 29, 31),
            backdrop: Color32::from_rgb(16, 16, 18),
            inset: Color32::from_rgb(40, 40, 43),
            field: Color32::from_rgb(34, 34, 37),
            shadow: Color32::from_black_alpha(96),
            pick: Color32::from_rgb(76, 217, 100),
            track: Color32::from_rgb(58, 58, 62),
            field_border: Color32::from_rgb(72, 72, 78),
            card_stroke: Color32::from_rgb(62, 62, 68),
            divider: Color32::from_rgb(52, 52, 57),
            hover: Color32::from_rgb(54, 54, 59),
            tool_active: Color32::from_rgb(10, 98, 190),
            accent: Color32::from_rgb(64, 156, 255),
            text: Color32::from_rgb(240, 240, 243),
            text_dim: Color32::from_rgb(160, 160, 169),
            text_label: Color32::from_rgb(196, 196, 204),
            text_disabled: Color32::from_rgb(104, 104, 112),
            reject: Color32::from_rgb(255, 99, 90),
            caution: Color32::from_rgb(255, 189, 68),
        }
    }

    pub fn light() -> Tokens {
        Tokens {
            dark: false,
            canvas: Color32::from_rgb(244, 244, 246),
            chrome: Color32::from_rgb(252, 252, 253),
            backdrop: Color32::from_rgb(232, 232, 236),
            inset: Color32::from_rgb(236, 236, 240),
            field: Color32::from_rgb(255, 255, 255),
            shadow: Color32::from_black_alpha(48),
            pick: Color32::from_rgb(30, 150, 60),
            track: Color32::from_rgb(214, 214, 220),
            field_border: Color32::from_rgb(196, 196, 204),
            card_stroke: Color32::from_rgb(208, 208, 214),
            divider: Color32::from_rgb(224, 224, 229),
            hover: Color32::from_rgb(228, 228, 233),
            tool_active: Color32::from_rgb(190, 218, 252),
            accent: Color32::from_rgb(0, 112, 224),
            text: Color32::from_rgb(29, 29, 31),
            text_dim: Color32::from_rgb(110, 110, 118),
            text_label: Color32::from_rgb(70, 70, 78),
            text_disabled: Color32::from_rgb(166, 166, 174),
            reject: Color32::from_rgb(215, 38, 30),
            caution: Color32::from_rgb(190, 110, 0),
        }
    }

    /// The tokens for the look in use.
    pub fn get(ctx: &Context) -> Tokens {
        if ctx.global_style().visuals.dark_mode { Tokens::dark() } else { Tokens::light() }
    }

    /// The regular type at `size` points.
    pub fn font(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Proportional)
    }

    /// The heavier type at `size` points.
    pub fn semibold(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
    }
}

/// Registers the semibold family. It is made of the fonts egui brings; where the system has a heavier face of its own it is
/// put first.
pub fn install_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut family = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, path) in system_bold_faces() {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(name.to_string(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            family.insert(0, name.to_string());
            break;
        }
    }
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), family);
    ctx.set_fonts(fonts);
}

fn system_bold_faces() -> &'static [(&'static str, &'static str)] {
    &[
        ("system-bold", "/System/Library/Fonts/Supplemental/Arial Bold.ttf"),
        ("system-bold", "C:\\Windows\\Fonts\\segoeuib.ttf"),
        ("system-bold", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
    ]
}

/// The egui visuals made from the tokens.
fn visuals(t: &Tokens) -> egui::Visuals {
    let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = t.chrome;
    v.window_fill = t.chrome;
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.inset;
    v.override_text_color = Some(t.text);
    v.hyperlink_color = t.accent;
    v.selection.bg_fill = t.accent.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.window_stroke = Stroke::new(1.0, t.card_stroke);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
    v.widgets.inactive.bg_fill = t.inset;
    v.widgets.inactive.weak_bg_fill = t.inset;
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.active.bg_fill = t.tool_active;
    v.widgets.active.weak_bg_fill = t.tool_active;
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
        w.corner_radius = CornerRadius::same(R_CONTROL as u8);
    }
    v
}

/// Sets the visuals and spacing of both looks from the tokens.
pub fn apply(ctx: &Context) {
    // Light Studio is always dark, whatever the system's appearance is
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&Tokens::dark()));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&Tokens::light()));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.interaction.tooltip_delay = 0.4;
    });
}

/// The fill of a small chip floating over the map.
pub fn chip_fill(t: &Tokens) -> Color32 {
    t.chrome.gamma_multiply(0.92)
}

/// A chip: the fill, and a hairline round it.
pub fn paint_chip(p: &Painter, r: Rect, t: &Tokens) {
    paint_chip_at(p, r, t, 1.0);
}

/// [`paint_chip`] at `k` (0..1) of its opacity.
pub fn paint_chip_at(p: &Painter, r: Rect, t: &Tokens, k: f32) {
    let radius = CornerRadius::same(R_TILE as u8);
    p.rect_filled(r, radius, chip_fill(t).gamma_multiply(k));
    p.rect_stroke(r, radius, Stroke::new(1.0, t.card_stroke.gamma_multiply(k)), StrokeKind::Inside);
}

/// The graphite glass of cards and menus: a dark grey (a light grey in the light look) that lets the map show through.
fn glass(t: &Tokens, alpha: u8) -> Color32 {
    if t.dark { Color32::from_rgba_unmultiplied(36, 36, 39, alpha) } else { Color32::from_rgba_unmultiplied(248, 248, 250, alpha) }
}

/// A card: graphite glass with a hairline and a soft shadow; `k` (0..1) scales how opaque it is.
pub fn paint_card(p: &Painter, r: Rect, t: &Tokens, k: f32) {
    paint_glass(p, r, t, 196, k);
}

/// A card that holds a lot of text over the map (the big cards): the same glass, more opaque so the map does not get in the way.
pub fn paint_card_big(p: &Painter, r: Rect, t: &Tokens, k: f32) {
    paint_glass(p, r, t, 226, k.clamp(0.0, 1.0));
}

fn paint_glass(p: &Painter, r: Rect, t: &Tokens, alpha: u8, k: f32) {
    let radius = CornerRadius::same(R_CARD as u8);
    // a soft shadow: two layers, wider and fainter outside
    p.rect_filled(
        r.expand(6.0).translate(egui::vec2(0.0, 6.0)),
        CornerRadius::same(R_CARD as u8 + 6),
        Color32::from_black_alpha(18).gamma_multiply(k),
    );
    p.rect_filled(r.translate(egui::vec2(0.0, 2.0)), radius, Color32::from_black_alpha(50).gamma_multiply(k));
    p.rect_filled(r, radius, glass(t, alpha).gamma_multiply(k));
    let hairline = if t.dark { Color32::from_white_alpha(30) } else { Color32::from_black_alpha(24) };
    p.rect_stroke(r, radius, Stroke::new(1.0, hairline.gamma_multiply(k)), StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_look_has_readable_text() {
        let lum = |c: Color32| 0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b());
        let d = Tokens::dark();
        assert!(lum(d.text) - lum(d.chrome) > 150.0);
        let l = Tokens::light();
        assert!(lum(l.chrome) - lum(l.text) > 150.0);
    }
}
