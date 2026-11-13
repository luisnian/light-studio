//! The colour of the ground round a place, from aerial photos, for the Mountains picture: each sample of the skyline's
//! grid (directions × rings of distance) gets the colour of the photo at its place. In Spain the IGN's PNOA
//! orthophotos (free, CC BY 4.0, «PNOA © Instituto Geográfico Nacional»); elsewhere Esri World Imagery, which the map's
//! satellite view already uses. The near ground is read from sharp tiles, the far ground from coarse ones (a ridge 50 km
//! away is a few pixels). Tiles are kept on disk, so looking again asks nothing.
//! `LIGHT_STUDIO_ORTHO_URL` points the tiles at another server (`{z}`, `{x}`, `{y}` in the address; tests).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use crate::services::map_tiles;

const PNOA: &str = "https://www.ign.es/wmts/pnoa-ma?service=WMTS&request=GetTile&version=1.0.0&layer=OI.OrthoimageCoverage&style=default&format=image/jpeg&tilematrixset=GoogleMapsCompatible&TileMatrix={z}&TileRow={y}&TileCol={x}";
const ESRI: &str = "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}";
/// The credit of each source.
pub const PNOA_CREDIT: &str = "Photo: PNOA © Instituto Geográfico Nacional (CC BY 4.0)";
pub const ESRI_CREDIT: &str = "Photo: © Esri · Maxar · Earthstar Geographics";

/// Up to this distance from the place (metres) a sample is read at this zoom: sharp near, coarse far.
const LEVELS: [(f64, u8); 3] = [(8_000.0, 13), (30_000.0, 11), (f64::MAX, 9)];
/// The most tiles one picture may need.
pub const MAX_TILES: usize = 220;
const TILE: f64 = 256.0;

/// The photo source for a place: its address template, a name for its folder in the cache, and its credit.
pub fn source(lat: f64, lon: f64) -> (&'static str, &'static str, &'static str) {
    // Spain, the Balearics and the Canaries are in the PNOA; a box a little wide is enough (outside it Esri is used)
    let spain = (35.8..=43.9).contains(&lat) && (-9.4..=4.4).contains(&lon) || (27.5..=29.5).contains(&lat) && (-18.3..=-13.3).contains(&lon);
    if spain { (PNOA, "pnoa", PNOA_CREDIT) } else { (ESRI, "esri", ESRI_CREDIT) }
}

/// The photo colour of every one of `samples` (latitude and longitude, direction by direction, `rings` to a direction;
/// `ring_m` is each ring's distance from the place `(lat, lon)`): RGB and 255, or alpha 0 where a tile is missing.
/// Needs the network unless the tiles were fetched before.
pub fn photo(lat: f64, lon: f64, samples: &[(f64, f64)], rings: usize, ring_m: &[f64]) -> Result<Vec<[u8; 4]>, String> {
    let (template, slug, _) = source(lat, lon);
    let template = std::env::var("LIGHT_STUDIO_ORTHO_URL").ok().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| template.to_string());
    let cache = crate::paths::config_dir().map(|d| d.join("ortho-tiles").join(slug));
    photo_with(&template, cache.as_deref(), lat, lon, samples, rings, ring_m)
}

/// Where a sample falls: zoom, tile column and row, pixel in the tile.
type Spot = (u8, u32, u32, usize, usize);

fn spot(lat: f64, lon: f64, zoom: u8) -> Spot {
    let n = f64::from(1u32 << zoom.min(20));
    let (wx, wy) = map_tiles::project(lat, lon);
    let (px, py) = ((wx * n * TILE).max(0.0), (wy * n * TILE).clamp(0.0, n * TILE - 1.0));
    let (tx, ty) = ((px / TILE).floor(), (py / TILE).floor());
    (zoom, tx as u32 % (1u32 << zoom.min(20)), ty as u32, (px - tx * TILE).floor() as usize % 256, (py - ty * TILE).floor() as usize % 256)
}

fn photo_with(
    template: &str,
    cache: Option<&Path>,
    lat: f64,
    lon: f64,
    samples: &[(f64, f64)],
    rings: usize,
    ring_m: &[f64],
) -> Result<Vec<[u8; 4]>, String> {
    if rings < 2
        || ring_m.len() != rings
        || samples.is_empty()
        || !samples.len().is_multiple_of(rings)
        || !(lat.is_finite() && lon.is_finite() && lat.abs() < 85.0)
    {
        return Err("that place has no photo".into());
    }
    if samples.iter().any(|(la, lo)| !(la.is_finite() && lo.is_finite() && la.abs() < 85.0)) {
        return Err("that place has no photo".into());
    }
    let zoom_of = |ring: usize| {
        let d = ring_m.get(ring).copied().unwrap_or(f64::MAX);
        LEVELS.iter().find(|(far, _)| d <= *far).map_or(9, |(_, z)| *z)
    };
    let spots: Vec<Spot> = samples.iter().enumerate().map(|(k, (la, lo))| spot(*la, *lo, zoom_of(k % rings))).collect();
    let needed: BTreeSet<(u8, u32, u32)> = spots.iter().map(|s| (s.0, s.1, s.2)).collect();
    if needed.len() > MAX_TILES {
        return Err("the photo needs too many tiles".into());
    }
    let wanted: Vec<(u8, u32, u32)> = needed.into_iter().collect();
    // a few at a time
    let results: Vec<((u8, u32, u32), Result<Option<map_tiles::Tile>, String>)> = std::thread::scope(|s| {
        let mut out = Vec::with_capacity(wanted.len());
        for batch in wanted.chunks(8) {
            let handles: Vec<_> = batch.iter().map(|&k| (k, s.spawn(move || tile(template, cache, k)))).collect();
            out.extend(handles.into_iter().map(|(k, h)| (k, h.join().unwrap_or_else(|_| Err("a photo tile could not be read".into())))));
        }
        out
    });
    let mut tiles: HashMap<(u8, u32, u32), map_tiles::Tile> = HashMap::new();
    let mut unreachable = 0usize;
    for (k, r) in results {
        match r {
            Ok(Some(t)) => {
                tiles.insert(k, t);
            }
            Ok(None) => {}
            Err(_) => unreachable += 1,
        }
    }
    // (no tile at all and the network the reason: say so; a few missing tiles are just gaps)
    if tiles.is_empty() && unreachable > 0 {
        return Err("could not reach the photo service (is the computer online?)".into());
    }
    Ok(spots
        .iter()
        .map(|&(z, x, y, px, py)| {
            let Some(t) = tiles.get(&(z, x, y)) else { return [0, 0, 0, 0] };
            let (w, h) = (t.width as usize, t.height as usize);
            // (tiles are 256 pixels; a bigger one is read at the same share of its width)
            let (qx, qy) = (px * w / 256, py * h / 256);
            match t.rgba.get((qy.min(h.saturating_sub(1)) * w + qx.min(w.saturating_sub(1))) * 4..).and_then(|p| p.get(..3)) {
                Some(p) => [p[0], p[1], p[2], 255],
                None => [0, 0, 0, 0],
            }
        })
        .collect())
}

/// One tile, from the disk or the network: `Ok(None)` when the server has none, `Err` when it cannot be reached.
fn tile(template: &str, cache: Option<&Path>, (z, x, y): (u8, u32, u32)) -> Result<Option<map_tiles::Tile>, String> {
    let file: Option<PathBuf> = cache.map(|d| d.join(z.to_string()).join(x.to_string()).join(format!("{y}.img")));
    if let Some(f) = file.as_deref()
        && let Ok(bytes) = std::fs::read(f)
        && let Some(t) = map_tiles::decode(&bytes)
    {
        return Ok(Some(t));
    }
    let address = template.replace("{z}", &z.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string());
    match map_tiles::fetch_tile(&address) {
        Ok(bytes) => {
            let t = map_tiles::decode(&bytes).ok_or("a photo tile came in a form that cannot be read")?;
            if let Some(f) = file.as_deref() {
                keep(f, &bytes);
            }
            Ok(Some(t))
        }
        Err(false) => Ok(None),
        Err(true) => Err("could not reach the photo service".into()),
    }
}

/// Keeps a tile (best effort).
fn keep(file: &Path, bytes: &[u8]) {
    let Some(dir) = file.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = file.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, file).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples on `rings` rings from `near` to `far` metres, in `azimuths` directions, and each ring's distance.
    fn ring_points(lat: f64, lon: f64, azimuths: usize, rings: usize, near: f64, far: f64) -> (Vec<(f64, f64)>, Vec<f64>) {
        let ring_m: Vec<f64> = (0..rings).map(|j| near * (far / near).powf(j as f64 / (rings - 1) as f64)).collect();
        let mut out = Vec::new();
        for i in 0..azimuths {
            let az = (360.0 * i as f64 / azimuths as f64).to_radians();
            for d in &ring_m {
                out.push((lat + d * az.cos() / 111_000.0, lon + d * az.sin() / (111_000.0 * lat.to_radians().cos())));
            }
        }
        (out, ring_m)
    }

    #[test]
    fn spain_has_its_own_photos_and_the_rest_of_the_world_the_other() {
        assert_eq!(source(43.32, -1.98).1, "pnoa");
        assert_eq!(source(28.27, -16.64).1, "pnoa", "Tenerife");
        assert_eq!(source(46.55, 8.0).1, "esri");
        assert_eq!(source(48.85, 2.35).1, "esri");
    }

    #[test]
    fn a_sample_lands_in_its_tile_and_pixel() {
        let (z, x, y, px, py) = spot(43.3, -2.0, 13);
        assert_eq!(z, 13);
        let n = f64::from(1u32 << 13);
        let (wx, wy) = map_tiles::project(43.3, -2.0);
        assert_eq!(x, (wx * n).floor() as u32);
        assert_eq!(y, (wy * n).floor() as u32);
        assert!(px < 256 && py < 256);
    }

    #[test]
    fn every_sample_gets_the_colour_of_its_tile_and_the_tiles_are_kept() {
        use std::io::{BufRead, BufReader, Write};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/{{z}}/{{x}}/{{y}}.jpg", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let seen = seen.clone();
                std::thread::spawn(move || {
                    let mut stream = stream;
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    loop {
                        let mut h = String::new();
                        reader.read_line(&mut h).unwrap();
                        if h.trim().is_empty() {
                            break;
                        }
                    }
                    seen.fetch_add(1, Ordering::SeqCst);
                    let img = image::RgbaImage::from_pixel(256, 256, image::Rgba([30, 160, 60, 255]));
                    let mut png = Vec::new();
                    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", png.len());
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&png);
                });
            }
        });
        let (samples, ring_m) = ring_points(43.3, -2.0, 24, 8, 150.0, 6_000.0);
        let dir = std::env::temp_dir().join(format!("ls-ortho-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let photo = photo_with(&base, Some(&dir), 43.3, -2.0, &samples, 8, &ring_m).unwrap();
        let asked = count.load(Ordering::SeqCst);
        assert!((1..=MAX_TILES).contains(&asked), "{asked}");
        assert_eq!(photo.len(), 24 * 8);
        assert!(photo.iter().all(|p| *p == [30, 160, 60, 255]));
        // again: from the disk
        let again = photo_with("http://127.0.0.1:1/{z}/{x}/{y}.jpg", Some(&dir), 43.3, -2.0, &samples, 8, &ring_m).unwrap();
        assert_eq!(again, photo);
        assert_eq!(count.load(Ordering::SeqCst), asked);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_service_that_is_not_there_says_so_and_nonsense_is_refused() {
        let url = "http://127.0.0.1:1/{z}/{x}/{y}.jpg";
        let (samples, ring_m) = ring_points(43.3, -2.0, 8, 4, 150.0, 3_000.0);
        let e = photo_with(url, None, 43.3, -2.0, &samples, 4, &ring_m).unwrap_err();
        assert!(e.contains("online"), "{e}");
        assert!(photo_with(url, None, f64::NAN, 0.0, &samples, 4, &ring_m).is_err());
        assert!(photo_with(url, None, 89.0, 0.0, &samples, 4, &ring_m).is_err());
        assert!(photo_with(url, None, 43.3, -2.0, &samples, 3, &ring_m).is_err(), "rings that do not match");
    }
}
