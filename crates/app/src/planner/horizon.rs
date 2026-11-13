//! Mountains: the skyline seen from the place all the way round (PeakFinder style), from the real ground heights
//! (Open-Meteo, as the terrain profile), with the Sun, the Moon and the Milky Way's core drawn on it where they are at
//! the time shown, their paths over the day, and the summits that stand out with their height and distance. When it is
//! on it takes the map's place (the whole free part), as a panorama is only of use big.

use light_studio_astro::horizon::{self, Skyline};
use light_studio_astro::panorama::{Photo, Surface, View as PanoView};

use super::*;

/// The place the skyline was read for (latitude and longitude bits).
pub(super) type HorizonKey = (u64, u64);

/// The skyline of a place, or why it could not be had.
#[derive(Clone, Debug)]
pub(super) struct Horizon {
    pub key: HorizonKey,
    pub data: Result<Arc<Skyline>, String>,
    /// The named summits that show on the skyline, highest first.
    pub names: Arc<Vec<Named>>,
    /// Why the summit names are missing (the lookup failed), shown in the credit line.
    pub names_error: Option<String>,
    /// Whether the skyline is the fine one (terrain tiles) and not the coarse web lookup.
    pub fine: bool,
    /// The ground all round, to draw the panorama in colour (only with the fine terrain).
    pub surface: Option<Arc<Surface>>,
    /// Whether this is the whole 60 km look (`false`: the quick one, 15 km, shown while the whole one is read).
    pub complete: bool,
    /// Whether the ground near the place comes from the finer terrain (Mapterhorn).
    pub detail: bool,
}

/// The colour of every sample of the ground, for the Sun's place and the photo it was made with (the place, whether a
/// photo was used, the Sun's direction to half a degree).
pub(super) struct PanoColours {
    pub key: (HorizonKey, bool, i64, i64, usize),
    pub colours: Arc<Vec<[u8; 3]>>,
}

/// The panorama as a picture, and what it was drawn for.
pub(super) struct PanoTexture {
    pub key: ((HorizonKey, bool, i64, i64, usize), u32, u32, u32, u32, usize, usize),
    pub tex: egui::TextureHandle,
}

/// The widest the panorama's picture is drawn (pixels).
const PANO_MAX_W: usize = 1800;

/// A named summit as seen from the place.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Named {
    pub name: String,
    pub az: f64,
    /// Degrees above the camera's horizon, metres from the place, metres above the sea.
    pub alt: f64,
    pub dist: f64,
    pub ele: f64,
}

/// The named summits (OpenStreetMap) that stand on or above the skyline from the place.
pub(super) fn visible_names(sky: &Skyline, lat: f64, lon: f64, peaks: &[crate::services::peaks::Peak]) -> Vec<Named> {
    let mut out: Vec<Named> = peaks
        .iter()
        .filter_map(|p| {
            let s = light_studio_astro::align::sight(lat, lon, p.lat, p.lon, p.ele - (sky.ground + light_studio_astro::align::EYE));
            // (a summit under the skyline is hidden by something nearer; the skyline's own samples are a little rough)
            let shows = s.distance >= 300.0 && s.distance <= sky.far * 1.05 && s.altitude >= sky.altitude_at(s.azimuth) - 0.3;
            shows.then(|| Named { name: p.name.clone(), az: s.azimuth, alt: s.altitude, dist: s.distance, ele: p.ele })
        })
        .collect();
    out.sort_by(|a, b| b.ele.total_cmp(&a.ele));
    out.truncate(400);
    out
}

const DEFAULT_FOV: f32 = 110.0;
const MIN_FOV: f32 = 20.0;
const COMPASS_H: f32 = 20.0;

pub(super) fn key_of(st: &PlannerState) -> HorizonKey {
    (st.lat.to_bits(), st.lon.to_bits())
}

/// Where the skyline goes: the map's free part, beside the three icons of the search, date and time (when there is
/// room for it; on a narrow window it runs underneath them; their cards open over it).
pub(super) fn view_rect(free: Rect) -> Rect {
    let top = free.top();
    let beside = super::layout::island_rects(free, 3).chip.right() + 8.0;
    let left = if free.right() - 8.0 - beside >= 420.0 { beside } else { free.left() + 8.0 };
    let r = Rect::from_min_max(pos2(left, top.min(free.bottom())), pos2(free.right() - 8.0, free.bottom() - 8.0));
    if r.width() > 0.0 && r.height() > 0.0 { r } else { Rect::from_min_size(r.min, vec2(0.0, 0.0)) }
}

/// The sampling the ground heights are kept under, by terrain source: with the finer terrain near the place, or the AWS
/// tiles alone.
const TAG_FINE: &str = "mh";
const TAG_AWS: &str = "aws";

fn heights_tag(source: &str, grid: horizon::Grid) -> String {
    format!("{source}{}x{}", grid.azimuths, grid.rings)
}

/// The ground heights of `grid` round a place and whether the finer terrain is in them: kept from before, else read
/// from the terrain tiles (and kept, for the whole look only: the quick one is cheap and only a stop on the way).
fn read_heights(grid: horizon::Grid, lat: f64, lon: f64, keep: bool) -> Result<(Vec<f64>, bool), String> {
    use crate::services::place_cache as kept;
    if keep {
        if let Some(h) = kept::load_heights(&heights_tag(TAG_FINE, grid), lat, lon, grid.points()) {
            return Ok((h, true));
        }
        // (kept without the finer terrain: used while it cannot be had, else read again with it)
        if crate::services::terrain_fine::given_up()
            && let Some(h) = kept::load_heights(&heights_tag(TAG_AWS, grid), lat, lon, grid.points())
        {
            return Ok((h, false));
        }
    }
    let (h, detail) = crate::services::terrain::heights_detailed(&grid.sample_points(lat, lon))?;
    if keep {
        kept::store_heights(&heights_tag(if detail { TAG_FINE } else { TAG_AWS }, grid), lat, lon, &h);
    }
    Ok((h, detail))
}

/// Starts the ground heights round the place when the strip is on and they are not at hand (on a worker thread). A place
/// not seen before first gets a quick look (15 km, few tiles) and then the whole one (60 km); one seen before opens at
/// once from what was kept.
pub(super) fn fetch(app: &mut App) {
    let st = &app.ui.planner;
    if !st.show_horizon {
        return;
    }
    let key = key_of(st);
    if app.planner.horizon.as_ref().is_some_and(|h| h.key == key && h.complete) || app.planner.horizon_pending == Some(key) {
        return;
    }
    let (lat, lon) = (st.lat, st.lon);
    // the quick look only for a place with nothing kept, and only once
    let quick = app.planner.horizon.as_ref().is_none_or(|h| h.key != key)
        && [TAG_FINE, TAG_AWS]
            .iter()
            .all(|t| !crate::services::place_cache::has_heights(&heights_tag(t, horizon::Grid::FINE), lat, lon, horizon::Grid::FINE.points()));
    app.planner.horizon_pending = Some(key);
    let started = crate::tasks::spawn(
        app,
        "Mountains",
        Some("download"),
        move || -> Result<(Arc<Skyline>, Arc<Vec<Named>>, bool, Option<String>, Option<Arc<Surface>>, bool), String> {
            // the fine skyline from the terrain tiles; without them (offline, no tiles kept), the coarse one from the web service
            let grid = if quick { horizon::Grid::QUICK } else { horizon::Grid::FINE };
            let mut surface: Option<Arc<Surface>> = None;
            let mut detail = false;
            let from_tiles = match read_heights(grid, lat, lon, !quick) {
                Ok((h, d)) => {
                    detail = d;
                    surface = Surface::new(grid, &h).map(Arc::new);
                    horizon::skyline_for(grid, &h)
                }
                Err(e) => {
                    log::warn!("mountains: terrain tiles: {e}");
                    None
                }
            };
            let (sky, is_fine) = match from_tiles {
                Some(s) => (s, true),
                None => {
                    let points = horizon::sample_points(lat, lon);
                    let heights = crate::services::elevation::fetch_up_to(&points, crate::services::elevation::MAX_PANORAMA_POINTS)?;
                    (horizon::skyline(&heights).ok_or_else(|| "the ground heights were not what was asked for".to_string())?, false)
                }
            };
            // the names are a bonus: without them the summits keep their height and distance
            let mut names_error: Option<String> = None;
            let names = match crate::services::peaks::near(lat, lon) {
                Ok(p) => visible_names(&sky, lat, lon, &p),
                Err(e) => {
                    log::warn!("mountains: summit names: {e}");
                    names_error = Some(e);
                    Vec::new()
                }
            };
            Ok((Arc::new(sky), Arc::new(names), is_fine, names_error, surface, detail))
        },
        move |app, _ctx, data| {
            if app.planner.horizon_pending == Some(key) {
                app.planner.horizon_pending = None;
            }
            // (the quick look is not the end of it when the tiles were there; a failure is final, so it is not tried again and again)
            app.planner.horizon = Some(match data {
                Ok((sky, names, fine, names_error, surface, detail)) => {
                    let complete = !(quick && fine);
                    Horizon { key, data: Ok(sky), names, fine, names_error, surface, complete, detail }
                }
                Err(e) => Horizon {
                    key,
                    data: Err(e),
                    names: Arc::new(Vec::new()),
                    fine: false,
                    names_error: None,
                    surface: None,
                    complete: true,
                    detail: false,
                },
            });
        },
    );
    if let Err(e) = started {
        app.planner.horizon_pending = None;
        app.planner.horizon = Some(Horizon {
            key,
            data: Err(e),
            names: Arc::new(Vec::new()),
            fine: false,
            names_error: None,
            surface: None,
            complete: true,
            detail: false,
        });
    }
}

/// Starts the aerial photo colours of the ground round the place, once the ground is there (on a worker thread). The
/// quick ground gets its own, smaller photo, so colour shows early too.
pub(super) fn fetch_photo(app: &mut App) {
    let st = &app.ui.planner;
    if !st.show_horizon {
        return;
    }
    let key = key_of(st);
    let Some(h) = app.planner.horizon.as_ref().filter(|h| h.key == key && h.surface.is_some()) else {
        return;
    };
    let complete = h.complete;
    if app.planner.horizon_photo.as_ref().is_some_and(|p| p.0 == key && p.1 == complete) || app.planner.horizon_photo_pending == Some((key, complete))
    {
        return;
    }
    let (lat, lon) = (st.lat, st.lon);
    app.planner.horizon_photo_pending = Some((key, complete));
    let started = crate::tasks::spawn(
        app,
        "Mountains colour",
        Some("download"),
        move || -> Result<Arc<Vec<Photo>>, String> {
            let grid = if complete { horizon::Grid::FINE } else { horizon::Grid::QUICK };
            let tag = format!("{}{}x{}", crate::services::ortho::source(lat, lon).1, grid.azimuths, grid.rings);
            let count = grid.azimuths * grid.rings;
            if let Some(p) = crate::services::place_cache::load_photo(&tag, lat, lon, count) {
                return Ok(Arc::new(p));
            }
            let points = grid.sample_points(lat, lon);
            let samples = points.get(1..).ok_or("no samples")?;
            let p = crate::services::ortho::photo(lat, lon, samples, grid.rings, &grid.ranges())?;
            // (a picture with gaps is not kept: the missing tiles may come next time)
            if complete && p.iter().all(|c| c[3] == 255) {
                crate::services::place_cache::store_photo(&tag, lat, lon, &p);
            }
            Ok(Arc::new(p))
        },
        move |app, _ctx, photo| {
            if app.planner.horizon_photo_pending == Some((key, complete)) {
                app.planner.horizon_photo_pending = None;
            }
            app.planner.horizon_photo = Some((key, complete, photo));
        },
    );
    if let Err(e) = started {
        app.planner.horizon_photo_pending = None;
        app.planner.horizon_photo = Some((key, complete, Err(e)));
    }
}

/// The sky's colour for the Sun at `alt` degrees: night, dusk, day.
/// One soft round patch of the Milky Way's glow: where, how wide (degrees) and what colour.
pub(super) struct Blob {
    pub az: f64,
    pub alt: f64,
    pub radius: f32,
    pub colour: Color32,
}

/// The glow of the galactic plane as overlapping faint patches along it: wide and warm at the core, narrower and cooler
/// away from it, a brighter thin band down the middle. `dark` is 0 (twilight, nothing) to 1 (a dark sky).
pub(super) fn milky_way_blobs(plane: &[light_studio_astro::sky::PlanePoint], dark: f32) -> Vec<Blob> {
    if dark <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for p in plane.iter().filter(|p| p.at.alt > -8.0 && p.at.alt.is_finite() && p.at.az.is_finite()) {
        let away = (p.l + 180.0).rem_euclid(360.0) - 180.0;
        let k = (1.0 - (away.abs() / 180.0) as f32).clamp(0.0, 1.0);
        let warm = k * k;
        let tint = |cool: [u8; 3], hot: [u8; 3]| -> [u8; 3] {
            std::array::from_fn(|i| (f32::from(cool[i]) * (1.0 - warm) + f32::from(hot[i]) * warm) as u8)
        };
        let rgb = tint([185, 200, 255], [255, 226, 180]);
        let alpha = |a: f32| (a * dark).round().clamp(0.0, 255.0) as u8;
        // the wide halo, then the brighter thin band
        out.push(Blob {
            az: p.at.az,
            alt: p.at.alt,
            radius: 5.0 + 6.0 * warm,
            colour: Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], alpha(6.0 + 3.0 * warm)),
        });
        out.push(Blob {
            az: p.at.az,
            alt: p.at.alt,
            radius: 2.2 + 2.5 * warm,
            colour: Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], alpha(5.0 + 5.0 * warm)),
        });
    }
    out
}

pub(super) fn sky_colours(sun_alt: f64, dark_ui: bool) -> (Color32, Color32) {
    let k = ((sun_alt + 14.0) / 20.0).clamp(0.0, 1.0) as f32;
    let night = (Color32::from_rgb(8, 12, 28), Color32::from_rgb(26, 36, 66));
    let day = (Color32::from_rgb(82, 128, 196), Color32::from_rgb(176, 205, 232));
    let dusk = if dark_ui {
        (Color32::from_rgb(38, 44, 84), Color32::from_rgb(170, 110, 96))
    } else {
        (Color32::from_rgb(70, 84, 140), Color32::from_rgb(220, 160, 130))
    };
    let mix = |a: (Color32, Color32), b: (Color32, Color32), f: f32| (a.0.lerp_to_gamma(b.0, f), a.1.lerp_to_gamma(b.1, f));
    if k < 0.5 { mix(night, dusk, k * 2.0) } else { mix(dusk, day, (k - 0.5) * 2.0) }
}

impl Page<'_> {
    /// The day as a slider under the skyline (the time bar is not shown here): the track is tinted by the Sun's height, so
    /// night, dusk and day can be told apart, and dragging it moves the Sun and the Moon on the skyline at once.
    pub(super) fn time_slider(&mut self, ui: &mut Ui, st: &mut PlannerState, r: Rect) {
        if r.width() < 120.0 || r.height() < 20.0 {
            return;
        }
        let t = self.t;
        let track = Rect::from_min_max(pos2(r.left() + 40.0, r.center().y - 7.0), pos2(r.right() - 40.0, r.center().y + 7.0));
        let resp = ui.interact(r, egui::Id::new("planner-mountains-time"), Sense::click_and_drag());
        if (resp.dragged() || resp.clicked())
            && let Some(p) = resp.interact_pointer_pos()
        {
            let f = f64::from(((p.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0));
            st.minute = Some((f * 1439.0).round());
        }
        let p = ui.painter();
        p.rect_filled(r, R_CONTROL, t.inset.gamma_multiply(0.9));
        let dark = ui.visuals().dark_mode;
        // the track, in slices coloured by the Sun's height at that time of the day
        let slices = 96;
        for k in 0..slices {
            let (f0, f1) = (k as f32 / slices as f32, (k + 1) as f32 / slices as f32);
            let minute = (f64::from(f0) + f64::from(f1)) / 2.0 * 1439.0;
            let alt = light_studio_astro::sun_horizontal(self.c.start + minute * 60.0, self.obs).alt;
            let (_, tint) = sky_colours(alt, dark);
            let slice = Rect::from_min_max(
                pos2(track.left() + track.width() * f0, track.top()),
                pos2(track.left() + track.width() * f1 + 0.5, track.bottom()),
            );
            p.rect_filled(slice, 0.0, tint);
        }
        p.rect_stroke(track, 7.0, egui::Stroke::new(1.0, t.divider), egui::StrokeKind::Inside);
        for h in [0, 6, 12, 18, 24] {
            let x = track.left() + track.width() * h as f32 / 24.0;
            p.text(pos2(x, r.bottom() - 2.0), Align2::CENTER_BOTTOM, format!("{h:02}"), t.font(9.5), t.text_dim);
        }
        let x = track.left() + track.width() * (self.minute / 1439.0).clamp(0.0, 1.0) as f32;
        p.circle_filled(pos2(x, track.center().y), 8.0, Color32::WHITE);
        p.circle_stroke(pos2(x, track.center().y), 8.0, egui::Stroke::new(1.5, t.accent));
        p.text(pos2(r.left() + 6.0, track.center().y), Align2::LEFT_CENTER, clock_minutes(self.minute), t.semibold(11.5), t.text);
    }

    /// The panorama's picture for the view the strip shows: the ground in colour for the Sun where it is, drawn from the
    /// place's samples. Kept while nothing it depends on changes. `None` if it cannot be drawn.
    #[allow(clippy::too_many_arguments)]
    fn pano_picture(
        &self,
        ctx: &egui::Context,
        rt: &mut Runtime,
        key: HorizonKey,
        surf: &Surface,
        inner: Rect,
        centre: f32,
        fov: f32,
        sy: f32,
        y0: f32,
        haze: Color32,
    ) -> Option<egui::TextureId> {
        let sun = self.skies.sun;
        let complete = surf.grid().rings == horizon::Grid::FINE.rings;
        let photo = rt.horizon_photo.as_ref().filter(|p| p.0 == key && p.1 == complete).and_then(|p| p.2.as_ref().ok()).cloned();
        // (the quick and the whole ground have their own colours: the number of rings tells them apart)
        let ckey = (key, photo.is_some(), (sun.az * 2.0).round() as i64, (sun.alt * 2.0).round() as i64, surf.grid().rings);
        if rt.pano_colours.as_ref().is_none_or(|c| c.key != ckey) {
            let colours = surf.colours(sun, photo.as_deref().map(Vec::as_slice), [haze.r(), haze.g(), haze.b()]);
            rt.pano_colours = Some(PanoColours { key: ckey, colours: Arc::new(colours) });
        }
        let colours = Arc::clone(&rt.pano_colours.as_ref()?.colours);
        // one picture pixel to a screen pixel, up to a cap on the width
        let w = ((inner.width() * ctx.pixels_per_point()) as usize).clamp(16, PANO_MAX_W);
        let scale = w as f32 / inner.width().max(1.0);
        let h = ((inner.height() * scale) as usize).clamp(16, 4096);
        let tkey = (ckey, centre.to_bits(), fov.to_bits(), sy.to_bits(), y0.to_bits(), w, h);
        if rt.pano_texture.as_ref().is_none_or(|t| t.key != tkey) {
            let view = PanoView {
                width: w,
                height: h,
                az_left: f64::from(centre) - f64::from(fov) / 2.0,
                deg_x: f64::from(fov) / w as f64,
                alt_top: f64::from(y0 - inner.top()) / f64::from(sy),
                deg_y: 1.0 / (f64::from(sy) * f64::from(scale)),
            };
            let rgba = surf.render(&colours, &view);
            if rgba.is_empty() {
                return None;
            }
            let tex = ctx.load_texture(
                "planner-panorama",
                egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba),
                egui::TextureOptions {
                    magnification: egui::TextureFilter::Linear,
                    minification: egui::TextureFilter::Linear,
                    wrap_mode: egui::TextureWrapMode::ClampToEdge,
                    mipmap_mode: None,
                },
            );
            rt.pano_texture = Some(PanoTexture { key: tkey, tex });
        }
        rt.pano_texture.as_ref().map(|t| t.tex.id())
    }

    pub(super) fn horizon_strip(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, r: Rect) {
        let t = self.t;
        let pal = self.pal;
        if r.width() < 160.0 || r.height() < 90.0 {
            return;
        }
        let shadow = egui::epaint::Shadow { offset: [0, 4], blur: 18, spread: 0, color: t.shadow };
        ui.painter().add(shadow.as_shape(r, egui::CornerRadius::same(R_CARD as u8)));
        let key = key_of(st);
        let sky = rt.horizon.as_ref().filter(|h| h.key == key).cloned();

        // the header: the title, where to look, and the close button
        let head = Rect::from_min_size(r.min, vec2(r.width(), 30.0));
        let body = Rect::from_min_max(pos2(r.left(), head.bottom()), r.max);
        let sun_alt = self.skies.sun.alt;
        let (top, low) = sky_colours(sun_alt, t.backdrop.r() < 128);

        // sky: a gradient from the top down to the horizon (over the ground afterwards)
        let painter = ui.painter_at(r);
        let mut m = egui::Mesh::default();
        m.colored_vertex(body.left_top(), top);
        m.colored_vertex(body.right_top(), top);
        m.colored_vertex(body.right_bottom(), low);
        m.colored_vertex(body.left_bottom(), low);
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        // the card's top corners are rounded: the header has the card's colour, the body the sky
        painter.rect_filled(r, R_CARD, t.chrome);
        painter.add(Shape::mesh(m));
        painter.rect_filled(head, egui::CornerRadius { nw: R_CARD as u8, ne: R_CARD as u8, sw: 0, se: 0 }, t.chrome);

        let inner = Rect::from_min_max(pos2(body.left(), body.top()), pos2(body.right(), body.bottom() - COMPASS_H));
        let fov = if rt.pano_fov >= MIN_FOV { rt.pano_fov.min(360.0) } else { DEFAULT_FOV };
        let centre = rt.pano_az.unwrap_or(self.skies.sun.az as f32);
        let ppd = inner.width() / fov;
        let x_of = |az: f64| inner.center().x + (((az as f32 - centre + 540.0).rem_euclid(360.0)) - 180.0) * ppd;
        let az_at = |x: f32| f64::from((centre + (x - inner.center().x) / ppd).rem_euclid(360.0));

        // how tall the sky is drawn: true angles, stretched (at most three times) when the mountains are low
        let data = sky.as_ref().and_then(|h| h.data.as_ref().ok()).cloned();
        let mut max_in_view = data.as_ref().map_or(0.0, |s| {
            (0..=40).map(|k| s.altitude_at(f64::from(centre) - f64::from(fov) / 2.0 + f64::from(fov) * f64::from(k) / 40.0)).fold(0.0, f64::max)
        });
        // the Sun, the Moon and the core stay in the picture when they are in the directions shown
        for (i, h) in [(0usize, self.skies.sun), (1, self.skies.moon), (2, self.skies.core)] {
            let off = ((h.az as f32 - centre + 540.0).rem_euclid(360.0) - 180.0).abs();
            if st.layers[i] && off < fov / 2.0 {
                max_in_view = max_in_view.max(h.alt);
            }
        }
        let avail = inner.height() - 14.0;
        let painter_sky = painter.with_clip_rect(inner);
        let sy = (avail / (max_in_view as f32 + 5.0).max(8.0)).min(ppd * 3.0).max(0.5);
        let y0 =
            inner.bottom() - 6.0 - (-(data.as_ref().map_or(0.0, |s| s.alt.iter().copied().fold(f64::MAX, f64::min).min(0.0)) as f32) * sy).min(14.0);
        let y_of = |alt: f64| y0 - alt as f32 * sy;

        // the Milky Way: a soft band along the galactic plane in a dark sky, behind the ground
        if st.layers[2] {
            let dark = ((-sun_alt - 8.0) / 10.0).clamp(0.0, 1.0) as f32;
            if dark > 0.0 {
                let plane = light_studio_astro::sky::milky_way_plane(self.c.start + self.minute * 60.0, self.obs, 2.0);
                let unit = (ppd * sy).sqrt();
                for b in milky_way_blobs(&plane, dark) {
                    let p = pos2(x_of(b.az), y_of(b.alt));
                    let r = (b.radius * unit).max(1.0);
                    if inner.expand(r).contains(p) {
                        painter_sky.circle_filled(p, r, b.colour);
                    }
                }
            }
        }

        // the ground: in colour (relief lit by the Sun, aerial photo, haze) when the fine terrain is there, else a silhouette
        let picture = sky
            .as_ref()
            .and_then(|h| h.surface.clone())
            .and_then(|surf| self.pano_picture(ui.ctx(), rt, key, &surf, inner, centre, fov, sy, y0, low));
        if let Some(tex) = picture {
            painter.image(tex, inner, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        } else if let Some(s) = &data {
            let mut mesh = egui::Mesh::default();
            let mut x = inner.left();
            while x < inner.right() {
                let x2 = (x + 1.5).min(inner.right());
                let (a1, a2) = (az_at(x), az_at(x2));
                let near = |az: f64| {
                    let d = s.distance_at(az);
                    let k = ((d / s.near).max(1.0).ln() / (s.far / s.near).ln()).clamp(0.0, 1.0) as f32;
                    Color32::from_rgb(28, 32, 30).lerp_to_gamma(Color32::from_rgb(98, 112, 138), k.powf(0.8))
                };
                let (c1, c2) = (near(a1), near(a2));
                let bottom = |c: Color32| c.gamma_multiply(0.55);
                let (y1, y2) = (y_of(s.altitude_at(a1)), y_of(s.altitude_at(a2)));
                let base = mesh.vertices.len() as u32;
                mesh.colored_vertex(pos2(x, y1), c1);
                mesh.colored_vertex(pos2(x2, y2), c2);
                mesh.colored_vertex(pos2(x2, inner.bottom()), bottom(c2));
                mesh.colored_vertex(pos2(x, inner.bottom()), bottom(c1));
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
                x = x2;
            }
            painter.add(Shape::mesh(mesh));
        } else {
            painter.rect_filled(Rect::from_min_max(pos2(inner.left(), y0), inner.right_bottom()), 0.0, Color32::from_rgb(30, 34, 32));
        }
        // the true horizon
        painter.line_segment([pos2(inner.left(), y0), pos2(inner.right(), y0)], Stroke::new(1.0, Color32::from_white_alpha(46)));

        // the paths of the Sun and the Moon over the day, dotted
        let sky_p = &painter_sky;
        let path = |f: &dyn Fn(f64) -> Horizontal, color: Color32| {
            let mut prev: Option<Pos2> = None;
            for k in 0..=144 {
                let h = f(self.c.start + f64::from(k) * 600.0);
                if h.alt < -3.0 {
                    prev = None;
                    continue;
                }
                let p = pos2(x_of(h.az), y_of(h.alt));
                if let Some(q) = prev
                    && (p.x - q.x).abs() < inner.width() / 2.0
                    && k % 2 == 0
                {
                    sky_p.line_segment([q, p], Stroke::new(1.5, color.gamma_multiply(0.7)));
                }
                prev = Some(p);
            }
        };
        if st.layers[0] {
            path(&|u| light_studio_astro::sun_horizontal(u, self.obs), pal.sun);
        }
        if st.layers[1] {
            path(&|u| light_studio_astro::moon_horizontal(u, self.obs), pal.moon);
        }

        // the summits: named ones (OpenStreetMap) first, then the ones that only stand out; each in the lowest row where
        // it does not cover another, with a line down to its peak
        if let Some(s) = &data {
            struct Label {
                x: f32,
                y: f32,
                title: String,
                sub: String,
            }
            let names = sky.as_ref().map(|h| h.names.clone()).unwrap_or_default();
            let mut labels: Vec<Label> = Vec::new();
            for n in names.iter() {
                labels.push(Label {
                    x: x_of(n.az),
                    y: y_of(n.alt),
                    title: n.name.clone(),
                    sub: format!("{:.0} m · {}", n.ele, plans::distance_label(n.dist)),
                });
            }
            let diff = |a: f64, b: f64| ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs();
            for p in s.peaks(0.8).into_iter().take(24) {
                if names.iter().any(|n| diff(n.az, p.az) < 1.5) {
                    continue;
                }
                labels.push(Label { x: x_of(p.az), y: y_of(p.alt), title: format!("{:.0} m", p.height), sub: plans::distance_label(p.dist) });
            }
            const ROWS: usize = 5;
            let mut rows: Vec<Vec<(f32, f32)>> = vec![Vec::new(); ROWS];
            let mut placed = 0;
            for l in labels {
                if placed >= 16 {
                    break;
                }
                if l.x < inner.left() + 14.0 || l.x > inner.right() - 14.0 {
                    continue;
                }
                let g1 = painter.layout_no_wrap(l.title, t.semibold(11.0), Color32::WHITE);
                let g2 = painter.layout_no_wrap(l.sub, t.font(10.0), Color32::from_white_alpha(190));
                let w = g1.size().x.max(g2.size().x) + 14.0;
                let span = (l.x - w / 2.0 - 3.0, l.x + w / 2.0 + 3.0);
                let Some(row) = rows.iter().position(|r| r.iter().all(|(a, b)| span.1 < *a || span.0 > *b)) else { continue };
                let cy = l.y - 26.0 - row as f32 * 34.0;
                if cy - 15.0 < body.top() + 2.0 {
                    continue;
                }
                if let Some(r) = rows.get_mut(row) {
                    r.push(span);
                }
                placed += 1;
                let chip = Rect::from_center_size(pos2(l.x, cy), vec2(w, 30.0));
                painter.line_segment([pos2(l.x, l.y - 2.0), pos2(l.x, chip.bottom())], Stroke::new(1.0, Color32::from_white_alpha(120)));
                painter.rect_filled(chip, 6.0, Color32::from_black_alpha(140));
                painter.galley(pos2(chip.center().x - g1.size().x / 2.0, chip.top() + 2.0), g1, Color32::WHITE);
                painter.galley(pos2(chip.center().x - g2.size().x / 2.0, chip.top() + 15.0), g2, Color32::WHITE);
            }
            // the credits
            let fine = sky.as_ref().is_some_and(|h| h.fine);
            let detail = sky.as_ref().is_some_and(|h| h.detail);
            let mut credit = String::from(match (fine, detail) {
                (true, true) => "Terrain: © Mapterhorn · AWS Terrain Tiles",
                (true, false) => "Terrain: AWS Terrain Tiles",
                _ => "Terrain: Copernicus DEM (Open-Meteo)",
            });
            if !names.is_empty() {
                credit.push_str(" · Names: © OpenStreetMap contributors");
            } else if let Some(why) = sky.as_ref().and_then(|h| h.names_error.as_deref()) {
                credit.push_str(&format!(" · {}: {why}", crate::i18n::tr("Summit names unavailable")));
            }
            if sky.as_ref().is_some_and(|h| h.surface.is_some()) {
                let complete = sky.as_ref().is_some_and(|h| h.complete);
                match rt.horizon_photo.as_ref().filter(|p| p.0 == key && p.1 == complete).map(|p| &p.2) {
                    Some(Ok(_)) => credit.push_str(&format!(" · {}", crate::services::ortho::source(st.lat, st.lon).2)),
                    Some(Err(e)) => credit.push_str(&format!(" · {}: {e}", crate::i18n::tr("Photo colours unavailable"))),
                    None => credit.push_str(&format!(" · {}", crate::i18n::tr("Loading photo colours…"))),
                }
            }
            let g = painter.layout_no_wrap(credit, t.font(9.5), Color32::from_white_alpha(150));
            painter.galley(pos2(inner.left() + 8.0, inner.bottom() - g.size().y - 3.0), g, Color32::WHITE);
        }

        // the target, if there is one
        if let Some(s) = plans::sight_of(st, None) {
            let x = x_of(s.azimuth);
            if inner.x_range().contains(x) {
                let y = y_of(s.altitude);
                painter.add(Shape::convex_polygon(
                    vec![pos2(x, y), pos2(x - 5.0, y - 9.0), pos2(x + 5.0, y - 9.0)],
                    t.accent,
                    Stroke::new(1.0, Color32::WHITE),
                ));
            }
        }

        // the bodies at the time shown (hollow when the mountains hide them)
        let bodies = [(0usize, self.skies.sun, pal.sun, 8.0f32), (1, self.skies.moon, pal.moon, 7.0), (2, self.skies.core, pal.core, 6.0)];
        for (i, h, color, rad) in bodies {
            if !st.layers[i] || h.alt < -4.0 {
                continue;
            }
            let p = pos2(x_of(h.az), y_of(h.alt));
            if !inner.expand(rad).contains(p) {
                continue;
            }
            let hidden = data.as_ref().is_some_and(|s| h.alt < s.altitude_at(h.az));
            if hidden {
                painter.circle_stroke(p, rad, Stroke::new(1.8, color));
            } else {
                painter.circle_filled(p, rad + 4.0, color.gamma_multiply(0.25));
                painter.circle_filled(p, rad, color);
            }
        }

        // the core below the horizon: said where it will come up, so a missing dot is not taken for a fault
        let core = self.skies.core;
        if st.layers[2] && core.alt < -4.0 && ((core.az as f32 - centre + 540.0).rem_euclid(360.0) - 180.0).abs() < fov / 2.0 {
            let x = x_of(core.az).clamp(inner.left() + 8.0, inner.right() - 8.0);
            let mut text = crate::i18n::tr("Milky Way core below the horizon").to_string();
            if let Some(rise) = self.c.plan.core.rise {
                text.push_str(&format!(" · {} {}", crate::i18n::tr("Core Rises"), clock(rise, self.c.offset)));
            }
            let g = painter.layout_no_wrap(text, t.font(10.5), Color32::WHITE);
            let size = g.size() + vec2(16.0, 8.0);
            let chip = Rect::from_center_size(
                pos2(x.clamp(inner.left() + size.x / 2.0 + 4.0, inner.right() - size.x / 2.0 - 4.0), inner.top() + 16.0),
                size,
            );
            painter.rect_filled(chip, 8.0, Color32::from_black_alpha(150));
            painter.galley(chip.center() - g.size() / 2.0, g, Color32::WHITE);
            painter.circle_stroke(pos2(x, y0), 6.0, Stroke::new(1.8, pal.core));
        }

        // the compass strip
        let cs = Rect::from_min_max(pos2(body.left(), body.bottom() - COMPASS_H), body.right_bottom());
        painter.rect_filled(cs, egui::CornerRadius { nw: 0, ne: 0, sw: R_CARD as u8, se: R_CARD as u8 }, Color32::from_black_alpha(150));
        let first = (f64::from(centre - fov / 2.0) / 15.0).floor() as i32 - 1;
        let last = (f64::from(centre + fov / 2.0) / 15.0).ceil() as i32 + 1;
        for k in first..=last {
            let az = f64::from(k) * 15.0;
            let x = x_of(az);
            if !inner.x_range().contains(x) {
                continue;
            }
            let az360 = az.rem_euclid(360.0);
            let main = (az360 % 45.0).abs() < 1e-6;
            painter.line_segment(
                [pos2(x, cs.top()), pos2(x, cs.top() + if main { 7.0 } else { 4.0 })],
                Stroke::new(1.0, Color32::from_white_alpha(if main { 200 } else { 110 })),
            );
            if main {
                let label = if az360 % 90.0 == 0.0 { crate::i18n::tr_ctx("compass", compass(az360)).to_string() } else { format!("{az360:.0}°") };
                painter.text(pos2(x, cs.bottom() - 6.0), Align2::CENTER_CENTER, label, t.semibold(10.5), Color32::WHITE);
            }
        }

        // interaction: drag to turn, wheel to zoom, a double click to look at the Sun
        let resp = ui.interact(inner, egui::Id::new("planner-horizon"), Sense::click_and_drag());
        let mut new_az = centre;
        let mut new_fov = fov;
        if resp.dragged() {
            new_az = (centre - resp.drag_delta().x / ppd).rem_euclid(360.0);
        }
        if resp.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            new_fov = (fov * (-scroll.y * 0.004).exp() / zoom).clamp(MIN_FOV, 360.0);
            if scroll.x.abs() > 0.0 {
                new_az = (new_az - scroll.x / ppd).rem_euclid(360.0);
            }
        }
        if resp.double_clicked() {
            new_az = self.skies.sun.az as f32;
        }
        rt.pano_az = Some(new_az);
        rt.pano_fov = new_fov;
        // what is under the pointer
        let mut readout = String::new();
        if resp.hovered()
            && let (Some(pos), Some(s)) = (resp.hover_pos(), &data)
        {
            let az = az_at(pos.x);
            let i = s.index_at(az);
            readout = format!(
                "{} {}  ·  {:.1}°  ·  {:.0} m {} {}",
                az_text(az),
                crate::i18n::tr_ctx("compass", compass(az)),
                s.altitude_at(az),
                s.height.get(i).copied().unwrap_or(0.0),
                crate::i18n::tr("at"),
                plans::distance_label(s.dist.get(i).copied().unwrap_or(0.0))
            );
        }

        // the card's frame and header
        painter.rect_stroke(r, R_CARD, Stroke::new(1.0, t.card_stroke), StrokeKind::Inside);
        let mut hui = ui.new_child(egui::UiBuilder::new().max_rect(head.shrink2(vec2(12.0, 3.0))).layout(egui::Layout::left_to_right(Align::Center)));
        hui.spacing_mut().item_spacing.x = 6.0;
        hui.label(egui::RichText::new(crate::i18n::tr("Mountains")).font(t.semibold(13.0)).color(t.text));
        for (id, label, az) in [
            ("plannerHorizonSun", "Sun", self.skies.sun.az),
            ("plannerHorizonMoon", "Moon", self.skies.moon.az),
            ("plannerHorizonCore", "Milky Way", self.skies.core.az),
        ] {
            if text_button(&mut hui, id, label, false).clicked() {
                rt.pano_az = Some(az as f32);
            }
        }
        if let Some(s) = plans::sight_of(st, None)
            && text_button(&mut hui, "plannerHorizonTarget", "Target", false).clicked()
        {
            rt.pano_az = Some(s.azimuth as f32);
        }
        let mut right =
            ui.new_child(egui::UiBuilder::new().max_rect(head.shrink2(vec2(12.0, 3.0))).layout(egui::Layout::right_to_left(Align::Center)));
        right.spacing_mut().item_spacing.x = 6.0;
        if icon_button(&mut right, "plannerHorizonClose", Icon::Minus, vec2(24.0, 24.0), false, true, "Hide the Mountains").clicked() {
            st.show_horizon = false;
        }
        let stretch = if sy > ppd * 1.05 { format!("  ·  ×{:.1}", sy / ppd) } else { String::new() };
        let info = if readout.is_empty() { format!("{fov:.0}°{stretch}") } else { readout };
        right.label(egui::RichText::new(info).font(t.font(11.5)).color(t.text_dim));

        // while it is being looked up, or if it could not be
        let centre_text = match sky.as_ref().map(|h| &h.data) {
            Some(Err(e)) => Some(format!("{}: {e}", crate::i18n::tr("Could not look up the ground heights"))),
            None => Some(crate::i18n::tr("Looking up the ground heights…").to_string()),
            Some(Ok(_)) => None,
        };
        if let Some(msg) = centre_text {
            let chip = Rect::from_center_size(inner.center(), vec2((inner.width() - 40.0).min(520.0), 34.0));
            painter.rect_filled(chip, 10.0, Color32::from_black_alpha(150));
            painter.text(chip.center(), Align2::CENTER_CENTER, msg, t.font(12.0), Color32::WHITE);
            if matches!(sky.as_ref().map(|h| &h.data), Some(Err(_))) {
                let retry = Rect::from_center_size(inner.center() + vec2(0.0, 40.0), vec2(90.0, 28.0));
                let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(retry));
                if text_button(&mut rui, "plannerHorizonRetry", "Try Again", false).clicked() {
                    rt.horizon = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_milky_way_glow_is_widest_and_warmest_at_the_core_and_needs_a_dark_sky() {
        use light_studio_astro::{Horizontal, sky::PlanePoint};
        let plane: Vec<PlanePoint> = [0.0, 90.0, 180.0, 270.0]
            .iter()
            .map(|&l| PlanePoint { l, at: Horizontal { az: 180.0 + l / 4.0, alt: 20.0 } })
            .chain([PlanePoint { l: 45.0, at: Horizontal { az: 10.0, alt: -30.0 } }])
            .collect();
        assert!(milky_way_blobs(&plane, 0.0).is_empty(), "no glow in the twilight");
        let blobs = milky_way_blobs(&plane, 1.0);
        // two patches for each of the four points above the horizon; the one far below it is left out
        assert_eq!(blobs.len(), 8);
        let core = &blobs[0];
        assert!(blobs.iter().all(|b| b.radius <= core.radius));
        let rgba = |b: &Blob| b.colour.to_srgba_unmultiplied();
        assert!(rgba(core)[0] > 240 && rgba(core)[2] < 200, "warm at the core: {:?}", rgba(core));
        let far = &blobs[4]; // l = 180: the opposite side of the galaxy
        assert!(far.radius < core.radius && rgba(far)[2] > rgba(far)[0] - 10, "cool far from it: {:?}", rgba(far));
        // a fainter sky shows a fainter glow
        let dim = milky_way_blobs(&plane, 0.4);
        assert!(dim[0].colour.a() < core.colour.a(), "{} vs {}", dim[0].colour.a(), core.colour.a());
    }

    #[test]
    fn the_three_new_layers_start_off_are_kept_and_old_settings_still_load() {
        let st = PlannerState::default();
        assert!(!st.show_horizon && !st.show_shadows && !st.show_light);
        let on = PlannerState { show_horizon: true, show_shadows: true, show_light: true, ..Default::default() };
        let back: PlannerState = serde_json::from_str(&serde_json::to_string(&on).unwrap()).unwrap();
        assert!(back.show_horizon && back.show_shadows && back.show_light);
        let old: PlannerState = serde_json::from_str(r#"{"lat":43.0,"lon":-2.0}"#).unwrap();
        assert!(!old.show_horizon && !old.show_shadows && !old.show_light);
    }

    #[test]
    fn the_skyline_fills_the_free_part_beside_the_icons() {
        for (w, h) in [(300.0, 200.0), (1100.0, 700.0), (50.0, 30.0)] {
            let free = Rect::from_min_size(pos2(10.0, 10.0), vec2(w, h));
            let r = view_rect(free);
            assert!(free.contains_rect(r) || r.height() <= 0.0, "{w}×{h}: {r:?}");
            assert!(r.top() >= free.top() - 1e-3 || r.height() <= 0.0, "{w}×{h}: {r:?}");
            assert!(
                r.height() <= 0.0 || ((r.right() - (free.right() - 8.0)).abs() < 1e-3 && (r.bottom() - (free.bottom() - 8.0)).abs() < 1e-3),
                "{w}×{h}: {r:?}"
            );
        }
    }

    #[test]
    fn the_skyline_key_follows_the_place() {
        let a = PlannerState { lat: 43.0, lon: -2.0, ..Default::default() };
        let b = PlannerState { lat: 43.0, lon: -2.1, ..Default::default() };
        assert_eq!(key_of(&a), key_of(&a.clone()));
        assert_ne!(key_of(&a), key_of(&b));
    }
}
