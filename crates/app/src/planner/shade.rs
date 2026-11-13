//! The shadow map: the ground the Sun does not reach at the time shown, over the planner's map (about 25 km round the
//! place, from terrain tiles of 56 m a pixel, as the CRM's solar map does). Ridges throw their shadows; slopes turned
//! from the Sun go dark. Drag the time bar and the shadows move.

use crate::services::terrain_tiles::{self, Dem};
use light_studio_astro::shade as grid;

use super::*;

/// The ground heights under the map's view, or why they could not be had. `serial` counts the grids read (a new one makes
/// a new picture); `ideal` is the terrain zoom the map's zoom asked for (the grid may be coarser to keep its size).
#[derive(Clone, Debug)]
pub(super) struct Heights {
    pub serial: u64,
    pub ideal: u8,
    pub ground: Result<Arc<Dem>, String>,
}

/// The ground the map's view needs, in Web Mercator world units (0..1, y south): the view with a margin round it for the
/// shadows cast from outside it, and the terrain zoom the map's zoom asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Want {
    pub nw: (f64, f64),
    pub se: (f64, f64),
    pub ideal: u8,
}

/// The picture on the map: what it was drawn for (the grid, the Sun's direction to a twentieth of a degree).
pub(super) struct Picture {
    pub tex: egui::TextureHandle,
    pub key: (u64, i64, i64),
}

/// The darkest the map gets under a cast shadow.
const MAX_ALPHA: f32 = 0.62;
/// Metres round the Earth at the equator.
const EQUATOR_M: f64 = 40_075_016.686;
/// How high a ridge outside the view may be and still throw its shadow into it (metres): with the Sun at `alt` degrees
/// the shadow reaches this over `tan(alt)`.
const RELIEF_M: f64 = 2500.0;
/// The margins the view is widened by (metres), the first one that is enough; the last is the most.
const MARGINS_M: [f64; 4] = [8_000.0, 15_000.0, 30_000.0, 60_000.0];
/// The share of the view's size added at each edge, so that a little panning does not ask for new ground.
const PAD: f64 = 0.10;
/// The extra share read when new ground is needed (on top of [`PAD`]), so that the next pans are free.
const READ_PAD: f64 = 0.20;
/// The picture fades out over this many pixels at the grid's edge (it has no hard border).
const FADE_PX: f32 = 3.0;

/// The margin round the view for the Sun at `alt` degrees: the shadow of a 2 500 m ridge, in steps (so that the Sun's
/// slow fall does not ask for new ground all the time).
pub(super) fn margin_m(alt: f64) -> f64 {
    let reach = RELIEF_M / alt.clamp(1.5, 90.0).to_radians().tan();
    MARGINS_M.iter().copied().find(|m| *m >= reach).unwrap_or(MARGINS_M[MARGINS_M.len() - 1])
}

/// The ground the view of the map `cam` over `free` needs for the Sun at `alt` at a place at latitude `lat`.
pub(super) fn want_for(cam: crate::map::Cam, free: Rect, lat: f64, alt: f64, map_zoom: f64) -> Want {
    // (the world repeats east–west: the view is taken in the copy of the world the centre is in)
    let shift = cam.cx.floor();
    let (x0, y0) = cam.world(free.left_top());
    let (x1, y1) = cam.world(free.right_bottom());
    let (x0, x1, y0, y1) = (x0 - shift, x1 - shift, y0, y1);
    let m = margin_m(alt) / (EQUATOR_M * lat.to_radians().cos().max(0.05));
    let (px, py) = ((x1 - x0) * PAD + m, (y1 - y0) * PAD + m);
    let ideal = map_zoom.round().clamp(f64::from(terrain_tiles::VIEW_MIN_ZOOM), f64::from(terrain_tiles::VIEW_MAX_ZOOM)) as u8;
    Want { nw: ((x0 - px).clamp(0.0, 1.0), (y0 - py).clamp(0.0, 1.0)), se: ((x1 + px).clamp(0.0, 1.0), (y1 + py).clamp(0.0, 1.0)), ideal }
}

/// Whether the ground in hand serves `want`: it holds the whole of it, and was not read for a map zoom far from the
/// one now.
pub(super) fn serves(h: &Heights, want: &Want) -> bool {
    match &h.ground {
        Ok(g) => terrain_tiles::covers(g, want.nw, want.se) && (i32::from(want.ideal) - i32::from(h.ideal)).abs() < 2,
        // (a lookup that failed is not tried again by itself: turning the layer off and on does)
        Err(_) => true,
    }
}

/// Starts the ground heights for the view when the shadows are on and what is at hand does not serve it.
pub(super) fn fetch(app: &mut App) {
    if !app.ui.planner.show_shadows {
        // a lookup that failed is forgotten when the layer is turned off: turning it on again tries again
        if app.planner.shade_heights.as_ref().is_some_and(|h| h.ground.is_err()) {
            app.planner.shade_heights = None;
        }
        app.planner.shade_want = None;
        return;
    }
    let Some(want) = app.planner.shade_want else { return };
    if app.planner.shade_pending || app.planner.shade_heights.as_ref().is_some_and(|h| serves(h, &want)) {
        return;
    }
    // read a little more than is needed, so that the next pans are free
    let (w, h) = (want.se.0 - want.nw.0, want.se.1 - want.nw.1);
    let nw = ((want.nw.0 - w * READ_PAD).max(0.0), (want.nw.1 - h * READ_PAD).max(0.0));
    let se = ((want.se.0 + w * READ_PAD).min(1.0), (want.se.1 + h * READ_PAD).min(1.0));
    let (lat, lon) = (app.ui.planner.lat, app.ui.planner.lon);
    let serial = app.planner.shade_serial + 1;
    app.planner.shade_serial = serial;
    app.planner.shade_pending = true;
    let ideal = want.ideal;
    let started = crate::tasks::spawn(
        app,
        "Shadows",
        Some("download"),
        move || {
            // 1-metre LiDAR from a local folder, where it holds the whole view, beats the global tiles
            if let Ok(Some(fine)) = crate::services::fine_terrain::dem(lat, lon)
                && terrain_tiles::covers(&fine, want.nw, want.se)
            {
                return Ok(Arc::new(fine));
            }
            terrain_tiles::fetch_view(nw, se, ideal).map(Arc::new)
        },
        move |app, _ctx, ground| {
            app.planner.shade_pending = false;
            app.planner.shade_heights = Some(Heights { serial, ideal, ground });
        },
    );
    if let Err(e) = started {
        app.planner.shade_pending = false;
        app.planner.shade_heights = Some(Heights { serial, ideal, ground: Err(e) });
    }
}

/// The darkness as an image: a dark blue, more opaque the darker the ground, fading out at the grid's edge.
pub(super) fn image(dark: &[f32], w: usize, h: usize) -> egui::ColorImage {
    let mut px = Vec::with_capacity(dark.len() * 4);
    for (k, d) in dark.iter().enumerate() {
        let (i, j) = (k / w.max(1), k % w.max(1));
        let to_edge = i.min(j).min(h.saturating_sub(1).saturating_sub(i)).min(w.saturating_sub(1).saturating_sub(j));
        let fade = ((to_edge as f32 + 0.5) / FADE_PX).clamp(0.0, 1.0);
        let fade = fade * fade * (3.0 - 2.0 * fade);
        px.extend_from_slice(&[8, 12, 30, (d.clamp(0.0, 1.0) * MAX_ALPHA * fade * 255.0).round() as u8]);
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &px)
}

impl Page<'_> {
    /// Draws the shadows over the map (under the lines and pins) and, in `free`, says what is wrong when they cannot be.
    pub(super) fn shade_overlay(&mut self, painter: &egui::Painter, st: &PlannerState, rt: &mut Runtime, cam: crate::map::Cam, free: Rect) {
        let t = self.t;
        let sun = self.skies.sun;
        let say = |text: &str| {
            let g = painter.layout_no_wrap(text.to_string(), t.font(11.5), t.text);
            // (low in the map: the Sun and Moon card floats over its top)
            let r = Rect::from_center_size(pos2(free.center().x, free.bottom() - 70.0), g.size() + vec2(20.0, 12.0));
            crate::theme::paint_chip(painter, r, &t);
            painter.galley(r.center() - g.size() / 2.0, g, t.text);
        };
        if sun.alt <= 0.0 {
            rt.shade_want = None;
            say(crate::i18n::tr("The Sun is under the horizon: no shadows."));
            return;
        }
        let want = want_for(cam, free, st.lat, sun.alt, st.map.zoom);
        if terrain_tiles::plan_view(want.nw, want.se, want.ideal).is_none() {
            rt.shade_want = None;
            say(crate::i18n::tr("Zoom in to see the shadows."));
            return;
        }
        rt.shade_want = Some(want);
        let ground = match rt.shade_heights.clone() {
            Some(h) => match h.ground {
                Ok(g) => {
                    // (new ground is on its way: the shadows keep to what is at hand)
                    if !serves(&Heights { serial: h.serial, ideal: h.ideal, ground: Ok(Arc::clone(&g)) }, &want) {
                        painter.ctx().request_repaint_after(std::time::Duration::from_millis(120));
                    }
                    (h.serial, g)
                }
                Err(e) => {
                    say(&format!(
                        "{}: {e}. {}",
                        crate::i18n::tr("Could not look up the ground heights"),
                        crate::i18n::tr("Turn Shadows off and on to try again")
                    ));
                    return;
                }
            },
            None => {
                painter.ctx().request_repaint_after(std::time::Duration::from_millis(120));
                say(crate::i18n::tr("Looking up the ground heights…"));
                return;
            }
        };
        let (serial, ground) = ground;
        let pic_key = (serial, (sun.az * 20.0).round() as i64, (sun.alt * 20.0).round() as i64);
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
        let wx = cam.cx;
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

    fn cam_over(lat: f64, lon: f64, zoom: f64, rect: Rect) -> crate::map::Cam {
        crate::map::Cam::new(crate::map::MapView { lat, lon, zoom }, rect)
    }

    #[test]
    fn the_margin_grows_as_the_sun_gets_low_and_never_past_the_most() {
        assert_eq!(margin_m(60.0), MARGINS_M[0]);
        assert!(margin_m(20.0) <= margin_m(8.0) && margin_m(8.0) <= margin_m(3.0));
        assert_eq!(margin_m(0.1), MARGINS_M[3]);
        assert_eq!(margin_m(f64::NAN.max(0.0)), MARGINS_M[3]);
    }

    #[test]
    fn what_is_wanted_is_the_view_with_a_margin_wherever_the_map_looks() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0));
        // Donostia at zoom 11 and again far from it (Madrid, Sevilla): the ground follows the view, not the place
        for (lat, lon) in [(43.32, -1.98), (40.42, -3.70), (37.39, -5.98)] {
            let cam = cam_over(lat, lon, 11.0, rect);
            let w = want_for(cam, rect, lat, 25.0, 11.0);
            let (cx, cy) = (cam.cx, cam.cy);
            assert!(w.nw.0 < cx && cx < w.se.0 && w.nw.1 < cy && cy < w.se.1, "{w:?}");
            // wider than the view by at least the margin each side (1200 px at zoom 11 = 1200 / (256 · 2048) of the world)
            let view = 1200.0 / (256.0 * 2048.0);
            assert!(w.se.0 - w.nw.0 > view * 1.2, "{}", w.se.0 - w.nw.0);
            assert_eq!(w.ideal, 11);
            assert!(terrain_tiles::plan_view(w.nw, w.se, w.ideal).is_some());
        }
        // a lower Sun asks for a wider margin
        let cam = cam_over(43.3, -2.0, 11.0, rect);
        let high = want_for(cam, rect, 43.3, 60.0, 11.0);
        let low = want_for(cam, rect, 43.3, 2.0, 11.0);
        assert!(low.se.0 - low.nw.0 > high.se.0 - high.nw.0);
        // the zoom asked for is kept between the data's limits
        assert_eq!(want_for(cam_over(43.3, -2.0, 17.0, rect), rect, 43.3, 25.0, 17.0).ideal, 12);
        assert_eq!(want_for(cam_over(43.3, -2.0, 3.0, rect), rect, 43.3, 25.0, 3.0).ideal, 6);
        // a view over the date line stays inside the world
        let w = want_for(cam_over(0.0, 179.9, 9.0, rect), rect, 0.0, 25.0, 9.0);
        assert!(w.nw.0 >= 0.0 && w.se.0 <= 1.0 && w.nw.0 < w.se.0, "{w:?}");
    }

    #[test]
    fn ground_in_hand_serves_the_view_while_it_holds_it_and_the_zoom_is_near() {
        let dem = |nw: (f64, f64), se: (f64, f64)| Dem { width: 256, height: 256, heights: vec![0.0; 256 * 256], nw, se, m_per_px: 50.0 };
        let want = Want { nw: (0.40, 0.40), se: (0.41, 0.41), ideal: 11 };
        let h = |nw, se, ideal| Heights { serial: 1, ideal, ground: Ok(Arc::new(dem(nw, se))) };
        assert!(serves(&h((0.39, 0.39), (0.42, 0.42), 11), &want));
        assert!(serves(&h((0.39, 0.39), (0.42, 0.42), 10), &want), "a zoom off by one is near enough");
        assert!(!serves(&h((0.39, 0.39), (0.42, 0.42), 9), &want), "far from the zoom now");
        assert!(!serves(&h((0.405, 0.39), (0.42, 0.42), 11), &want), "the west edge is not held");
        let failed = Heights { serial: 1, ideal: 11, ground: Err("offline".into()) };
        assert!(serves(&failed, &want), "a failed lookup is not tried again by itself");
    }
}
