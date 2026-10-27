//! The shadow map: the ground the Sun does not reach at the time shown, over the planner's map (about 25 km round the
//! place, from terrain tiles of 56 m a pixel, as the CRM's solar map does). Ridges throw their shadows; slopes turned
//! from the Sun go dark. Drag the time bar and the shadows move.

use crate::services::{
    map_tiles,
    terrain_tiles::{self, Dem},
};
use light_studio_astro::shade as grid;

use super::*;

/// The place the heights were read for (latitude and longitude bits).
pub(super) type ShadeKey = (u64, u64);

/// The ground heights of the grid round a place, or why they could not be had.
#[derive(Clone, Debug)]
pub(super) struct Heights {
    pub key: ShadeKey,
    pub ground: Result<Arc<Dem>, String>,
}

/// The picture on the map: what it was drawn for (place, the Sun's direction to a twentieth of a degree).
pub(super) struct Picture {
    pub tex: egui::TextureHandle,
    pub key: (ShadeKey, i64, i64),
}

/// The darkest the map gets under a cast shadow.
const MAX_ALPHA: f32 = 0.62;

pub(super) fn key_of(st: &PlannerState) -> ShadeKey {
    (st.lat.to_bits(), st.lon.to_bits())
}

/// Starts the ground heights round the place when the shadows are on and they are not at hand.
pub(super) fn fetch(app: &mut App) {
    let st = &app.ui.planner;
    if !st.show_shadows {
        // a lookup that failed is forgotten when the layer is turned off: turning it on again tries again
        if app.planner.shade_heights.as_ref().is_some_and(|h| h.ground.is_err()) {
            app.planner.shade_heights = None;
        }
        return;
    }
    let key = key_of(st);
    if app.planner.shade_heights.as_ref().is_some_and(|h| h.key == key) || app.planner.shade_pending == Some(key) {
        return;
    }
    let (lat, lon) = (st.lat, st.lon);
    app.planner.shade_pending = Some(key);
    let started = crate::tasks::spawn(
        app,
        "Shadows",
        Some("download"),
        move || terrain_tiles::fetch(lat, lon, RADIUS_M).map(Arc::new),
        move |app, _ctx, ground| {
            if app.planner.shade_pending == Some(key) {
                app.planner.shade_pending = None;
            }
            app.planner.shade_heights = Some(Heights { key, ground });
        },
    );
    if let Err(e) = started {
        app.planner.shade_pending = None;
        app.planner.shade_heights = Some(Heights { key, ground: Err(e) });
    }
}

/// How far round the place the shadows reach (metres).
const RADIUS_M: f64 = 25_000.0;
/// The share of the picture's side over which it fades out at each edge (the area has no hard border).
const FADE: f32 = 0.07;

/// The darkness as an image: a dark blue, more opaque the darker the ground, fading out towards the edges.
pub(super) fn image(dark: &[f32], w: usize, h: usize) -> egui::ColorImage {
    let mut px = Vec::with_capacity(dark.len() * 4);
    let edge = (w.min(h) as f32 * FADE).max(1.0);
    for (k, d) in dark.iter().enumerate() {
        let (i, j) = (k / w.max(1), k % w.max(1));
        let to_edge = i.min(j).min(h.saturating_sub(1).saturating_sub(i)).min(w.saturating_sub(1).saturating_sub(j));
        let fade = ((to_edge as f32 + 0.5) / edge).clamp(0.0, 1.0);
        let fade = fade * fade * (3.0 - 2.0 * fade);
        px.extend_from_slice(&[8, 12, 30, (d.clamp(0.0, 1.0) * MAX_ALPHA * fade * 255.0).round() as u8]);
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &px)
}

impl Page<'_> {
    /// Draws the shadows over the map (under the lines and pins) and, in `free`, says what is wrong when they cannot be.
    pub(super) fn shade_overlay(&mut self, painter: &egui::Painter, st: &PlannerState, rt: &mut Runtime, cam: crate::map::Cam, free: Rect) {
        let t = self.t;
        let key = key_of(st);
        let sun = self.skies.sun;
        let say = |text: &str| {
            let g = painter.layout_no_wrap(text.to_string(), t.font(11.5), t.text);
            // (low in the map: the Sun and Moon card floats over its top)
            let r = Rect::from_center_size(pos2(free.center().x, free.bottom() - 70.0), g.size() + vec2(20.0, 12.0));
            crate::theme::paint_chip(painter, r, &t);
            painter.galley(r.center() - g.size() / 2.0, g, t.text);
        };
        let heights = rt.shade_heights.as_ref().filter(|h| h.key == key).cloned();
        let ground = match heights.as_ref().map(|h| &h.ground) {
            Some(Ok(g)) => Arc::clone(g),
            Some(Err(e)) => {
                say(&format!(
                    "{}: {e}. {}",
                    crate::i18n::tr("Could not look up the ground heights"),
                    crate::i18n::tr("Turn Shadows off and on to try again")
                ));
                return;
            }
            None => {
                say(crate::i18n::tr("Looking up the ground heights…"));
                return;
            }
        };
        if sun.alt <= 0.0 {
            say(crate::i18n::tr("The Sun is under the horizon: no shadows."));
            return;
        }
        let pic_key = (key, (sun.az * 20.0).round() as i64, (sun.alt * 20.0).round() as i64);
        if rt.shade_picture.as_ref().is_none_or(|p| p.key != pic_key) {
            let Some(dark) = grid::shade_dem(&ground.heights, ground.width, ground.height, ground.m_per_px, sun) else { return };
            let tex = painter.ctx().load_texture(
                "planner-shade",
                image(&dark, ground.width, ground.height),
                egui::TextureOptions {
                    magnification: egui::TextureFilter::Linear,
                    minification: egui::TextureFilter::Linear,
                    wrap_mode: egui::TextureWrapMode::ClampToEdge,
                    mipmap_mode: None,
                },
            );
            rt.shade_picture = Some(Picture { tex, key: pic_key });
        }
        let Some(pic) = rt.shade_picture.as_ref() else { return };
        let (wx, _) = map_tiles::project(st.lat, st.lon);
        let screen = |(x, y): (f64, f64)| {
            let mut dx = x - wx;
            dx -= dx.round();
            cam.screen(wx + dx, y)
        };
        let r = Rect::from_two_pos(screen(ground.nw), screen(ground.se));
        painter.image(pic.tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_is_clear_where_lit_dark_where_shaded_and_fades_at_the_edges() {
        let (w, h) = (60, 40);
        let mut dark = vec![0.0; w * h];
        dark[(h / 2) * w + w / 2] = 1.0;
        dark[0] = 1.0;
        let im = image(&dark, w, h);
        assert_eq!(im.size, [w, h]);
        assert_eq!(im.pixels[1].a(), 0);
        let centre = im.pixels[(h / 2) * w + w / 2].a();
        assert!(centre > 100 && centre < 200, "{centre}");
        assert!(im.pixels[0].a() < 25, "the corner has faded out");
    }
}
