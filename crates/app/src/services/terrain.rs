//! Ground heights read on this computer from terrain tiles, for the Mountains view: the AWS Terrain Tiles
//! («Terrarium» PNGs, free and keyless; global, about 75 m between samples at zoom 11), a handful of tiles instead of
//! tens of thousands of web requests. A skyline sampled every quarter of a degree and 120 rings deep is 170 000 heights,
//! which only works when the heights are read locally; the tiles are kept on disk, so looking at a place again (or
//! another one nearby) asks little or nothing.
//!
//! A pixel holds `R·256 + G + B/256 − 32768` metres. Heights below the sea (bathymetry) are read as sea level.
//! `LIGHT_STUDIO_TERRAIN_URL` points the tiles at another server (`{z}`, `{x}` and `{y}` in the address; tests).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The tiles' address.
pub const SERVICE: &str = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png";
/// The zoom asked for first, and the most tiles one lookup may need (the zoom drops until it fits).
const BEST_ZOOM: u32 = 11;
const MIN_ZOOM: u32 = 8;
pub const MAX_TILES: usize = 120;
/// Most points one lookup may ask for.
pub const MAX_POINTS: usize = 400_000;
/// Tiles kept on disk.
const MAX_CACHED: usize = 1500;
const MAX_TILE_BYTES: usize = 1024 * 1024;
const TILE: usize = 256;

/// One decoded tile: `TILE × TILE` heights.
struct Tile {
    heights: Vec<f32>,
}

/// A tile's column and row at `zoom`, and the pixel position `(x, y)` of a point on the whole world's grid at that zoom.
fn world_pixel(lat: f64, lon: f64, zoom: u32) -> (f64, f64) {
    let n = f64::from(1u32 << zoom) * TILE as f64;
    let lat = lat.clamp(-85.05, 85.05).to_radians();
    let x = (lon + 180.0) / 360.0 * n;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// The tiles (column, row) that cover every point with a pixel to spare for the bilinear reads.
fn tiles_for(points: &[(f64, f64)], zoom: u32) -> HashSet<(u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (la, lo) in points {
        let (x, y) = world_pixel(*la, *lo, zoom);
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    let count = 1u32 << zoom;
    let tile = |v: f64| ((v / TILE as f64).floor().max(0.0) as u32).min(count - 1);
    let mut out = HashSet::new();
    // (a lookup across the antimeridian wraps: its columns are taken modulo the world's width)
    let (cx0, cx1, cy0, cy1) = (tile(x0 - 2.0), tile(x1 + 2.0), tile(y0 - 2.0), tile(y1 + 2.0));
    for cy in cy0..=cy1 {
        for cx in cx0..=cx1 {
            out.insert((cx, cy));
        }
    }
    out
}

/// The zoom to read `points` at, and the tiles it needs.
fn plan(points: &[(f64, f64)]) -> (u32, HashSet<(u32, u32)>) {
    let mut zoom = BEST_ZOOM;
    loop {
        let tiles = tiles_for(points, zoom);
        if tiles.len() <= MAX_TILES || zoom <= MIN_ZOOM {
            return (zoom, tiles);
        }
        zoom -= 1;
    }
}

fn tile_address(base: &str, zoom: u32, x: u32, y: u32) -> String {
    base.replace("{z}", &zoom.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string())
}

/// Reads a Terrarium PNG into heights (metres).
pub fn decode_tile(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(|e| format!("terrain tile: {e}"))?;
    if img.width() as usize != TILE || img.height() as usize != TILE {
        return Err("terrain tile: not 256 × 256".into());
    }
    Ok(img
        .to_rgb8()
        .pixels()
        .map(|p| {
            let h = f32::from(p[0]) * 256.0 + f32::from(p[1]) + f32::from(p[2]) / 256.0 - 32768.0;
            h.clamp(0.0, 9_000.0)
        })
        .collect())
}

/// (the same folder and layout as the shadow map's tiles, so the two share what is kept)
fn cache_file(dir: &Path, z: u32, x: u32, y: u32) -> PathBuf {
    dir.join(z.to_string()).join(x.to_string()).join(format!("{y}.png"))
}

/// Every kept tile: its age and path.
fn kept(dir: &Path, out: &mut Vec<(std::time::SystemTime, PathBuf)>, depth: usize) {
    if depth > 4 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        match e.metadata() {
            Ok(m) if m.is_dir() => kept(&p, out, depth + 1),
            Ok(m) if p.extension().is_some_and(|x| x == "png") => {
                if let Ok(t) = m.modified() {
                    out.push((t, p));
                }
            }
            _ => {}
        }
    }
}

/// A tile: from the disk, else from the network (and kept).
fn load_tile(dir: Option<&Path>, base: &str, z: u32, x: u32, y: u32) -> Result<Tile, String> {
    if let Some(d) = dir
        && let Ok(bytes) = std::fs::read(cache_file(d, z, x, y))
        && let Ok(h) = decode_tile(&bytes)
    {
        return Ok(Tile { heights: h });
    }
    let bytes = get(&tile_address(base, z, x, y))?;
    let heights = decode_tile(&bytes)?;
    if let Some(d) = dir {
        keep(d, &cache_file(d, z, x, y), &bytes);
    }
    Ok(Tile { heights })
}

/// Keeps a tile (best effort), and now and then the folder to the newest few hundred.
fn keep(dir: &Path, file: &Path, bytes: &[u8]) {
    static WRITES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if let Some(parent) = file.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let tmp = file.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, file).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    if WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 25 != 24 {
        return;
    }
    let mut files = Vec::new();
    kept(dir, &mut files, 0);
    files.sort();
    let extra = files.len().saturating_sub(MAX_CACHED);
    for (_, old) in files.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn get(address: &str) -> Result<Vec<u8>, String> {
    use crate::net;
    let r = net::get(address, &[("Accept", "image/png".to_string())], net::Timeouts::secs(8, 20), MAX_TILE_BYTES).map_err(|e| match e {
        net::NetError::TooBig => "the terrain service answered too much".to_string(),
        net::NetError::BadUrl => "the terrain service address is not valid".to_string(),
        net::NetError::Unreachable(_) => "could not reach the terrain service (is the computer online?)".to_string(),
    })?;
    if r.status != 200 {
        return Err(format!("the terrain service answered {}", r.status));
    }
    Ok(r.body)
}

#[cfg(target_arch = "wasm32")]
fn get(_: &str) -> Result<Vec<u8>, String> {
    Err("the mountains need the desktop app".into())
}

/// The tiles of a lookup, a few at a time.
fn load_all(dir: Option<&Path>, base: &str, zoom: u32, needed: &HashSet<(u32, u32)>) -> Result<HashMap<(u32, u32), Tile>, String> {
    let list: Vec<(u32, u32)> = needed.iter().copied().collect();
    let workers = 6.min(list.len().max(1));
    let results = std::sync::Mutex::new(HashMap::new());
    let failure = std::sync::Mutex::new(None::<String>);
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&(x, y)) = list.get(i) else { break };
                    if failure.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_some() {
                        break;
                    }
                    match load_tile(dir, base, zoom, x, y) {
                        Ok(t) => {
                            results.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert((x, y), t);
                        }
                        Err(e) => {
                            failure.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert(e);
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = failure.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner) {
        return Err(e);
    }
    Ok(results.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// The ground height (metres above the sea) at each point, read from the terrain tiles (the network the first time).
pub fn heights(points: &[(f64, f64)]) -> Result<Vec<f64>, String> {
    let dir = crate::paths::config_dir().map(|d| d.join("terrain-tiles"));
    let base =
        std::env::var("LIGHT_STUDIO_TERRAIN_URL").ok().map(|u| u.trim().to_string()).filter(|u| !u.is_empty()).unwrap_or_else(|| SERVICE.to_string());
    heights_with(dir.as_deref(), &base, points)
}

fn heights_with(dir: Option<&Path>, base: &str, points: &[(f64, f64)]) -> Result<Vec<f64>, String> {
    if points.is_empty() || points.len() > MAX_POINTS {
        return Err(format!("ask for 1 to {MAX_POINTS} points"));
    }
    if points.iter().any(|(la, lo)| !(la.is_finite() && lo.is_finite() && la.abs() <= 90.0 && lo.abs() <= 180.0)) {
        return Err("a point is not a place on Earth".into());
    }
    let (zoom, needed) = plan(points);
    if needed.len() > MAX_TILES {
        return Err("that is too wide an area for the terrain tiles".into());
    }
    let tiles = load_all(dir, base, zoom, &needed)?;
    let count = 1u32 << zoom;
    let world = f64::from(count) * TILE as f64;
    // a pixel of the world grid (the column wraps round the world, the row stops at its edges)
    let pixel = |px: i64, py: i64| -> f32 {
        let w = world as i64;
        let px = px.rem_euclid(w);
        let py = py.clamp(0, w - 1);
        let (tx, ty) = ((px / TILE as i64) as u32, (py / TILE as i64) as u32);
        let (ix, iy) = ((px % TILE as i64) as usize, (py % TILE as i64) as usize);
        tiles.get(&(tx, ty)).and_then(|t| t.heights.get(iy * TILE + ix)).copied().unwrap_or(0.0)
    };
    Ok(points
        .iter()
        .map(|(la, lo)| {
            let (x, y) = world_pixel(*la, *lo, zoom);
            // (pixel centres are at the half)
            let (fx, fy) = (x - 0.5, y - 0.5);
            let (x0, y0) = (fx.floor(), fy.floor());
            let (u, v) = ((fx - x0) as f32, (fy - y0) as f32);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let top = pixel(x0, y0) * (1.0 - u) + pixel(x0 + 1, y0) * u;
            let bottom = pixel(x0, y0 + 1) * (1.0 - u) + pixel(x0 + 1, y0 + 1) * u;
            f64::from(top * (1.0 - v) + bottom * v)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Terrarium PNG whose every pixel is `metres`.
    fn flat_tile(metres: f32) -> Vec<u8> {
        let v = metres + 32768.0;
        let (r, g, b) = ((v / 256.0).floor() as u8, (v % 256.0).floor() as u8, ((v.fract()) * 256.0) as u8);
        let mut img = Vec::new();
        for _ in 0..TILE * TILE {
            img.extend_from_slice(&[r, g, b]);
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, TILE as u32, TILE as u32);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&img).unwrap();
        }
        out
    }

    #[test]
    fn a_terrarium_pixel_is_read_as_metres_and_the_sea_is_level() {
        let h = decode_tile(&flat_tile(1234.5)).unwrap();
        assert_eq!(h.len(), TILE * TILE);
        assert!((h[0] - 1234.5).abs() < 0.01, "{}", h[0]);
        assert_eq!(decode_tile(&flat_tile(-300.0)).unwrap()[5], 0.0, "below the sea reads as sea level");
        assert!(decode_tile(b"not a picture").is_err());
        assert!(decode_tile(&[]).is_err());
    }

    #[test]
    fn a_world_pixel_follows_the_web_map_grid() {
        let (x, y) = world_pixel(0.0, 0.0, 1);
        assert!((x - 256.0).abs() < 1e-6 && (y - 256.0).abs() < 1e-6, "{x} {y}");
        let (x, _) = world_pixel(10.0, -180.0, 3);
        assert!(x.abs() < 1e-6);
    }

    #[test]
    fn a_lookup_around_a_place_needs_a_few_tiles_and_the_zoom_drops_when_it_would_need_too_many() {
        let ring: Vec<(f64, f64)> = (0..36)
            .map(|k| (43.3 + 0.54 * (f64::from(k) * 10.0).to_radians().cos(), -1.98 + 0.74 * (f64::from(k) * 10.0).to_radians().sin()))
            .collect();
        let (z, tiles) = plan(&ring);
        assert_eq!(z, BEST_ZOOM);
        assert!((4..=MAX_TILES).contains(&tiles.len()), "{}", tiles.len());
        let wide = [(-60.0, -170.0), (60.0, 170.0)];
        let (z, _) = plan(&wide);
        assert_eq!(z, MIN_ZOOM, "the zoom drops as far as it goes");
        assert!(heights_with(None, "http://127.0.0.1:9/{z}/{x}/{y}.png", &wide).is_err(), "too wide: refused before asking");
        // a district fits at the best zoom; a province a zoom or two lower
        let province: Vec<(f64, f64)> = vec![(42.0, -4.0), (44.0, -1.0)];
        assert!(plan(&province).0 < BEST_ZOOM);
    }

    use std::io::{BufRead, BufReader, Write};

    /// A server on this machine giving the same flat tile for every address.
    fn serve(metres: f32) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/{{z}}/{{x}}/{{y}}.png", listener.local_addr().unwrap());
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = count.clone();
        let tile = flat_tile(metres);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) <= 2 {
                        break;
                    }
                }
                let mut s = &stream;
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", tile.len());
                let _ = s.write_all(&tile);
            }
        });
        (base, count)
    }

    #[test]
    fn heights_are_read_from_the_tiles_and_the_tiles_are_kept() {
        let (base, count) = serve(800.0);
        let dir = std::env::temp_dir().join(format!("ls-terrain-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let points: Vec<(f64, f64)> = (0..50).map(|k| (43.3 + f64::from(k) * 0.01, -1.98)).collect();
        let h = heights_with(Some(&dir), &base, &points).unwrap();
        assert_eq!(h.len(), 50);
        assert!(h.iter().all(|v| (v - 800.0).abs() < 0.05), "{h:?}");
        let asked = count.load(std::sync::atomic::Ordering::SeqCst);
        assert!(asked >= 1);
        // again: the tiles are on the disk
        let again = heights_with(Some(&dir), &base, &points).unwrap();
        assert_eq!(again, h);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), asked, "no second request");
        assert!(heights_with(Some(&dir), &base, &[]).is_err());
        assert!(heights_with(Some(&dir), &base, &[(f64::NAN, 0.0)]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
