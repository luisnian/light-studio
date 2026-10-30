//! Map tiles for the Map view: raster tiles (Web Mercator, `{z}/{x}/{y}`) fetched in the
//! background, kept in a disk cache under the settings folder and handed to the UI decoded
//! (RGBA8). The UI thread never waits on the network: it asks for the tiles on screen with
//! [`MapTiles::want`] and collects whatever has arrived with [`MapTiles::drain`].
//!
//! Two sources ([`Source`]): CARTO's basemaps on OpenStreetMap data (dark or light, to follow the
//! app's appearance, at double resolution for Retina screens), which since September 2026 need a
//! free key (`?key=`; without one every tile carries an "API KEY REQUIRED" watermark), and,
//! without a key, OpenStreetMap's own tiles, toned down here to match the app (greyed for light,
//! dark grey with darker water and light labels for dark). `LIGHT_STUDIO_MAP_TILES` replaces both with another
//! `https://…/{z}/{x}/{y}.png` template (or a loopback `http://127.0.0.1…` one for tests). Only
//! tile coordinates are sent; never anything about the photos.
//!
//! The satellite view ([`MapStyle::Satellite`], the default) is Esri's World Imagery, with
//! Esri's place names and borders ([`MapStyle::Labels`], transparent) drawn over it.
//!
//! Sources: Web Mercator tile scheme (OpenStreetMap wiki, "Slippy map tilenames"); own design.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;

use crate::services::pmtiles::Archive;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

/// Where the tiles come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// CARTO basemaps (with a key).
    Carto,
    /// OpenStreetMap's standard tiles, toned to the app's appearance.
    Osm,
    /// `LIGHT_STUDIO_MAP_TILES`.
    Custom,
    /// A PMTiles file on this computer (its own credit is kept with it).
    Offline,
}

impl Source {
    /// The credit the map must show (empty for a custom source).
    pub fn attribution(self) -> &'static str {
        match self {
            Source::Carto => "© OpenStreetMap · © CARTO",
            Source::Osm => "© OpenStreetMap",
            Source::Custom | Source::Offline => "",
        }
    }
}
/// Deepest zoom level the default tiles have.
pub const MAX_ZOOM: u8 = 19;
/// Tiles fetched at once.
const WORKERS: usize = 4;
/// Largest tile accepted (bytes).
const MAX_TILE_BYTES: usize = 4 * 1024 * 1024;
/// Largest decoded tile edge accepted (pixels).
const MAX_TILE_EDGE: u32 = 1024;
/// A cached tile older than this is fetched again (when the network allows; else it's kept).
const CACHE_DAYS: u64 = 45;

/// Light or dark map, to match the app; or the satellite view and the place names over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MapStyle {
    Dark,
    Light,
    Satellite,
    Labels,
    /// David Lorenz's light pollution atlas (transparent, drawn over the map). Its tiles are 1024 pixels wide and
    /// the zoom counts from 0 for a single tile of the whole world (two levels under the usual numbering).
    LightPollution,
}

impl MapStyle {
    fn slug(self) -> &'static str {
        match self {
            MapStyle::Dark => "dark_all",
            MapStyle::Light => "light_all",
            MapStyle::Satellite => "satellite",
            MapStyle::Labels => "labels",
            MapStyle::LightPollution => "lightpollution",
        }
    }
}

/// Esri World Imagery (satellite and aerial photos) and its reference layer (place names, borders).
const ESRI_IMAGERY: &str = "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}";
const ESRI_LABELS: &str = "https://server.arcgisonline.com/ArcGIS/rest/services/Reference/World_Boundaries_and_Places/MapServer/tile/{z}/{y}/{x}";
/// The light pollution atlas (VIIRS night lights, 2025) by David J. Lorenz, as his own map site shows it: the tile at
/// zoom `{z}` (0–6) is 1024 pixels and covers `1/2^z` of the world.
const LORENZ_ATLAS: &str = "https://djlorenz.github.io/astronomy/image_tiles/tiles2025/tile_{z}_{x}_{y}.png";
/// The atlas's deepest tile level (the tiles are enlarged beyond it), and its credit.
pub const LIGHT_POLLUTION_MAX_Z: u8 = 6;
const _: () = assert!(LIGHT_POLLUTION_MAX_Z < MAX_ZOOM);
pub const LIGHT_POLLUTION_ATTRIBUTION: &str = "Light pollution: D. J. Lorenz (VIIRS 2025)";
/// The satellite view's credit.
pub const SATELLITE_ATTRIBUTION: &str = "© Esri · Maxar · Earthstar Geographics";

/// One tile: style, zoom, column, row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub style: MapStyle,
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl TileKey {
    /// `None` outside the world at zoom `z` (column wrapped around the antimeridian first).
    pub fn new(style: MapStyle, z: u8, x: i64, y: i64) -> Option<TileKey> {
        let n = 1i64 << z.min(MAX_ZOOM);
        if y < 0 || y >= n {
            return None;
        }
        let x = x.rem_euclid(n);
        Some(TileKey { style, z: z.min(MAX_ZOOM), x: u32::try_from(x).ok()?, y: u32::try_from(y).ok()? })
    }

    /// The tile one level up that holds this one, and which quarter of it (0..2, 0..2).
    pub fn parent(self) -> Option<(TileKey, u32, u32)> {
        (self.z > 0).then(|| (TileKey { style: self.style, z: self.z - 1, x: self.x / 2, y: self.y / 2 }, self.x % 2, self.y % 2))
    }
}

/// A decoded tile.
#[derive(Clone, Debug)]
pub struct Tile {
    pub width: u32,
    pub height: u32,
    /// RGBA, 8 bits per channel, rows top to bottom.
    pub rgba: Vec<u8>,
}

#[derive(Default)]
struct Inner {
    /// Wanted and not started, most recently wanted last (taken from the back).
    queue: VecDeque<TileKey>,
    queued: HashSet<TileKey>,
    running: HashSet<TileKey>,
    /// Finished since the last drain: the tile, or `None` when it could not be had.
    done: Vec<(TileKey, Option<Tile>)>,
    workers: usize,
    /// Set when the owner goes away: the workers stop.
    closed: bool,
    /// Tiles that failed (not asked for again this session).
    failed: HashSet<TileKey>,
    /// Network failures in a row; above a few the network is assumed down and only the disk cache is read.
    offline_streak: u32,
}

struct Shared {
    /// The offline map file, when one is set: it serves the picture styles.
    archive: Option<Arc<Archive>>,
    inner: Mutex<Inner>,
    wake: Condvar,
    template: String,
    source: Source,
    cache: Option<PathBuf>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The offline map file in use: its path and the archive, or why it could not be opened.
static OFFLINE: Mutex<Option<(String, Result<Arc<Archive>, String>)>> = Mutex::new(None);

/// Sets the offline map file (empty: none). The file is opened here; the result is what [`offline_status`] answers.
pub fn set_offline(path: &str) {
    let path = path.trim();
    let mut g = OFFLINE.lock().unwrap_or_else(PoisonError::into_inner);
    if g.as_ref().is_some_and(|(p, _)| p == path) || (path.is_empty() && g.is_none()) {
        return;
    }
    *g = (!path.is_empty()).then(|| (path.to_string(), Archive::open(std::path::Path::new(path)).map(Arc::new)));
}

/// The path of the offline map file in use (empty: none).
pub fn offline_path() -> String {
    OFFLINE.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(|(p, _)| p.clone()).unwrap_or_default()
}

/// `None` without an offline file; else a description of it, or why it cannot be used.
pub fn offline_status() -> Option<Result<String, String>> {
    OFFLINE.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(|(_, r)| r.as_ref().map(|a| a.describe()).map_err(Clone::clone))
}

fn offline_archive() -> Option<Arc<Archive>> {
    OFFLINE.lock().unwrap_or_else(PoisonError::into_inner).as_ref().and_then(|(_, r)| r.as_ref().ok().cloned())
}

/// The tile service of one window.
pub struct MapTiles {
    shared: Arc<Shared>,
}

impl Drop for MapTiles {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.wake.notify_all();
    }
}

impl Default for MapTiles {
    fn default() -> Self {
        Self::new(None)
    }
}

/// The URL template in use: `LIGHT_STUDIO_MAP_TILES`, else CARTO with `carto_key`, else OpenStreetMap.
fn template(carto_key: Option<&str>) -> (String, Source) {
    if let Some(t) = std::env::var("LIGHT_STUDIO_MAP_TILES").ok().filter(|t| allowed(t)) {
        return (t, Source::Custom);
    }
    match carto_key.map(str::trim).filter(|k| valid_key(k)) {
        Some(k) => (format!("https://basemaps.cartocdn.com/{{style}}/{{z}}/{{x}}/{{y}}@2x.png?key={k}"), Source::Carto),
        None => ("https://tile.openstreetmap.org/{z}/{x}/{y}.png".to_string(), Source::Osm),
    }
}

/// A CARTO key as it can go in a URL: letters, digits, `-` and `_` (what CARTO hands out), 8–200 long.
pub fn valid_key(k: &str) -> bool {
    (8..=200).contains(&k.len()) && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A template: https, or plain http to this machine's loopback address only (a test server).
fn allowed(t: &str) -> bool {
    t.starts_with("https://") || t.strip_prefix("http://127.0.0.1").is_some_and(|r| r.starts_with(':') || r.starts_with('/'))
}

/// The tile's address from a template.
pub fn url(template: &str, k: TileKey) -> String {
    template.replace("{style}", k.style.slug()).replace("{z}", &k.z.to_string()).replace("{x}", &k.x.to_string()).replace("{y}", &k.y.to_string())
}

impl MapTiles {
    /// CARTO when there's a key (else OpenStreetMap; `LIGHT_STUDIO_MAP_TILES` overrides both), with
    /// the cache in the settings folder.
    pub fn new(carto_key: Option<&str>) -> MapTiles {
        if let Some(archive) = offline_archive() {
            let mut m = MapTiles::with(String::new(), Source::Offline, None);
            if let Some(shared) = Arc::get_mut(&mut m.shared) {
                shared.archive = Some(archive);
            }
            return m;
        }
        let (template, source) = template(carto_key);
        // one cache per source: a different template never shows another's tiles
        let cache = crate::paths::config_dir().map(|d| d.join("map-tiles").join(format!("{:016x}", fnv(&template))));
        MapTiles::with(template, source, cache)
    }

    pub fn with(template: String, source: Source, cache: Option<PathBuf>) -> MapTiles {
        MapTiles { shared: Arc::new(Shared { archive: None, inner: Mutex::new(Inner::default()), wake: Condvar::new(), template, source, cache }) }
    }

    /// The credit the picture tiles must show: the offline file's own, else the source's.
    pub fn attribution(&self) -> String {
        match &self.shared.archive {
            Some(a) => a.attribution.clone(),
            None => self.shared.source.attribution().to_string(),
        }
    }

    /// Where the tiles come from.
    pub fn source(&self) -> Source {
        self.shared.source
    }

    /// Ask for `keys` (the tiles on screen, most important last). Tiles queued earlier that are
    /// no longer wanted are dropped; ones already being fetched finish.
    pub fn want(&self, keys: &[TileKey]) {
        let mut g = self.shared.lock();
        let wanted: HashSet<TileKey> = keys.iter().copied().collect();
        g.queue.retain(|k| wanted.contains(k));
        let inner = &mut *g;
        inner.queued.retain(|k| wanted.contains(k));
        for &k in keys {
            if !inner.queued.contains(&k) && !inner.running.contains(&k) && !inner.failed.contains(&k) {
                inner.queued.insert(k);
                inner.queue.push_back(k);
            }
        }
        let spawn = g.queue.len().min(WORKERS.saturating_sub(g.workers));
        g.workers += spawn;
        drop(g);
        for i in 0..spawn {
            let shared = self.shared.clone();
            let ok = std::thread::Builder::new().name(format!("map-tiles-{i}")).spawn(move || worker(&shared)).is_ok();
            if !ok {
                self.shared.lock().workers -= 1;
            }
        }
        self.shared.wake.notify_all();
    }

    /// Tiles that arrived (or failed) since the last call.
    pub fn drain(&self) -> Vec<(TileKey, Option<Tile>)> {
        std::mem::take(&mut self.shared.lock().done)
    }

    /// Whether anything is queued or being fetched.
    pub fn busy(&self) -> bool {
        let g = self.shared.lock();
        !g.queue.is_empty() || !g.running.is_empty() || !g.done.is_empty()
    }
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

fn worker(shared: &Shared) {
    loop {
        let key = {
            let mut g = shared.lock();
            loop {
                if g.closed {
                    g.workers = g.workers.saturating_sub(1);
                    return;
                }
                if let Some(k) = g.queue.pop_back() {
                    g.queued.remove(&k);
                    g.running.insert(k);
                    break k;
                }
                let (next, timeout) = shared.wake.wait_timeout(g, std::time::Duration::from_secs(30)).unwrap_or_else(PoisonError::into_inner);
                g = next;
                // idle for a while: the thread ends (a later `want` starts another)
                if timeout.timed_out() && g.queue.is_empty() {
                    g.workers = g.workers.saturating_sub(1);
                    return;
                }
            }
        };
        let offline = shared.lock().offline_streak >= 6;
        let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load(shared, key, offline))).unwrap_or(Err(false));
        let mut g = shared.lock();
        g.running.remove(&key);
        match got {
            Ok(tile) => {
                g.offline_streak = 0;
                g.done.push((key, Some(tile)));
            }
            Err(network) => {
                if network {
                    g.offline_streak = g.offline_streak.saturating_add(1);
                } else {
                    g.failed.insert(key);
                }
                g.done.push((key, None));
            }
        }
    }
}

/// The template a tile is fetched with: the satellite view and its names have their own (a custom
/// template serves every style).
fn template_for(shared: &Shared, k: TileKey) -> &str {
    match (shared.source, k.style) {
        (Source::Custom, _) => &shared.template,
        (_, MapStyle::Satellite) => ESRI_IMAGERY,
        (_, MapStyle::Labels) => ESRI_LABELS,
        (_, MapStyle::LightPollution) => LORENZ_ATLAS,
        _ => &shared.template,
    }
}

fn cache_path(shared: &Shared, k: TileKey) -> Option<PathBuf> {
    // OpenStreetMap has one picture for both styles (toned when decoded)
    let dir = if shared.source == Source::Osm && matches!(k.style, MapStyle::Dark | MapStyle::Light) { "base" } else { k.style.slug() };
    shared.cache.as_ref().map(|d| d.join(dir).join(k.z.to_string()).join(k.x.to_string()).join(format!("{}.png", k.y)))
}

/// OpenStreetMap's colourful tiles toned to sit quietly behind the photos: mostly grey and a little
/// lighter for the light style; for the dark one, land and roads dark grey, water darker, labels
/// light, colour almost gone.
pub fn tone(t: &mut Tile, style: MapStyle) {
    for px in t.rgba.chunks_exact_mut(4) {
        let [r, g, b] = [f32::from(px[0]), f32::from(px[1]), f32::from(px[2])];
        let l = 0.299 * r + 0.587 * g + 0.114 * b;
        let out = match style {
            MapStyle::Light => {
                // 85 % grey, lifted towards white
                [r, g, b].map(|c| (l + (c - l) * 0.15) * 0.8 + 255.0 * 0.2)
            }
            MapStyle::Dark | MapStyle::Satellite | MapStyle::Labels | MapStyle::LightPollution => {
                // water (OSM's pale blue) darkest; land and roads (light) a dark grey, the lighter
                // the higher; labels and borders (dark) turned light so they stay legible
                let water = b - r > 30.0 && g > r;
                // major roads are drawn warm (orange, yellow, pink); minor ones white
                let main_road = r - b > 45.0 && r > 200.0;
                let v = if water {
                    22.0
                } else if l < 150.0 {
                    50.0 + (150.0 - l) / 150.0 * 130.0
                } else if main_road {
                    78.0
                } else if l > 250.0 {
                    66.0
                } else {
                    30.0 + (l - 150.0) / 105.0 * 20.0
                };
                [r, g, b].map(|c| (v + (c - l) * 0.06).max(0.0))
            }
        };
        for (d, v) in px.iter_mut().zip(out) {
            *d = v.round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// The tile from the cache, else the network (then cached). `Err(true)`: a network failure (may
/// work later); `Err(false)`: the tile itself is bad.
fn load(shared: &Shared, k: TileKey, offline: bool) -> Result<Tile, bool> {
    // the offline file serves the pictures; it has no names layer of its own, and the light atlas still comes by the network
    if let Some(a) = &shared.archive
        && k.style != MapStyle::LightPollution
    {
        if k.style == MapStyle::Labels {
            return Err(false);
        }
        return match a.tile(k.z, k.x, k.y) {
            Ok(Some(bytes)) => decode(&bytes).ok_or(false),
            _ => Err(false),
        };
    }
    let path = cache_path(shared, k);
    let cached = path.as_ref().and_then(|p| {
        let meta = std::fs::metadata(p).ok()?;
        let fresh = meta.modified().ok().and_then(|m| m.elapsed().ok()).is_none_or(|age| age.as_secs() < CACHE_DAYS * 86_400);
        let bytes = std::fs::read(p).ok()?;
        Some((bytes, fresh))
    });
    let decode = |b: &[u8]| {
        let mut t = decode(b)?;
        if shared.source == Source::Osm && matches!(k.style, MapStyle::Dark | MapStyle::Light) {
            tone(&mut t, k.style);
        }
        Some(t)
    };
    if let Some((bytes, fresh)) = &cached
        && (*fresh || offline)
        && let Some(t) = decode(bytes)
    {
        return Ok(t);
    }
    if offline {
        return Err(true);
    }
    match fetch(&url(template_for(shared, k), k)) {
        Ok(bytes) => {
            let tile = decode(&bytes).ok_or(false)?;
            if let Some(p) = &path
                && let Some(dir) = p.parent()
                && std::fs::create_dir_all(dir).is_ok()
            {
                let part = p.with_extension("part");
                if std::fs::write(&part, &bytes).is_ok() {
                    let _ = std::fs::rename(&part, p);
                }
            }
            Ok(tile)
        }
        Err(network) => match cached.and_then(|(b, _)| decode(&b)) {
            // stale but better than nothing
            Some(t) => Ok(t),
            None => Err(network),
        },
    }
}

/// A tile's bytes: `Err(false)` when the server has none, `Err(true)` when it cannot be reached.
pub(crate) fn fetch_tile(address: &str) -> Result<Vec<u8>, bool> {
    fetch(address)
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch(address: &str) -> Result<Vec<u8>, bool> {
    use crate::net;
    let r =
        net::get(address, &[("Accept", "image/png,image/jpeg".to_string())], net::Timeouts::secs(8, 15), MAX_TILE_BYTES).map_err(|e| match e {
            // a tile too large, or an address that cannot be used, will not get better by asking again
            net::NetError::TooBig | net::NetError::BadUrl => false,
            net::NetError::Unreachable(_) => true,
        })?;
    match r.status {
        200 => Ok(r.body),
        // the server is there but has no such tile
        400..=499 => Err(false),
        _ => Err(true),
    }
}

#[cfg(target_arch = "wasm32")]
fn fetch(_: &str) -> Result<Vec<u8>, bool> {
    Err(true)
}

/// A PNG (or JPEG) tile as RGBA8.
pub fn decode(bytes: &[u8]) -> Option<Tile> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || w > MAX_TILE_EDGE || h > MAX_TILE_EDGE {
        return None;
    }
    Some(Tile { width: w, height: h, rgba: img.into_raw() })
}

// ------------------------------------------------------------------------- Web Mercator

/// Longitude, latitude (degrees) → world position in 0..1 (x east, y south).
pub fn project(lat: f64, lon: f64) -> (f64, f64) {
    let lat = lat.clamp(-85.051_128_78, 85.051_128_78).to_radians();
    let x = (lon + 180.0) / 360.0;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0;
    (x, y)
}

/// World position (0..1) → latitude, longitude (degrees).
pub fn unproject(x: f64, y: f64) -> (f64, f64) {
    let lon = x * 360.0 - 180.0;
    let n = std::f64::consts::PI * (1.0 - 2.0 * y);
    let lat = n.sinh().atan().to_degrees();
    (lat, lon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_pollution_atlas_is_asked_for_by_its_own_tile_numbers() {
        let k = TileKey::new(MapStyle::LightPollution, 4, 8, 5).unwrap();
        assert_eq!(url(LORENZ_ATLAS, k), "https://djlorenz.github.io/astronomy/image_tiles/tiles2025/tile_4_8_5.png");
        assert_eq!(url("http://127.0.0.1:1/{style}/{z}/{x}/{y}.png", k), "http://127.0.0.1:1/lightpollution/4/8/5.png");
        // beyond the atlas's deepest level the same tile is enlarged, so nothing deeper is asked for by the UI
    }

    #[test]
    fn projection_round_trips() {
        for (lat, lon) in [(43.3183, -1.9812), (0.0, 0.0), (-33.86, 151.2), (64.1, -21.9)] {
            let (x, y) = project(lat, lon);
            let (la, lo) = unproject(x, y);
            assert!((la - lat).abs() < 1e-9 && (lo - lon).abs() < 1e-9, "{lat},{lon} → {la},{lo}");
        }
        assert_eq!(project(0.0, 0.0), (0.5, 0.5));
    }

    #[test]
    fn tile_keys_wrap_columns_and_drop_rows_outside() {
        assert_eq!(TileKey::new(MapStyle::Dark, 2, -1, 0).map(|k| k.x), Some(3));
        assert_eq!(TileKey::new(MapStyle::Dark, 2, 4, 1).map(|k| k.x), Some(0));
        assert!(TileKey::new(MapStyle::Dark, 2, 0, 4).is_none());
        assert!(TileKey::new(MapStyle::Dark, 2, 0, -1).is_none());
        let k = TileKey::new(MapStyle::Light, 5, 15, 11).unwrap();
        assert_eq!(k.parent().map(|(p, qx, qy)| (p.z, p.x, p.y, qx, qy)), Some((4, 7, 5, 1, 1)));
    }

    #[test]
    fn templates_are_https_or_loopback() {
        assert!(allowed("https://example.org/{z}/{x}/{y}.png"));
        assert!(allowed("http://127.0.0.1:8080/{z}/{x}/{y}.png"));
        assert!(!allowed("http://127.0.0.1.example.com/{z}.png"));
        assert!(!allowed("http://example.org/{z}.png"));
        let k = TileKey::new(MapStyle::Dark, 3, 4, 2).unwrap();
        assert_eq!(url("https://h/{style}/{z}/{x}/{y}.png", k), "https://h/dark_all/3/4/2.png");
    }

    #[test]
    fn tiles_come_from_the_cache_and_bad_bytes_fail_without_a_panic() {
        let dir = std::env::temp_dir().join(format!("ls-map-tiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let k = TileKey::new(MapStyle::Dark, 1, 0, 0).unwrap();
        // a 2×2 PNG in the cache
        let mut png = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png, 2, 2);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[10, 20, 30, 10, 20, 30, 10, 20, 30, 10, 20, 30]).unwrap();
        }
        let p = dir.join("dark_all/1/0/0.png");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, &png).unwrap();
        // a garbage tile next to it
        let bad = TileKey::new(MapStyle::Dark, 1, 1, 0).unwrap();
        let pb = dir.join("dark_all/1/1/0.png");
        std::fs::create_dir_all(pb.parent().unwrap()).unwrap();
        std::fs::write(&pb, b"not a png").unwrap();
        // nothing listens here: the network part fails fast
        let tiles = MapTiles::with("http://127.0.0.1:9/{style}/{z}/{x}/{y}.png".into(), Source::Custom, Some(dir.clone()));
        tiles.want(&[k, bad]);
        let mut got = Vec::new();
        for _ in 0..400 {
            got.extend(tiles.drain());
            if got.len() == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let ok = got.iter().find(|(key, _)| *key == k).and_then(|(_, t)| t.clone()).expect("cached tile");
        assert_eq!((ok.width, ok.height, &ok.rgba[..4]), (2, 2, &[10, 20, 30, 255][..]));
        assert!(got.iter().any(|(key, t)| *key == bad && t.is_none()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_carto_key_picks_carto_and_none_openstreetmap() {
        if std::env::var_os("LIGHT_STUDIO_MAP_TILES").is_some() {
            return;
        }
        let (t, s) = template(Some(" abcDEF12-_x "));
        assert_eq!(s, Source::Carto);
        assert!(t.starts_with("https://basemaps.cartocdn.com/") && t.ends_with("@2x.png?key=abcDEF12-_x"), "{t}");
        assert_eq!(url(&t, TileKey::new(MapStyle::Dark, 3, 4, 2).unwrap()), "https://basemaps.cartocdn.com/dark_all/3/4/2@2x.png?key=abcDEF12-_x");
        for bad in [None, Some(""), Some("short"), Some("has space inside"), Some("a&b=c?d#eeee")] {
            let (t, s) = template(bad);
            assert_eq!(s, Source::Osm, "{bad:?}");
            assert_eq!(t, "https://tile.openstreetmap.org/{z}/{x}/{y}.png");
        }
        assert_eq!(Source::Osm.attribution(), "© OpenStreetMap");
    }

    #[test]
    fn the_satellite_view_has_its_own_tiles_and_names_whatever_the_source() {
        let shared = |source| Shared {
            archive: None,
            inner: Mutex::new(Inner::default()),
            wake: Condvar::new(),
            template: "https://h/{style}/{z}/{x}/{y}.png".into(),
            source,
            cache: Some(PathBuf::from("/c")),
        };
        let k = TileKey::new(MapStyle::Satellite, 5, 15, 11).unwrap();
        let l = TileKey { style: MapStyle::Labels, ..k };
        for source in [Source::Osm, Source::Carto] {
            let sh = shared(source);
            assert_eq!(url(template_for(&sh, k), k), "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/5/11/15");
            assert!(url(template_for(&sh, l), l).contains("World_Boundaries_and_Places/MapServer/tile/5/11/15"));
            assert_eq!(cache_path(&sh, k), Some(PathBuf::from("/c/satellite/5/15/11.png")));
        }
        // a test server serves everything
        assert_eq!(url(template_for(&shared(Source::Custom), k), k), "https://h/satellite/5/15/11.png");
    }

    #[test]
    fn openstreetmap_tiles_are_toned_to_the_app() {
        // OSM land (near white), water (blue), a road (orange)
        let land = [242u8, 239, 233, 255];
        let water = [170u8, 211, 223, 255];
        let road = [252u8, 214, 164, 255];
        let tile = || Tile { width: 3, height: 1, rgba: [land, water, road].concat() };
        let mut dark = tile();
        tone(&mut dark, MapStyle::Dark);
        let px = |t: &Tile, i: usize| [t.rgba[i * 4], t.rgba[i * 4 + 1], t.rgba[i * 4 + 2]];
        let lum = |p: [u8; 3]| (u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) / 3;
        // dark land, darker water, roads lighter than land
        assert!(lum(px(&dark, 0)) < 50, "{:?}", px(&dark, 0));
        assert!(lum(px(&dark, 1)) < lum(px(&dark, 0)), "water darker than land");
        assert!(lum(px(&dark, 2)) > lum(px(&dark, 0)) + 20, "roads stand out");
        // nearly grey
        let spread = |p: [u8; 3]| p.iter().max().unwrap() - p.iter().min().unwrap();
        assert!(spread(px(&dark, 2)) < 12 && spread(px(&dark, 1)) < 12);
        // a black label turns light
        let mut label = Tile { width: 1, height: 1, rgba: vec![20, 20, 20, 255] };
        tone(&mut label, MapStyle::Dark);
        assert!(label.rgba[0] > 150, "{:?}", label.rgba);
        let mut light = tile();
        tone(&mut light, MapStyle::Light);
        assert!(lum(px(&light, 0)) > 220 && spread(px(&light, 2)) < 20, "{:?}", px(&light, 2));
        assert_eq!(light.rgba[3], 255, "alpha kept");
    }

    #[test]
    fn an_offline_file_serves_the_picture_styles_and_nothing_else() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let bytes = crate::services::pmtiles::tests::build(&[(3, 4, 2, png)], false, false, 2);
        let dir = std::env::temp_dir().join(format!("light-studio-offline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("region.pmtiles");
        std::fs::write(&file, bytes).unwrap();
        let archive = Archive::open(&file).unwrap();
        let shared = Shared {
            archive: Some(Arc::new(archive)),
            inner: Mutex::new(Inner::default()),
            wake: Condvar::new(),
            template: String::new(),
            source: Source::Offline,
            cache: None,
        };
        let have = TileKey::new(MapStyle::Light, 3, 4, 2).unwrap();
        let t = load(&shared, have, false).unwrap();
        assert_eq!((t.width, t.height, t.rgba[0]), (8, 8, 200));
        assert!(load(&shared, TileKey { style: MapStyle::Satellite, ..have }, false).is_ok(), "the satellite view uses the same file");
        assert_eq!(load(&shared, TileKey { style: MapStyle::Labels, ..have }, false).err(), Some(false));
        assert_eq!(load(&shared, TileKey::new(MapStyle::Light, 3, 5, 2).unwrap(), false).err(), Some(false), "a tile the file lacks");
        // the status helpers
        set_offline(&file.display().to_string());
        assert!(matches!(offline_status(), Some(Ok(_))));
        set_offline("/nonexistent/x.pmtiles");
        assert!(matches!(offline_status(), Some(Err(_))));
        set_offline("");
        assert!(offline_status().is_none());
    }
}
