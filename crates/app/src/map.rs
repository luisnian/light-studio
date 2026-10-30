//! The map camera and its tiles: a view over the Web Mercator world (centre and zoom), the maths between the screen and
//! the world, and the painting of the tiles that are at hand while the rest are asked for from [`map_tiles`].

use std::collections::HashMap;

use egui::{Color32, Pos2, Rect, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::services::map_tiles::{self, MapStyle, MapTiles, TileKey};

pub const MIN_ZOOM: f64 = 1.5;
pub const MAX_ZOOM: f64 = 19.0;

/// Points a tile is wide at zoom 0.
const TILE: f64 = 256.0;
/// Tiles kept on the GPU; the ones drawn longest ago are let go first.
const MAX_TEXTURES: usize = 160;
/// How opaque the light pollution atlas is over the map.
const LIGHT_OPACITY: f32 = 0.55;
/// Seconds before a tile that failed is asked for again.
const RETRY_SECS: f64 = 20.0;
/// The most tiles drawn across one row (guards against absurd zoom/rect pairs).
const MAX_ACROSS: i64 = 64;

/// Where the map looks: centre and zoom (saved with the settings).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MapView {
    pub lat: f64,
    pub lon: f64,
    pub zoom: f64,
}

impl Default for MapView {
    fn default() -> Self {
        MapView { lat: 40.4, lon: -3.7, zoom: 5.0 }
    }
}

impl MapView {
    /// The same view with anything unusable replaced: finite numbers, a latitude the projection covers, a longitude in
    /// -180..180 and a zoom within range.
    pub(crate) fn sane(self) -> MapView {
        let d = MapView::default();
        MapView {
            lat: if self.lat.is_finite() { self.lat.clamp(-85.0, 85.0) } else { d.lat },
            lon: if self.lon.is_finite() { (self.lon + 180.0).rem_euclid(360.0) - 180.0 } else { d.lon },
            zoom: if self.zoom.is_finite() { self.zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { d.zoom },
        }
    }
}

/// A camera over the world (0..1 both ways, x east, y south): the centre, and how many points the world measures.
#[derive(Clone, Copy)]
pub(crate) struct Cam {
    pub(crate) cx: f64,
    pub(crate) cy: f64,
    pub(crate) s: f64,
    /// Where the centre falls on screen.
    pub(crate) c: Pos2,
}

impl Cam {
    pub(crate) fn new(v: MapView, rect: Rect) -> Cam {
        let (cx, cy) = map_tiles::project(v.lat, v.lon);
        Cam { cx, cy, s: TILE * 2f64.powf(v.zoom), c: rect.center() }
    }

    /// Where a world point is on screen. The world repeats east to west, so the copy nearest the centre is the one used.
    pub(crate) fn screen(&self, wx: f64, wy: f64) -> Pos2 {
        let mut dx = wx - self.cx;
        dx -= dx.round();
        pos2(self.c.x + (dx * self.s) as f32, self.c.y + ((wy - self.cy) * self.s) as f32)
    }

    /// The world point under a screen position.
    pub(crate) fn world(&self, p: Pos2) -> (f64, f64) {
        (self.cx + f64::from(p.x - self.c.x) / self.s, self.cy + f64::from(p.y - self.c.y) / self.s)
    }
}

/// The view centred on a world point.
pub(crate) fn view_at(wx: f64, wy: f64, zoom: f64) -> MapView {
    let (lat, lon) = map_tiles::unproject(wx.rem_euclid(1.0), wy.clamp(0.0, 1.0));
    MapView { lat, lon, zoom }.sane()
}

/// `v` with its centre kept where the world fills the height of `rect` (no empty band past the poles).
pub(crate) fn clamp_view(v: MapView, rect: Rect) -> MapView {
    let v = v.sane();
    let cam = Cam::new(v, rect);
    let half = f64::from(rect.height()) / 2.0 / cam.s;
    let cy = if half < 0.5 { cam.cy.clamp(half, 1.0 - half) } else { 0.5 };
    if (cy - cam.cy).abs() < 1e-12 { v } else { view_at(cam.cx, cy, v.zoom) }
}

/// Zooms to `zoom`, keeping the world point under `at` where it is.
pub(crate) fn zoom_about(v: MapView, rect: Rect, at: Pos2, zoom: f64) -> MapView {
    let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let cam = Cam::new(v, rect);
    let (wx, wy) = cam.world(at);
    let s = TILE * 2f64.powf(zoom);
    view_at(wx - f64::from(at.x - cam.c.x) / s, wy - f64::from(at.y - cam.c.y) / s, zoom)
}

/// What the map keeps between frames (not saved): the tile service, the tiles on the GPU and the ones that failed.
#[derive(Default)]
pub struct Runtime {
    tiles: Option<MapTiles>,
    /// The CARTO key the service was made with.
    tiles_key: String,
    textures: HashMap<TileKey, (egui::TextureHandle, u64)>,
    retry: HashMap<TileKey, f64>,
    frame: u64,
}

impl Runtime {
    /// Marks a new frame: tiles drawn in it are kept, the ones drawn longest ago are let go.
    pub(crate) fn next_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// The credit the tiles in use must show (the satellite view has Esri's; the plain map, its source's).
    pub fn credit(&self, satellite: bool) -> String {
        if satellite {
            return crate::services::map_tiles::SATELLITE_ATTRIBUTION.to_string();
        }
        self.tiles.as_ref().map(|t| t.source().attribution().to_string()).unwrap_or_default()
    }

    /// Forgets the service and its tiles (the source changed).
    pub fn reset(&mut self) {
        self.tiles = None;
        self.textures.clear();
        self.retry.clear();
    }

    /// Takes in the tiles that arrived since the last frame.
    fn receive(&mut self, ctx: &egui::Context, now: f64) {
        let Some(service) = &self.tiles else { return };
        for (k, tile) in service.drain() {
            match tile {
                Some(tile) => {
                    let size = [tile.width as usize, tile.height as usize];
                    let img = egui::ColorImage::from_rgba_unmultiplied(size, &tile.rgba);
                    let name = format!("map-tile-{:?}-{}-{}-{}", k.style, k.z, k.x, k.y);
                    let tex = ctx.load_texture(name, img, egui::TextureOptions::LINEAR);
                    self.textures.insert(k, (tex, self.frame));
                    self.retry.remove(&k);
                }
                None => {
                    self.retry.insert(k, now + RETRY_SECS);
                }
            }
        }
    }

    /// Lets go of the tiles drawn longest ago when there are too many (never the ones of this frame).
    fn trim(&mut self) {
        if self.textures.len() <= MAX_TEXTURES {
            return;
        }
        let mut by_age: Vec<(u64, TileKey)> = self.textures.iter().map(|(k, (_, used))| (*used, *k)).collect();
        by_age.sort_by_key(|(used, _)| *used);
        let extra = self.textures.len() - MAX_TEXTURES;
        for (used, k) in by_age.into_iter().take(extra) {
            if used != self.frame {
                self.textures.remove(&k);
            }
        }
    }
}

/// One layer of tiles over `rect`: those at hand are painted (a coarser tile cropped to the right part stands in for one
/// that has not come), the missing ones are added to `want` with how far they are from the middle.
struct Layer {
    style: MapStyle,
    zoom: u8,
    /// Levels up to look for a stand-in.
    lookup: u32,
    tint: Color32,
    /// Added to the distance so nearer layers are asked for first.
    rank: f32,
}

fn paint_layer(rt: &mut Runtime, painter: &egui::Painter, rect: Rect, cam: Cam, now: f64, layer: &Layer, want: &mut Vec<(f32, TileKey)>) {
    let n = f64::from(1u32 << layer.zoom);
    let edge = (cam.s / n) as f32;
    let (x0, y0) = cam.world(rect.min);
    let (x1, y1) = cam.world(rect.max);
    let (tx0, tx1) = ((x0 * n).floor() as i64, (x1 * n).floor() as i64);
    let (ty0, ty1) = ((y0 * n).floor().max(0.0) as i64, (y1 * n).floor().min(n - 1.0) as i64);
    let frame = rt.frame;
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1.min(tx0 + MAX_ACROSS) {
            let Some(key) = TileKey::new(layer.style, layer.zoom, tx, ty) else { continue };
            let min = pos2(cam.c.x + ((tx as f64 / n - cam.cx) * cam.s) as f32, cam.c.y + ((ty as f64 / n - cam.cy) * cam.s) as f32);
            let r = Rect::from_min_size(min, vec2(edge, edge)).expand(0.25);
            if let Some((tex, used)) = rt.textures.get_mut(&key) {
                *used = frame;
                painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), layer.tint);
                continue;
            }
            let (mut k, mut ox, mut oy, mut size) = (key, 0.0f32, 0.0f32, 1.0f32);
            for _ in 0..layer.lookup {
                let Some((parent, qx, qy)) = k.parent() else { break };
                (k, ox, oy, size) = (parent, (qx as f32 + ox) / 2.0, (qy as f32 + oy) / 2.0, size / 2.0);
                if let Some((tex, used)) = rt.textures.get_mut(&k) {
                    *used = frame;
                    painter.image(tex.id(), r, Rect::from_min_size(pos2(ox, oy), vec2(size, size)), layer.tint);
                    break;
                }
            }
            if rt.retry.get(&key).is_none_or(|&at| now >= at) {
                want.push((r.center().distance_sq(rect.center()) + layer.rank, key));
            }
        }
    }
}

/// The map's tiles over `rect`. `carto_key` is the CARTO key (empty: OpenStreetMap), `satellite` the satellite view (with
/// place names over it), `zoom` the view's, and `light` adds the light pollution atlas.
pub(crate) fn draw_tiles(
    rt: &mut Runtime,
    carto_key: &str,
    satellite: bool,
    zoom: f64,
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: Rect,
    cam: Cam,
    now: f64,
    light: bool,
) {
    // a new key (or none any more) is another source, so other tiles
    let key = carto_key.trim().to_string();
    if rt.tiles.is_some() && rt.tiles_key != key {
        rt.reset();
    }
    rt.tiles_key = key.clone();
    rt.tiles.get_or_insert_with(|| MapTiles::new(Some(key.as_str()).filter(|k| !k.is_empty())));
    rt.receive(ctx, now);

    let level = (zoom + 0.25).floor();
    let z = level.clamp(0.0, f64::from(map_tiles::MAX_ZOOM)) as u8;
    // (the plain map is always the light one)
    let base: &[MapStyle] = if satellite { &[MapStyle::Satellite, MapStyle::Labels] } else { &[MapStyle::Light] };
    let mut want: Vec<(f32, TileKey)> = Vec::new();
    for (i, &style) in base.iter().enumerate() {
        // the names come after the picture under them
        let layer = Layer { style, zoom: z, lookup: 6, tint: crate::theme::map_tint(), rank: i as f32 * 1e9 };
        paint_layer(rt, painter, rect, cam, now, &layer, &mut want);
    }
    // the light pollution atlas has its own numbering: 1024-pixel tiles, level 0 the whole world
    if light {
        let lz = (level - 2.0).clamp(0.0, f64::from(map_tiles::LIGHT_POLLUTION_MAX_Z)) as u8;
        let layer =
            Layer { style: MapStyle::LightPollution, zoom: lz, lookup: 4, tint: crate::theme::map_tint().gamma_multiply(LIGHT_OPACITY), rank: 2e9 };
        paint_layer(rt, painter, rect, cam, now, &layer, &mut want);
    }
    // farthest first: the service takes the last ones first
    want.sort_by(|a, b| b.0.total_cmp(&a.0));
    let keys: Vec<TileKey> = want.into_iter().map(|(_, k)| k).collect();
    if let Some(service) = &rt.tiles {
        service.want(&keys);
        if service.busy() || !keys.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
    rt.trim();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))
    }

    #[test]
    fn a_view_is_made_usable() {
        let v = MapView { lat: f64::NAN, lon: 1.0e9, zoom: -50.0 }.sane();
        assert!(v.lat.is_finite() && (-180.0..180.0).contains(&v.lon) && v.zoom == MIN_ZOOM);
        let v = MapView { lat: 95.0, lon: 190.0, zoom: 40.0 }.sane();
        assert_eq!((v.lat, v.lon, v.zoom), (85.0, -170.0, MAX_ZOOM));
    }

    #[test]
    fn the_screen_and_the_world_agree() {
        let v = MapView { lat: 43.3, lon: -2.0, zoom: 11.0 };
        let cam = Cam::new(v, rect());
        assert!((cam.screen(cam.cx, cam.cy) - rect().center()).length() < 0.01);
        let p = pos2(120.0, 410.0);
        let (wx, wy) = cam.world(p);
        assert!((cam.screen(wx, wy) - p).length() < 0.01);
    }

    #[test]
    fn the_world_repeats_to_the_nearest_copy() {
        let cam = Cam::new(MapView { lat: 0.0, lon: 179.0, zoom: 4.0 }, rect());
        // a point just past the date line is a little to the east of the centre, not a world to the west
        let (wx, wy) = map_tiles::project(0.0, -179.0);
        let p = cam.screen(wx, wy);
        assert!(p.x > cam.c.x && p.x - cam.c.x < 100.0, "{p:?}");
        assert!((p.y - cam.c.y).abs() < 0.1);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let v = MapView { lat: 40.0, lon: -3.0, zoom: 8.0 };
        let at = pos2(250.0, 150.0);
        let before = Cam::new(v, rect()).world(at);
        let z = zoom_about(v, rect(), at, 10.0);
        let after = Cam::new(z, rect()).world(at);
        assert!((before.0 - after.0).abs() < 1e-9 && (before.1 - after.1).abs() < 1e-9);
        assert_eq!(zoom_about(v, rect(), at, 99.0).zoom, MAX_ZOOM);
    }

    #[test]
    fn the_poles_are_not_passed() {
        let high = MapView { lat: 85.0, lon: 0.0, zoom: 6.0 };
        let c = clamp_view(high, rect());
        let cam = Cam::new(c, rect());
        assert!(cam.cy - f64::from(rect().height()) / 2.0 / cam.s >= -1e-9);
        // a zoomed-out world is centred
        let tall = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 2000.0));
        let far = clamp_view(MapView { lat: 60.0, lon: 0.0, zoom: MIN_ZOOM }, tall);
        assert!(far.lat.abs() < 1e-6);
    }
}
