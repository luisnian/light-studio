//! Finer ground heights near a place for the Mountains view, from Mapterhorn's open terrain tiles: one PMTiles archive on
//! a public server (`planet.pmtiles`, zoom 12 and below), whose tiles are 512 × 512 WebP pictures in the Terrarium
//! encoding (`R·256 + G + B/256 − 32768` metres), twice as sharp as the 256-pixel tiles of the same zoom the AWS
//! service gives. Only the bytes of the tiles asked for are fetched (HTTP `Range`), and they are kept on disk.
//!
//! The ground a few kilometres round the place decides the nearest ridges, so only that disc (`RADIUS_M`) is read from
//! here; the rest comes from the AWS tiles as before. Any failure (offline, the server changed) is an error that makes
//! the caller use the AWS tiles for everything, and is remembered for the session so it is not tried again and again.
//! Credit: © Mapterhorn (<https://mapterhorn.com/attribution>).
//! `LIGHT_STUDIO_MAPTERHORN_URL` points at another archive (tests).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::services::pmtiles::{Archive, ReadAt};

/// The archive's address.
pub const SERVICE: &str = "https://download.mapterhorn.com/planet.pmtiles";
/// The credit line.
pub const CREDIT: &str = "© Mapterhorn";
const ZOOM: u8 = 12;
const TILE: usize = 512;
/// The disc round the place read from here (metres).
pub const RADIUS_M: f64 = 16_000.0;
/// The most tiles one lookup may need.
const MAX_TILES: usize = 64;
/// Tiles kept on disk.
const MAX_CACHED: usize = 400;
/// The most bytes one range request may ask for.
const MAX_RANGE: u64 = 16 * 1024 * 1024;

/// The archive on a server, read with byte ranges.
#[derive(Debug)]
struct HttpReader {
    url: String,
}

impl ReadAt for HttpReader {
    fn read_at(&self, offset: u64, length: u64) -> Result<Vec<u8>, String> {
        use crate::net;
        const BAD: &str = "a range of the terrain archive is not what was expected";
        if length == 0 || length > MAX_RANGE {
            return Err(BAD.into());
        }
        let last = offset.checked_add(length - 1).ok_or(BAD)?;
        // (one byte more than asked, so an answer that is longer than the range is seen as such, not cut)
        let max = usize::try_from(length).map_err(|_| BAD.to_string())?.saturating_add(1);
        // (the client follows the server when it sends us on to another address)
        let r = net::get(&self.url, &[("Range", format!("bytes={offset}-{last}"))], net::Timeouts::secs(8, 30), max).map_err(|e| match e {
            net::NetError::TooBig => "the terrain service answered more than was asked".to_string(),
            net::NetError::BadUrl => "the terrain service address is not valid".to_string(),
            net::NetError::Unreachable(_) => "could not reach the terrain service (is the computer online?)".to_string(),
        })?;
        if r.status != 206 {
            return Err(format!("the terrain service answered {}", r.status));
        }
        if r.body.len() as u64 == length { Ok(r.body) } else { Err("the terrain service answered a different length than was asked".into()) }
    }
}

/// Set once the finer terrain could not be had, so the rest of the session does not wait for it again.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The finer terrain: the archive (opened only when a tile is not on disk) and where the tiles are kept.
pub struct Fine {
    url: String,
    dir: Option<PathBuf>,
    radius_m: f64,
    archive: OnceLock<Result<Archive, String>>,
}

/// The tile column and row (at `ZOOM`) and the pixel position on the world's grid of 512-pixel tiles of a point.
fn world_pixel(lat: f64, lon: f64) -> (f64, f64) {
    let n = f64::from(1u32 << ZOOM) * TILE as f64;
    let lat = lat.clamp(-85.05, 85.05).to_radians();
    ((lon + 180.0) / 360.0 * n, (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * n)
}

/// Terrarium pixels as heights in metres (below the sea reads as the sea).
fn heights_of(rgba: &[u8], width: usize, height: usize) -> Option<Vec<f32>> {
    if width != TILE || height != TILE || rgba.len() != width.checked_mul(height)?.checked_mul(4)? {
        return None;
    }
    Some(rgba.chunks_exact(4).map(|p| (f32::from(p[0]) * 256.0 + f32::from(p[1]) + f32::from(p[2]) / 256.0 - 32768.0).clamp(0.0, 9_000.0)).collect())
}

fn decode(bytes: &[u8]) -> Option<Vec<f32>> {
    let t = crate::services::map_tiles::decode(bytes)?;
    heights_of(&t.rgba, t.width as usize, t.height as usize)
}

fn cache_file(dir: &Path, x: u32, y: u32) -> PathBuf {
    dir.join(ZOOM.to_string()).join(x.to_string()).join(format!("{y}.webp"))
}

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
    if WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 20 != 19 {
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

fn kept(dir: &Path, out: &mut Vec<(std::time::SystemTime, PathBuf)>, depth: usize) {
    if depth > 4 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        match e.metadata() {
            Ok(m) if m.is_dir() => kept(&p, out, depth + 1),
            Ok(m) if p.extension().is_some_and(|x| x == "webp") => {
                if let Ok(t) = m.modified() {
                    out.push((t, p));
                }
            }
            _ => {}
        }
    }
}

impl Fine {
    /// The finer terrain at the usual place: Mapterhorn's server and a folder in the settings folder.
    pub fn standard() -> Fine {
        let url = std::env::var("LIGHT_STUDIO_MAPTERHORN_URL")
            .ok()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| SERVICE.to_string());
        Fine::new(url, crate::paths::config_dir().map(|d| d.join("terrain-mapterhorn")), RADIUS_M)
    }

    pub(crate) fn new(url: String, dir: Option<PathBuf>, radius_m: f64) -> Fine {
        Fine { url, dir, radius_m, archive: OnceLock::new() }
    }

    fn archive(&self) -> Result<&Archive, String> {
        self.archive.get_or_init(|| Archive::from_reader(Box::new(HttpReader { url: self.url.clone() }), None)).as_ref().map_err(Clone::clone)
    }

    /// A tile's heights: from the disk, else from the archive (and kept).
    fn tile(&self, x: u32, y: u32) -> Result<Vec<f32>, String> {
        if let Some(d) = self.dir.as_deref()
            && let Ok(bytes) = std::fs::read(cache_file(d, x, y))
            && let Some(h) = decode(&bytes)
        {
            return Ok(h);
        }
        let bytes = self.archive()?.tile(ZOOM, x, y)?.ok_or_else(|| "the terrain archive has no such tile".to_string())?;
        let h = decode(&bytes).ok_or_else(|| "a terrain tile is not what was expected".to_string())?;
        if let Some(d) = self.dir.as_deref() {
            keep(d, &cache_file(d, x, y), &bytes);
        }
        Ok(h)
    }

    /// The height of every point of `points` that lies within the radius of the first one, `None` for the others (the
    /// caller reads those elsewhere). An error when any tile is missing or the server cannot be reached.
    pub fn near_heights(&self, points: &[(f64, f64)]) -> Result<Vec<Option<f64>>, String> {
        let &(lat0, lon0) = points.first().ok_or("no points")?;
        if !(lat0.is_finite() && lon0.is_finite() && lat0.abs() < 85.0 && lon0.abs() <= 180.0) {
            return Err("that is not a place on Earth".into());
        }
        let (kx, ky) = (111_320.0 * lat0.to_radians().cos(), 110_540.0);
        let near: Vec<bool> = points
            .iter()
            .map(|&(la, lo)| {
                let (dx, dy) = ((lo - lon0) * kx, (la - lat0) * ky);
                la.is_finite() && lo.is_finite() && dx.hypot(dy) <= self.radius_m
            })
            .collect();
        let count = 1u32 << ZOOM;
        let tile_of = |v: f64| ((v / TILE as f64).floor().max(0.0) as u32).min(count - 1);
        let mut needed: BTreeSet<(u32, u32)> = BTreeSet::new();
        for (&(la, lo), _) in points.iter().zip(&near).filter(|(_, n)| **n) {
            let (x, y) = world_pixel(la, lo);
            // (a pixel to spare each way for the bilinear read)
            for (dx, dy) in [(-2.0, -2.0), (2.0, 2.0)] {
                needed.insert((tile_of(x + dx), tile_of(y + dy)));
            }
            needed.insert((tile_of(x - 2.0), tile_of(y + 2.0)));
            needed.insert((tile_of(x + 2.0), tile_of(y - 2.0)));
        }
        if needed.len() > MAX_TILES {
            return Err("that is too wide an area for the finer terrain".into());
        }
        let list: Vec<(u32, u32)> = needed.into_iter().collect();
        let tiles = self.load_all(&list)?;
        let world = i64::from(count) * TILE as i64;
        let pixel = |px: i64, py: i64| -> f32 {
            let px = px.rem_euclid(world);
            let py = py.clamp(0, world - 1);
            let (tx, ty) = ((px / TILE as i64) as u32, (py / TILE as i64) as u32);
            let (ix, iy) = ((px % TILE as i64) as usize, (py % TILE as i64) as usize);
            tiles.get(&(tx, ty)).and_then(|t| t.get(iy * TILE + ix)).copied().unwrap_or(0.0)
        };
        Ok(points
            .iter()
            .zip(&near)
            .map(|(&(la, lo), &n)| {
                n.then(|| {
                    let (x, y) = world_pixel(la, lo);
                    let (fx, fy) = (x - 0.5, y - 0.5);
                    let (x0, y0) = (fx.floor(), fy.floor());
                    let (u, v) = ((fx - x0) as f32, (fy - y0) as f32);
                    let (x0, y0) = (x0 as i64, y0 as i64);
                    let top = pixel(x0, y0) * (1.0 - u) + pixel(x0 + 1, y0) * u;
                    let bottom = pixel(x0, y0 + 1) * (1.0 - u) + pixel(x0 + 1, y0 + 1) * u;
                    f64::from(top * (1.0 - v) + bottom * v)
                })
            })
            .collect())
    }

    /// The tiles, a few at a time.
    fn load_all(&self, list: &[(u32, u32)]) -> Result<HashMap<(u32, u32), Vec<f32>>, String> {
        // (the archive's index is read once before the workers need it)
        if let Some(&(x, y)) = list.first() {
            let first = self.tile(x, y)?;
            let results = std::sync::Mutex::new(HashMap::from([((x, y), first)]));
            let failure = std::sync::Mutex::new(None::<String>);
            let next = std::sync::atomic::AtomicUsize::new(1);
            std::thread::scope(|s| {
                for _ in 0..6.min(list.len().max(1)) {
                    s.spawn(|| {
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(&(x, y)) = list.get(i) else { break };
                            if failure.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_some() {
                                break;
                            }
                            match self.tile(x, y) {
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
            return Ok(results.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner));
        }
        Ok(HashMap::new())
    }
}

/// Whether the finer terrain has been given up on for this session.
pub fn given_up() -> bool {
    FAILED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Gives up on the finer terrain for the rest of the session.
pub fn give_up() {
    FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A lossless 512-pixel WebP of one height (Terrarium).
    fn flat_tile(metres: f64) -> Vec<u8> {
        let v = metres + 32768.0;
        let (r, g, b) = ((v / 256.0).floor() as u8, (v % 256.0).floor() as u8, ((v.fract()) * 256.0) as u8);
        let img = image::RgbaImage::from_pixel(TILE as u32, TILE as u32, image::Rgba([r, g, b, 255]));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::WebP).unwrap();
        out
    }

    /// Serves `body` with byte ranges on a local port; counts the requests.
    pub(crate) fn serve(body: Vec<u8>, ranges: bool) -> (String, Arc<AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/planet.pmtiles", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let body = Arc::new(body);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (seen, body) = (seen.clone(), body.clone());
                std::thread::spawn(move || {
                    let mut stream = stream;
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let mut range = None;
                    loop {
                        let mut h = String::new();
                        reader.read_line(&mut h).unwrap();
                        if h.trim().is_empty() {
                            break;
                        }
                        if let Some(r) = h.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            let (a, b) = r.trim().split_once('-').unwrap();
                            range = Some((a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()));
                        }
                    }
                    seen.fetch_add(1, Ordering::SeqCst);
                    match range {
                        Some((a, b)) if ranges => {
                            let part = &body[a..=b.min(body.len() - 1)];
                            let head = format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{}/{}\r\nConnection: close\r\n\r\n",
                                part.len(),
                                a + part.len() - 1,
                                body.len()
                            );
                            let _ = stream.write_all(head.as_bytes());
                            let _ = stream.write_all(part);
                        }
                        _ => {
                            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                            let _ = stream.write_all(head.as_bytes());
                            let _ = stream.write_all(&body);
                        }
                    }
                });
            }
        });
        (url, count)
    }

    /// An archive with a flat tile of `metres` for every tile round the place at `radius_m`.
    pub(crate) fn archive_round(lat: f64, lon: f64, radius_m: f64, metres: f64) -> Vec<u8> {
        let (x, y) = world_pixel(lat, lon);
        let n = (radius_m / 14.0) as i64 + 4; // pixels (a pixel is about 14 m at 43°… generously)
        let (x0, x1) = (((x - n as f64) / TILE as f64).floor() as u32, ((x + n as f64) / TILE as f64).floor() as u32);
        let (y0, y1) = (((y - n as f64) / TILE as f64).floor() as u32, ((y + n as f64) / TILE as f64).floor() as u32);
        let tile = flat_tile(metres);
        let tiles: Vec<(u8, u32, u32, Vec<u8>)> =
            (x0..=x1).flat_map(|tx| (y0..=y1).map(move |ty| (tx, ty))).map(|(tx, ty)| (ZOOM, tx, ty, tile.clone())).collect();
        let mut bytes = crate::services::pmtiles::tests::build(&tiles, true, true, 4);
        // (the builder's header says zoom 0 to 4)
        bytes[101] = ZOOM;
        bytes
    }

    #[test]
    fn terrarium_pixels_read_as_metres() {
        let mut rgba = vec![0u8; TILE * TILE * 4];
        // 1 234.5 m: 34 002.5 → R 132, G 210, B 128
        rgba[..4].copy_from_slice(&[132, 210, 128, 255]);
        rgba[4..8].copy_from_slice(&[127, 0, 0, 255]); // below the sea
        let h = heights_of(&rgba, TILE, TILE).unwrap();
        assert!((h[0] - 1234.5).abs() < 1e-3, "{}", h[0]);
        assert_eq!(h[1], 0.0);
        assert!(heights_of(&rgba, 256, 256).is_none() && heights_of(&rgba[..40], TILE, TILE).is_none());
    }

    #[test]
    fn a_webp_tile_round_trips_to_its_height() {
        let h = decode(&flat_tile(612.25)).expect("a tile");
        assert_eq!(h.len(), TILE * TILE);
        assert!(h.iter().all(|v| (v - 612.25).abs() < 0.01), "{}", h[0]);
    }

    #[test]
    fn near_points_are_read_from_the_archive_by_ranges_and_far_ones_are_left_to_the_caller() {
        let (lat, lon) = (43.3, -1.98);
        let (url, count) = serve(archive_round(lat, lon, 3_500.0, 480.0), true);
        let dir = std::env::temp_dir().join(format!("ls-fine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fine = Fine::new(url, Some(dir.clone()), 3_000.0);
        // the place, a point 1 km east, a point 20 km east (beyond the radius)
        let far = (lat, lon + 20_000.0 / (111_320.0 * lat.to_radians().cos()));
        let east = (lat, lon + 1_000.0 / (111_320.0 * lat.to_radians().cos()));
        let h = fine.near_heights(&[(lat, lon), east, far]).unwrap();
        assert!((h[0].unwrap() - 480.0).abs() < 0.05 && (h[1].unwrap() - 480.0).abs() < 0.05, "{h:?}");
        assert_eq!(h[2], None);
        let asked = count.load(Ordering::SeqCst);
        assert!(asked >= 3, "header, index and a tile at least: {asked}");
        // again with the server gone: the kept tiles are enough
        let again = Fine::new("http://127.0.0.1:1/planet.pmtiles".into(), Some(dir.clone()), 3_000.0);
        assert_eq!(again.near_heights(&[(lat, lon), east, far]).unwrap(), h);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_server_without_ranges_or_not_there_is_an_error_and_so_is_nonsense() {
        let (lat, lon) = (43.3, -1.98);
        let (url, _) = serve(archive_round(lat, lon, 3_500.0, 480.0), false);
        let e = Fine::new(url, None, 3_000.0).near_heights(&[(lat, lon)]).unwrap_err();
        assert!(!e.is_empty());
        let e = Fine::new("http://127.0.0.1:1/planet.pmtiles".into(), None, 3_000.0).near_heights(&[(lat, lon)]).unwrap_err();
        assert!(e.contains("online"), "{e}");
        let fine = Fine::new("http://127.0.0.1:1/x".into(), None, 3_000.0);
        assert!(fine.near_heights(&[]).is_err());
        assert!(fine.near_heights(&[(f64::NAN, 0.0)]).is_err());
        assert!(fine.near_heights(&[(89.9, 0.0)]).is_err());
        // a huge disc needs too many tiles
        let wide = Fine::new("http://127.0.0.1:1/x".into(), None, 4_000_000.0);
        let spread: Vec<(f64, f64)> = (0..120).map(|i| (43.3 + 0.05 * f64::from(i), -1.98 + 0.07 * f64::from(i))).collect();
        let e = wide.near_heights(&spread).unwrap_err();
        assert!(e.contains("too wide"), "{e}");
    }
}
