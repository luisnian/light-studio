//! Ground heights as terrain tiles, for the Planner's shadow map: the Terrarium set (Mapzen/AWS, free, no key; the
//! same one Luis's «Mapa solar» draws its shadows from) at zoom 11, about 56 m a pixel in the north of Spain.
//! A tile's colour is the height: `r × 256 + g + b / 256 − 32768` metres. The tiles round a place are fetched (a few
//! at a time), kept on disk, and joined into one grid in north-up Web Mercator pixels.
//! `LIGHT_STUDIO_TERRAIN_URL` points at another server with the same address pattern (`{z}/{x}/{y}`), for tests.

use std::path::{Path, PathBuf};

use super::map_tiles;

/// The tiles' address.
pub const SERVICE: &str = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png";
/// The zoom the tiles are used at.
pub const ZOOM: u8 = 11;
/// The tiles' side in pixels.
const TILE: usize = 256;
/// The most tiles one grid is made of (a side of five).
const MAX_TILES: usize = 25;
/// Tiles kept on disk.
const MAX_CACHED: usize = 500;
/// Metres round the equator.
const EQUATOR_M: f64 = 40_075_016.686;

/// The ground round a place.
#[derive(Clone, Debug)]
pub struct Dem {
    pub width: usize,
    pub height: usize,
    /// Metres above sea level (the sea, and anything lower, is 0), row by row from the north.
    pub heights: Vec<f32>,
    /// The grid's corners in Web Mercator world units (0..1, x east, y south): north-west and south-east.
    pub nw: (f64, f64),
    pub se: (f64, f64),
    /// Ground metres in one pixel.
    pub m_per_px: f64,
}

/// The grid of tiles over `radius_m` round the place: first tile, and how many each way.
pub fn tile_range(lat: f64, lon: f64, radius_m: f64, zoom: u8) -> Option<((u32, u32), (u32, u32))> {
    if !(lat.is_finite() && lon.is_finite() && radius_m.is_finite() && radius_m > 0.0 && lat.abs() <= 85.0 && lon.abs() <= 180.0) {
        return None;
    }
    let n = f64::from(1u32 << zoom.min(20));
    let (wx, wy) = map_tiles::project(lat, lon);
    let r = radius_m / (EQUATOR_M * lat.to_radians().cos());
    let tile = |w: f64| (w * n).floor().clamp(0.0, n - 1.0) as u32;
    let (x0, x1, y0, y1) = (tile(wx - r), tile(wx + r), tile(wy - r), tile(wy + r));
    let (nx, ny) = (x1 - x0 + 1, y1 - y0 + 1);
    (nx as usize * ny as usize <= MAX_TILES).then_some(((x0, y0), (nx, ny)))
}

/// A terrain tile's heights from its picture.
pub fn heights_of(rgba: &[u8], width: usize, height: usize) -> Option<Vec<f32>> {
    if width != TILE || height != TILE || rgba.len() != width * height * 4 {
        return None;
    }
    Some(rgba.chunks_exact(4).map(|p| (f32::from(p[0]) * 256.0 + f32::from(p[1]) + f32::from(p[2]) / 256.0 - 32_768.0).max(0.0)).collect())
}

/// The ground within `radius_m` of the place (a little more: whole tiles); needs the network unless the tiles were
/// fetched before.
pub fn fetch(lat: f64, lon: f64, radius_m: f64) -> Result<Dem, String> {
    let template = std::env::var("LIGHT_STUDIO_TERRAIN_URL").ok().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| SERVICE.to_string());
    let cache = crate::paths::config_dir().map(|d| d.join("terrain-tiles"));
    fetch_with(&template, cache.as_deref(), lat, lon, radius_m, ZOOM)
}

fn fetch_with(template: &str, cache: Option<&Path>, lat: f64, lon: f64, radius_m: f64, zoom: u8) -> Result<Dem, String> {
    let ((x0, y0), (nx, ny)) = tile_range(lat, lon, radius_m, zoom).ok_or("that place has no terrain tiles")?;
    let (nx, ny) = (nx as usize, ny as usize);
    let coords: Vec<(usize, usize, u32, u32)> = (0..ny).flat_map(|j| (0..nx).map(move |i| (i, j, x0 + i as u32, y0 + j as u32))).collect();
    // a few at a time
    let results: Vec<Result<Vec<f32>, String>> = std::thread::scope(|s| {
        let mut out = Vec::with_capacity(coords.len());
        for batch in coords.chunks(6) {
            let handles: Vec<_> = batch.iter().map(|&(_, _, x, y)| s.spawn(move || tile_heights(template, cache, zoom, x, y))).collect();
            out.extend(handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("a terrain tile could not be read".into()))));
        }
        out
    });
    let (width, height) = (nx * TILE, ny * TILE);
    let mut heights = vec![0.0_f32; width * height];
    for (&(i, j, _, _), tile) in coords.iter().zip(results) {
        let tile = tile?;
        for row in 0..TILE {
            let dst = (j * TILE + row) * width + i * TILE;
            if let (Some(d), Some(s)) = (heights.get_mut(dst..dst + TILE), tile.get(row * TILE..(row + 1) * TILE)) {
                d.copy_from_slice(s);
            }
        }
    }
    let n = f64::from(1u32 << zoom.min(20));
    let (_, wy) = map_tiles::project(lat, lon);
    Ok(Dem {
        width,
        height,
        heights,
        nw: (f64::from(x0) / n, f64::from(y0) / n),
        se: (f64::from(x0 + nx as u32) / n, f64::from(y0 + ny as u32) / n),
        m_per_px: EQUATOR_M * lat.to_radians().cos() / (TILE as f64 * n) * (1.0 + 0.0 * wy),
    })
}

/// One tile's heights, from the disk or the network. A tile the server has not got is the sea (0).
fn tile_heights(template: &str, cache: Option<&Path>, zoom: u8, x: u32, y: u32) -> Result<Vec<f32>, String> {
    let file: Option<PathBuf> = cache.map(|d| d.join(zoom.to_string()).join(x.to_string()).join(format!("{y}.png")));
    if let Some(f) = file.as_deref()
        && let Ok(bytes) = std::fs::read(f)
        && let Some(h) = decode(&bytes)
    {
        return Ok(h);
    }
    let address = template.replace("{z}", &zoom.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string());
    match map_tiles::fetch_tile(&address) {
        Ok(bytes) => {
            let h = decode(&bytes).ok_or("a terrain tile came in a form that cannot be read")?;
            if let Some(f) = file.as_deref() {
                write_cache(f, &bytes);
            }
            Ok(h)
        }
        Err(false) => Ok(vec![0.0; TILE * TILE]),
        Err(true) => Err("could not reach the terrain service (is the computer online?)".into()),
    }
}

fn decode(bytes: &[u8]) -> Option<Vec<f32>> {
    let t = map_tiles::decode(bytes)?;
    heights_of(&t.rgba, t.width as usize, t.height as usize)
}

/// Keeps a tile (best effort), and the folder to the newest few hundred.
fn write_cache(file: &Path, bytes: &[u8]) {
    let Some(dir) = file.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = file.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, file).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    // (once in a while: count the whole cache and cut it)
    if file.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.len() % 7 == 0) {
        prune(file.ancestors().nth(3));
    }
}

fn prune(root: Option<&Path>) {
    let Some(root) = root else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(p),
                Ok(m) => {
                    if let Ok(t) = m.modified() {
                        files.push((t, p));
                    }
                }
                Err(_) => {}
            }
        }
    }
    if files.len() > MAX_CACHED {
        files.sort();
        let extra = files.len() - MAX_CACHED;
        for (_, p) in files.into_iter().take(extra) {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tiles_round_a_place_are_few_and_hold_it() {
        let ((x0, y0), (nx, ny)) = tile_range(43.3, -2.0, 25_000.0, 11).unwrap();
        assert!((1..=5).contains(&nx) && (1..=5).contains(&ny), "{nx}×{ny}");
        let (wx, wy) = map_tiles::project(43.3, -2.0);
        let n = f64::from(1u32 << 11);
        assert!(f64::from(x0) <= wx * n && wx * n < f64::from(x0 + nx) && f64::from(y0) <= wy * n && wy * n < f64::from(y0 + ny));
        assert!(tile_range(43.3, -2.0, 25_000_000.0, 11).is_none(), "too many tiles");
        assert!(tile_range(f64::NAN, 0.0, 1000.0, 11).is_none());
        assert!(tile_range(89.0, 0.0, 1000.0, 11).is_none());
    }

    #[test]
    fn a_colour_is_a_height() {
        let mut px = vec![0u8; TILE * TILE * 4];
        // 32768 + 1234.5 m = r 0x84 (132), g 0xD2 (210)… as terrarium writes it: (h + 32768) = r·256 + g + b/256
        let v: f64 = 32_768.0 + 1234.5;
        px[0] = (v / 256.0).floor() as u8;
        px[1] = (v % 256.0).floor() as u8;
        px[2] = (((v - v.floor()) * 256.0).round()) as u8;
        px[4] = 126; // below the sea: 126·256 − 32768 = −512
        let h = heights_of(&px, TILE, TILE).unwrap();
        assert!((h[0] - 1234.5).abs() < 0.01, "{}", h[0]);
        assert_eq!(h[1], 0.0, "under the sea level is the sea");
        assert!(heights_of(&px, 100, 100).is_none());
    }

    #[test]
    fn tiles_are_fetched_joined_kept_and_read_again_without_the_network() {
        use std::io::{BufRead, BufReader, Write};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        // a server giving every tile a height of 100 m + 10 m for each column of its x (so that the join shows)
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/{{z}}/{{x}}/{{y}}.png", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let seen = seen.clone();
                std::thread::spawn(move || {
                    let mut stream = stream;
                    let mut line = String::new();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    reader.read_line(&mut line).unwrap();
                    loop {
                        let mut h = String::new();
                        reader.read_line(&mut h).unwrap();
                        if h.trim().is_empty() {
                            break;
                        }
                    }
                    seen.fetch_add(1, Ordering::SeqCst);
                    let x: u32 = line.split('/').nth(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let v = 32_768.0 + 100.0 + f64::from(x % 100) * 10.0;
                    let px = image::Rgba([(v / 256.0).floor() as u8, (v % 256.0).floor() as u8, 0, 255]);
                    let img = image::RgbaImage::from_pixel(TILE as u32, TILE as u32, px);
                    let mut png = Vec::new();
                    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", png.len());
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&png);
                });
            }
        });
        let dir = std::env::temp_dir().join(format!("ls-terrain-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dem = fetch_with(&base, Some(&dir), 43.3, -2.0, 25_000.0, 11).unwrap();
        let tiles = count.load(Ordering::SeqCst);
        assert!(tiles >= 4, "{tiles}");
        assert_eq!(dem.heights.len(), dem.width * dem.height);
        assert_eq!(dem.width % TILE, 0);
        // the first and the last column come from different tiles
        assert!((dem.heights[0] - dem.heights[dem.width - 1]).abs() > 5.0, "{} {}", dem.heights[0], dem.heights[dem.width - 1]);
        assert!(dem.heights[0] >= 100.0 && dem.heights[0] < 1100.0);
        assert!(dem.nw.0 < dem.se.0 && dem.nw.1 < dem.se.1);
        assert!(dem.m_per_px > 30.0 && dem.m_per_px < 80.0, "{}", dem.m_per_px);
        // the second time: from the disk
        let again = fetch_with(&base, Some(&dir), 43.3, -2.0, 25_000.0, 11).unwrap();
        assert_eq!(count.load(Ordering::SeqCst), tiles, "no more requests");
        assert_eq!(again.heights, dem.heights);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_service_that_is_not_there_says_so() {
        let e = fetch_with("http://127.0.0.1:1/{z}/{x}/{y}.png", None, 43.3, -2.0, 5_000.0, 11).unwrap_err();
        assert!(e.contains("online") || e.contains("terrain"), "{e}");
    }
}
