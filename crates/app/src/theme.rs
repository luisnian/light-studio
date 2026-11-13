//! Colours, type and shapes. One set of [`Tokens`] for the dark look and one for the light, picked from the egui visuals
//! in use, and the few painting helpers every floating panel shares.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Painter, Rect, Stroke, StrokeKind};

/// Corner radius of the cards and panels floating over the map.
pub const R_CARD: f32 = 16.0;
/// Corner radius of buttons and fields.
pub const R_CONTROL: f32 = 8.0;
/// Corner radius of small tiles and chips.
pub const R_TILE: f32 = 10.0;

/// The red-light look is on (it keeps night vision: nothing but red reaches the eye).
static RED_LIGHT: AtomicBool = AtomicBool::new(false);

/// Turns the red-light look on or off. Call [`apply`] afterwards.
pub fn set_red_light(on: bool) {
    RED_LIGHT.store(on, Ordering::Relaxed);
}

/// The red-light look is on.
pub fn red_light() -> bool {
    RED_LIGHT.load(Ordering::Relaxed)
}

/// The colour map tiles are multiplied by: white, or a deep red in the red-light look.
pub fn map_tint() -> Color32 {
    if red_light() { Color32::from_rgb(255, 40, 30) } else { Color32::WHITE }
}

/// One map tile as it was painted this frame, kept so the cards can paint a blurred copy of the map under their glass.
#[derive(Clone, Copy, Debug)]
pub struct BackdropTile {
    pub texture: egui::TextureId,
    pub rect: Rect,
    pub uv: Rect,
    pub tint: Color32,
}

static BACKDROP: Mutex<Vec<BackdropTile>> = Mutex::new(Vec::new());
static NO_BLUR: AtomicBool = AtomicBool::new(false);

/// Turns the frosted glass (the blurred map under the cards) on or off.
pub fn set_frosted(on: bool) {
    NO_BLUR.store(!on, Ordering::Relaxed);
}

/// Keeps the tiles the map painted, in painting order, for the cards to blur.
pub fn set_backdrop(tiles: Vec<BackdropTile>) {
    *BACKDROP.lock().unwrap_or_else(PoisonError::into_inner) = tiles;
}

/// How far, in points, the taps of the blur reach.
const BLUR_RADIUS: f32 = 7.0;
/// Taps on the ring (and one in the middle).
const BLUR_TAPS: usize = 8;

/// Paints the map's tiles under `r` blurred: the tiles again, shifted round a ring, each tap weighted so the sum is their
/// plain average. No pixel is read back: it is only a few more textured quads.
fn paint_blurred_backdrop(p: &Painter, r: Rect, k: f32) {
    if NO_BLUR.load(Ordering::Relaxed) || k < 0.05 {
        return;
    }
    let tiles = BACKDROP.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let p = p.with_clip_rect(r.intersect(p.clip_rect()));
    let mut offsets = vec![egui::Vec2::ZERO];
    for i in 0..BLUR_TAPS {
        let a = std::f32::consts::TAU * i as f32 / BLUR_TAPS as f32;
        offsets.push(egui::vec2(a.cos(), a.sin()) * BLUR_RADIUS);
    }
    for (n, off) in offsets.iter().enumerate() {
        // the n-th tap at 1/(n+1) of the opacity makes the running average of all of them
        let weight = k / (n as f32 + 1.0);
        for t in tiles.iter().filter(|t| t.rect.translate(*off).intersects(r)) {
            p.image(t.texture, t.rect.translate(*off), t.uv, t.tint.gamma_multiply(weight));
        }
    }
}

static REDUCE_MOTION: AtomicBool = AtomicBool::new(false);

/// Turns the animations off (cards and bars appear at once).
pub fn set_reduce_motion(on: bool) {
    REDUCE_MOTION.store(on, Ordering::Relaxed);
}

/// An animation's length in seconds: zero when motion is reduced.
pub fn motion(secs: f32) -> f32 {
    if REDUCE_MOTION.load(Ordering::Relaxed) { 0.0 } else { secs }
}

/// The family text of a heavier weight is set in (see [`install_fonts`]).
const SEMIBOLD: &str = "semibold";
/// The family of the in-between weight (buttons, tabs, small titles).
const MEDIUM: &str = "medium";

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
    /// A day, row or tile that is chosen: a warm tint of the accent, with the accent's hairline (see [`paint_chosen`]).
    pub tool_active: Color32,
    /// A tab or segment that is on: a light pill, with `on_pill` text on it.
    pub pill: Color32,
    pub on_pill: Color32,
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
            canvas: Color32::from_rgb(14, 14, 16),
            chrome: Color32::from_rgb(22, 22, 24),
            backdrop: Color32::from_rgb(9, 9, 11),
            inset: Color32::from_rgb(31, 31, 34),
            field: Color32::from_rgb(26, 26, 29),
            shadow: Color32::from_black_alpha(130),
            pick: Color32::from_rgb(104, 214, 134),
            track: Color32::from_rgb(54, 54, 59),
            field_border: Color32::from_rgb(66, 66, 72),
            card_stroke: Color32::from_rgb(50, 50, 55),
            divider: Color32::from_rgb(41, 41, 45),
            hover: Color32::from_rgb(43, 43, 48),
            tool_active: Color32::from_rgb(62, 49, 32),
            pill: Color32::from_rgb(243, 243, 246),
            on_pill: Color32::from_rgb(14, 14, 16),
            accent: Color32::from_rgb(216, 168, 103),
            text: Color32::from_rgb(245, 245, 247),
            text_dim: Color32::from_rgb(161, 161, 166),
            text_label: Color32::from_rgb(200, 200, 206),
            text_disabled: Color32::from_rgb(106, 106, 112),
            reject: Color32::from_rgb(255, 105, 97),
            caution: Color32::from_rgb(255, 196, 87),
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
            tool_active: Color32::from_rgb(246, 232, 208),
            pill: Color32::from_rgb(29, 29, 31),
            on_pill: Color32::from_rgb(252, 252, 253),
            accent: Color32::from_rgb(168, 112, 28),
            text: Color32::from_rgb(29, 29, 31),
            text_dim: Color32::from_rgb(110, 110, 118),
            text_label: Color32::from_rgb(70, 70, 78),
            text_disabled: Color32::from_rgb(166, 166, 174),
            reject: Color32::from_rgb(215, 38, 30),
            caution: Color32::from_rgb(190, 110, 0),
        }
    }

    /// Dark, and only red: no green or blue anywhere, so a dark-adapted eye is not dazzled.
    pub fn red() -> Tokens {
        let r = |v: u8| Color32::from_rgb(v, 0, 0);
        Tokens {
            dark: true,
            canvas: r(12),
            chrome: r(22),
            backdrop: r(8),
            inset: r(34),
            field: r(28),
            shadow: Color32::from_black_alpha(110),
            pick: r(230),
            track: r(60),
            field_border: r(92),
            card_stroke: r(84),
            divider: r(66),
            hover: r(52),
            tool_active: r(78),
            pill: r(220),
            on_pill: r(16),
            accent: r(255),
            text: r(255),
            text_dim: r(170),
            text_label: r(200),
            text_disabled: r(104),
            reject: r(255),
            caution: r(214),
        }
    }

    /// The tokens for the look in use.
    pub fn get(ctx: &Context) -> Tokens {
        if red_light() {
            Tokens::red()
        } else if ctx.global_style().visuals.dark_mode {
            Tokens::dark()
        } else {
            Tokens::light()
        }
    }

    /// The regular type at `size` points.
    pub fn font(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Proportional)
    }

    /// The heavier type at `size` points.
    pub fn semibold(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
    }

    /// The in-between weight at `size` points: tabs, buttons and labels that must read as controls without shouting.
    pub fn medium(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(MEDIUM.into()))
    }
}

/// Registers Inter (SIL OFL, `assets/fonts`) as the interface's type: regular for text, and the heavier family used for
/// titles and numbers. egui's own fonts stay behind it as fallbacks for the glyphs Inter lacks.
pub fn install_fonts(ctx: &Context) {
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    let faces: [(&str, &[u8]); 3] = [
        ("inter-regular", include_bytes!("../../../assets/fonts/Inter-Regular.ttf")),
        ("inter-medium", include_bytes!("../../../assets/fonts/Inter-Medium.ttf")),
        ("inter-semibold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf")),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(name.to_string(), Arc::new(egui::FontData::from_static(bytes)));
    }
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let with = |first: &str| {
        let mut family = vec![first.to_string()];
        family.extend(fallback.iter().cloned());
        family
    };
    fonts.families.insert(FontFamily::Proportional, with("inter-regular"));
    fonts.families.insert(FontFamily::Name(MEDIUM.into()), with("inter-medium"));
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), with("inter-semibold"));
    ctx.set_fonts(fonts);
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
    v.window_corner_radius = CornerRadius::same(R_CARD as u8);
    v.menu_corner_radius = CornerRadius::same(R_TILE as u8);
    v.window_shadow = egui::Shadow { offset: [0, 10], blur: 32, spread: 0, color: Color32::from_black_alpha(150) };
    v.popup_shadow = v.window_shadow;
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
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&if red_light() { Tokens::red() } else { Tokens::dark() }));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&Tokens::light()));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 7.0);
        s.spacing.button_padding = egui::vec2(12.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        use egui::TextStyle::*;
        for (style, size, family) in [
            (Small, 11.0, FontFamily::Proportional),
            (Body, 13.0, FontFamily::Proportional),
            (Button, 13.0, FontFamily::Name(MEDIUM.into())),
            (Heading, 17.0, FontFamily::Name(SEMIBOLD.into())),
        ] {
            s.text_styles.insert(style, FontId::new(size, family));
        }
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
    let radius = CornerRadius::same(12);
    for (grow, down, a) in [(6.0, 8.0, 30u8), (0.0, 2.0, 55)] {
        p.rect_filled(
            r.expand(grow).translate(egui::vec2(0.0, down)),
            CornerRadius::same(12 + grow as u8),
            Color32::from_black_alpha(a).gamma_multiply(k),
        );
    }
    p.rect_filled(r, radius, chip_fill(t).gamma_multiply(k));
    p.rect_stroke(r, radius, Stroke::new(1.0, t.card_stroke.gamma_multiply(k)), StrokeKind::Inside);
}

/// The graphite glass of cards and menus: a dark grey (a light grey in the light look) that lets the map show through.
fn glass(t: &Tokens, alpha: u8) -> Color32 {
    if red_light() {
        Color32::from_rgba_unmultiplied(30, 0, 0, alpha.max(244))
    } else if t.dark {
        Color32::from_rgba_unmultiplied(22, 22, 25, alpha)
    } else {
        Color32::from_rgba_unmultiplied(248, 248, 250, alpha)
    }
}

/// A card: graphite glass with a hairline and a soft shadow; `k` (0..1) scales how opaque it is.
pub fn paint_card(p: &Painter, r: Rect, t: &Tokens, k: f32) {
    paint_glass(p, r, t, 206, k);
}

/// A card that holds a lot of text over the map (the big cards): the same glass, more opaque so the map does not get in the way.
pub fn paint_card_big(p: &Painter, r: Rect, t: &Tokens, k: f32) {
    paint_glass(p, r, t, 236, k.clamp(0.0, 1.0));
}

fn paint_glass(p: &Painter, r: Rect, t: &Tokens, alpha: u8, k: f32) {
    let radius = CornerRadius::same(R_CARD as u8);
    // a deep, soft shadow in three layers, wider and fainter outside, as the web's windows have
    for (grow, down, a) in [(18.0, 22.0, 26u8), (8.0, 9.0, 40), (0.0, 2.0, 60)] {
        p.rect_filled(
            r.expand(grow).translate(egui::vec2(0.0, down)),
            CornerRadius::same((R_CARD + grow) as u8),
            Color32::from_black_alpha(a).gamma_multiply(k),
        );
    }
    paint_blurred_backdrop(p, r, k);
    p.rect_filled(r, radius, glass(t, alpha).gamma_multiply(k));
    let hairline = if t.dark { Color32::from_white_alpha(26) } else { Color32::from_black_alpha(24) };
    p.rect_stroke(r, radius, Stroke::new(1.0, hairline.gamma_multiply(k)), StrokeKind::Inside);
    if t.dark && !red_light() {
        // the light catching the top edge: a brighter hairline that fades out over the first few points
        let top = Color32::from_white_alpha(34).gamma_multiply(k);
        let y = r.top() + 0.5;
        p.line_segment([egui::pos2(r.left() + R_CARD, y), egui::pos2(r.right() - R_CARD, y)], Stroke::new(1.0, top));
    }
}

/// A day, row or tile that is chosen: the accent's warm tint with the accent's own hairline round it.
pub fn paint_chosen(p: &Painter, r: Rect, radius: f32, t: &Tokens) {
    let radius = CornerRadius::same(radius.round() as u8);
    p.rect_filled(r, radius, t.tool_active);
    p.rect_stroke(r, radius, Stroke::new(1.0, t.accent.gamma_multiply(0.6)), StrokeKind::Inside);
}

/// A tab, segment or chip that is on: a light pill. The text on it is `t.on_pill`.
pub fn paint_pill(p: &Painter, r: Rect, radius: f32, t: &Tokens) {
    p.rect_filled(r, CornerRadius::same(radius.round() as u8), t.pill);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_red_look_has_no_green_or_blue_and_readable_text() {
        let t = Tokens::red();
        for c in [
            t.canvas,
            t.chrome,
            t.inset,
            t.field,
            t.text,
            t.text_dim,
            t.text_label,
            t.accent,
            t.hover,
            t.divider,
            t.card_stroke,
            t.pick,
            t.reject,
            t.caution,
            t.tool_active,
        ] {
            assert!(c.g() == 0 && c.b() == 0, "{c:?}");
        }
        assert!(t.text.r() - t.chrome.r() > 150);
        assert!(t.text_dim.r() - t.chrome.r() > 100);
    }

    #[test]
    fn a_card_over_the_map_paints_the_blur_taps_and_plain_glass_does_not() {
        let tile = BackdropTile {
            texture: egui::TextureId::Managed(1),
            rect: Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(256.0, 256.0)),
            uv: Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint: Color32::WHITE,
        };
        let count = |frosted: bool| {
            set_frosted(frosted);
            set_backdrop(vec![tile]);
            let ctx = Context::default();
            let mut images = 0;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                paint_card(ui.painter(), Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(100.0, 80.0)), &Tokens::dark(), 1.0);
            });
            out.textures_delta.clear();
            for s in &out.shapes {
                if matches!(s.shape, egui::Shape::Mesh(_)) {
                    images += 1;
                }
            }
            images
        };
        let (with, without) = (count(true), count(false));
        set_frosted(true);
        assert_eq!(with - without, 1 + BLUR_TAPS, "{with} {without}");
    }

    /// WCAG contrast ratio of two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lin = |c: u8| {
            let v = f32::from(c) / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        let l = |c: Color32| 0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b());
        let (x, y) = (l(a) + 0.05, l(b) + 0.05);
        x.max(y) / x.min(y)
    }

    /// `over` seen through the glass `g` (translucent) with the map `map` behind it.
    fn through(g: Color32, map: Color32) -> Color32 {
        let a = f32::from(g.a()) / 255.0;
        let mix = |t: u8, m: u8| (f32::from(t) * a + f32::from(m) * (1.0 - a)).round() as u8;
        Color32::from_rgb(mix(g.r(), map.r()), mix(g.g(), map.g()), mix(g.b(), map.b()))
    }

    #[test]
    fn text_on_the_glass_stays_readable_over_the_brightest_and_darkest_map() {
        set_red_light(false);
        // (Light Studio is always dark: the light tokens are not drawn over the map)
        for (t, name) in [(Tokens::dark(), "dark")] {
            for alpha in [206u8, 236] {
                let g = glass(&t, alpha);
                for map in [Color32::WHITE, Color32::BLACK] {
                    let bg = through(g, map);
                    assert!(contrast(t.text, bg) >= 4.5, "{name} text, alpha {alpha}, map {map:?}: {}", contrast(t.text, bg));
                    assert!(contrast(t.text_dim, bg) >= 3.0, "{name} dim text, alpha {alpha}, map {map:?}: {}", contrast(t.text_dim, bg));
                }
            }
        }
        set_red_light(true);
        let t = Tokens::red();
        for alpha in [206u8, 236] {
            for map in [Color32::from_rgb(255, 40, 30), Color32::BLACK] {
                let bg = through(glass(&t, alpha), map);
                // pure red is dark to the eye: 4:1 is the most it reaches, and the price of keeping night vision
                assert!(contrast(t.text, bg) >= 4.0, "red text, alpha {alpha}: {}", contrast(t.text, bg));
            }
        }
        set_red_light(false);
    }

    #[test]
    fn each_look_has_readable_text() {
        let lum = |c: Color32| 0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b());
        let d = Tokens::dark();
        assert!(lum(d.text) - lum(d.chrome) > 150.0);
        let l = Tokens::light();
        assert!(lum(l.chrome) - lum(l.text) > 150.0);
    }
}
