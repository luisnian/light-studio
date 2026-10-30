//! Fine terrain for the shadow map from a folder of 1-metre LiDAR height models on this computer: ArcInfo ASCII grids
//! (`.asc`, ETRS89 / UTM 30N, EPSG:25830) in 1 km boxes, as Gipuzkoa's b5m publishes them
//! (<https://b5m.gipuzkoa.eus/lidar/>; credit the Provincial Council of Gipuzkoa, CC BY 4.0 for the site's content: check
//! the terms of each download). This is a prototype for one region: it reads what the user has downloaded, it fetches
//! nothing.
//!
//! The area round the place is read box by box, each box averaged down to a coarser cell as it is read (so a few
//! megabytes are held, not the gigabytes of the full set), and resampled onto the web map's grid as a [`Dem`]. It is used
//! only when every box that area needs is in the folder; else the global terrain tiles serve as before.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::map_tiles;
use super::terrain_tiles::Dem;

/// The credit to show when this terrain is used.
pub const ATTRIBUTION: &str = "Terrain: b5m Gipuzkoa";
/// How far round the place the fine terrain reaches (metres) and the cell it is averaged to.
pub const RADIUS_M: f64 = 3000.0;
pub const STEP_M: f64 = 4.0;
const MAX_FILES: usize = 20_000;
const MAX_SIDE: usize = 5000;
const EQUATOR_M: f64 = 40_075_016.686;

// ------------------------------------------------------------------------------------------- UTM 30N

const A: f64 = 6_378_137.0;
const F: f64 = 1.0 / 298.257_222_101;
const K0: f64 = 0.9996;
const E0: f64 = 500_000.0;
const LON0: f64 = -3.0;

fn series(n: f64) -> (f64, [f64; 4], [f64; 4], [f64; 4]) {
    let (n2, n3, n4) = (n * n, n * n * n, n * n * n * n);
    let big_a = A / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0);
    let alpha = [
        n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0,
        13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0,
        61.0 * n3 / 240.0 - 103.0 * n4 / 140.0,
        49_561.0 * n4 / 161_280.0,
    ];
    let beta = [
        n / 2.0 - 2.0 * n2 / 3.0 + 37.0 * n3 / 96.0 - n4 / 360.0,
        n2 / 48.0 + n3 / 15.0 - 437.0 * n4 / 1440.0,
        17.0 * n3 / 480.0 - 37.0 * n4 / 840.0,
        4397.0 * n4 / 161_280.0,
    ];
    let delta = [
        2.0 * n - 2.0 * n2 / 3.0 - 2.0 * n3 + 116.0 * n4 / 45.0,
        7.0 * n2 / 3.0 - 8.0 * n3 / 5.0 - 227.0 * n4 / 45.0,
        56.0 * n3 / 15.0 - 136.0 * n4 / 35.0,
        4279.0 * n4 / 630.0,
    ];
    (big_a, alpha, beta, delta)
}

/// Latitude and longitude (degrees) to UTM zone 30 easting and northing (metres), by Krüger's series.
pub fn to_utm(lat: f64, lon: f64) -> (f64, f64) {
    let n = F / (2.0 - F);
    let (big_a, alpha, _, _) = series(n);
    let phi = lat.to_radians();
    let dl = (lon - LON0).to_radians();
    let c = 2.0 * n.sqrt() / (1.0 + n);
    let t = (phi.sin().atanh() - c * (c * phi.sin()).atanh()).sinh();
    let xi1 = t.atan2(dl.cos());
    let eta1 = (dl.sin() / (1.0 + t * t).sqrt()).atanh();
    let (mut xi, mut eta) = (xi1, eta1);
    for (j, a) in alpha.iter().enumerate() {
        let k = 2.0 * (j as f64 + 1.0);
        xi += a * (k * xi1).sin() * (k * eta1).cosh();
        eta += a * (k * xi1).cos() * (k * eta1).sinh();
    }
    (E0 + K0 * big_a * eta, K0 * big_a * xi)
}

/// UTM zone 30 easting and northing to latitude and longitude (degrees).
pub fn from_utm(e: f64, nrt: f64) -> (f64, f64) {
    let n = F / (2.0 - F);
    let (big_a, _, beta, delta) = series(n);
    let xi = nrt / (K0 * big_a);
    let eta = (e - E0) / (K0 * big_a);
    let (mut xi1, mut eta1) = (xi, eta);
    for (j, b) in beta.iter().enumerate() {
        let k = 2.0 * (j as f64 + 1.0);
        xi1 -= b * (k * xi).sin() * (k * eta).cosh();
        eta1 -= b * (k * xi).cos() * (k * eta).sinh();
    }
    let chi = (xi1.sin() / eta1.cosh()).asin();
    let mut phi = chi;
    for (j, d) in delta.iter().enumerate() {
        phi += d * (2.0 * (j as f64 + 1.0) * chi).sin();
    }
    (phi.to_degrees(), LON0 + eta1.sinh().atan2(xi1.cos()).to_degrees())
}

// ------------------------------------------------------------------------------------------- the files

/// What an `.asc` header says.
#[derive(Clone, Debug, PartialEq)]
struct Header {
    ncols: usize,
    nrows: usize,
    /// Lower-left corner of the grid (the corner of its first cell, metres).
    x: f64,
    y: f64,
    cell: f64,
    nodata: f64,
}

fn read_header(lines: &mut impl Iterator<Item = std::io::Result<String>>) -> Option<Header> {
    let (mut ncols, mut nrows, mut x, mut y, mut cell, mut nodata) = (None, None, None, None, None, -9999.0);
    let (mut x_center, mut y_center) = (false, false);
    for _ in 0..6 {
        let line = lines.next()?.ok()?;
        let mut it = line.split_whitespace();
        let key = it.next()?.to_ascii_lowercase();
        let v: f64 = it.next()?.replace(',', ".").parse().ok()?;
        match key.as_str() {
            "ncols" => ncols = Some(v as usize),
            "nrows" => nrows = Some(v as usize),
            "xllcorner" => x = Some(v),
            "xllcenter" => (x, x_center) = (Some(v), true),
            "yllcorner" => y = Some(v),
            "yllcenter" => (y, y_center) = (Some(v), true),
            "cellsize" => cell = Some(v),
            "nodata_value" => nodata = v,
            _ => return None,
        }
    }
    let (ncols, nrows, cell) = (ncols?, nrows?, cell?);
    if !(1..=MAX_SIDE).contains(&ncols) || !(1..=MAX_SIDE).contains(&nrows) || !cell.is_finite() || cell <= 0.0 || cell > 100.0 {
        return None;
    }
    let (mut x, mut y) = (x?, y?);
    if x_center {
        x -= cell / 2.0;
    }
    if y_center {
        y -= cell / 2.0;
    }
    (x.is_finite() && y.is_finite()).then_some(Header { ncols, nrows, x, y, cell, nodata })
}

/// A file of the folder: where it is and what it covers.
#[derive(Clone, Debug)]
struct Entry {
    path: PathBuf,
    head: Header,
}

/// The folder: its `.asc` files, indexed by the kilometre squares they overlap.
#[derive(Debug, Default)]
pub struct Index {
    files: Vec<Entry>,
    by_km: HashMap<(i64, i64), Vec<usize>>,
}

impl Index {
    /// Reads the headers of the `.asc` files in `dir` and in its folders one level down.
    pub fn scan(dir: &Path) -> Result<Index, String> {
        let mut paths = Vec::new();
        let mut stack = vec![(dir.to_path_buf(), 0)];
        while let Some((d, depth)) = stack.pop() {
            for e in std::fs::read_dir(&d).map_err(|e| format!("could not read the folder: {e}"))?.flatten() {
                let p = e.path();
                if p.is_dir() && depth < 1 {
                    stack.push((p, depth + 1));
                } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("asc")) && paths.len() < MAX_FILES {
                    paths.push(p);
                }
            }
        }
        let mut idx = Index::default();
        for path in paths {
            let Ok(f) = std::fs::File::open(&path) else { continue };
            let Some(head) = read_header(&mut BufReader::new(f).lines()) else { continue };
            let (x1, y1) = (head.x + head.ncols as f64 * head.cell, head.y + head.nrows as f64 * head.cell);
            let i = idx.files.len();
            for kx in (head.x / 1000.0).floor() as i64..=((x1 - 1e-6) / 1000.0).floor() as i64 {
                for ky in (head.y / 1000.0).floor() as i64..=((y1 - 1e-6) / 1000.0).floor() as i64 {
                    idx.by_km.entry((kx, ky)).or_default().push(i);
                }
            }
            idx.files.push(Entry { path, head });
        }
        if idx.files.is_empty() {
            return Err("no .asc height files were found in that folder".into());
        }
        Ok(idx)
    }

    pub fn files(&self) -> usize {
        self.files.len()
    }

    /// The file that covers a point (metres, UTM 30N).
    fn covering(&self, x: f64, y: f64) -> Option<usize> {
        let list = self.by_km.get(&((x / 1000.0).floor() as i64, (y / 1000.0).floor() as i64))?;
        list.iter().copied().find(|&i| {
            self.files.get(i).is_some_and(|e| {
                x >= e.head.x && y >= e.head.y && x < e.head.x + e.head.ncols as f64 * e.head.cell && y < e.head.y + e.head.nrows as f64 * e.head.cell
            })
        })
    }
}

/// One file averaged down to cells of `step` metres (whole multiples of its own cell).
struct Reduced {
    x: f64,
    /// The top edge.
    y_top: f64,
    cell: f64,
    cols: usize,
    rows: usize,
    heights: Vec<f32>,
}

fn reduce(e: &Entry, step: f64) -> Option<Reduced> {
    let h = &e.head;
    let k = ((step / h.cell).round() as usize).max(1);
    let (cols, rows) = (h.ncols.div_ceil(k), h.nrows.div_ceil(k));
    let mut sum = vec![0.0f64; cols * rows];
    let mut count = vec![0u32; cols * rows];
    let mut lines = BufReader::new(std::fs::File::open(&e.path).ok()?).lines();
    for _ in 0..6 {
        lines.next()?.ok()?;
    }
    for row in 0..h.nrows {
        let line = lines.next()?.ok()?;
        let out_row = row / k;
        for (col, tok) in line.split_ascii_whitespace().take(h.ncols).enumerate() {
            let Ok(v) = tok.replace(',', ".").parse::<f64>() else { continue };
            if !v.is_finite() || (v - h.nodata).abs() < 1e-6 {
                continue;
            }
            let at = out_row * cols + col / k;
            if let (Some(s), Some(c)) = (sum.get_mut(at), count.get_mut(at)) {
                *s += v;
                *c += 1;
            }
        }
    }
    // a cell with nothing in it (sea, or outside the survey) is level with the sea
    let heights = sum.iter().zip(&count).map(|(s, c)| if *c > 0 { (s / f64::from(*c)).max(0.0) as f32 } else { 0.0 }).collect();
    Some(Reduced { x: h.x, y_top: h.y + h.nrows as f64 * h.cell, cell: h.cell * k as f64, cols, rows, heights })
}

impl Reduced {
    /// The height at a point inside the file, interpolated between cell centres (clamped at the file's edges).
    fn at(&self, x: f64, y: f64) -> f32 {
        let fx = ((x - self.x) / self.cell - 0.5).clamp(0.0, (self.cols - 1) as f64);
        let fy = ((self.y_top - y) / self.cell - 0.5).clamp(0.0, (self.rows - 1) as f64);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.cols - 1), (y0 + 1).min(self.rows - 1));
        let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
        let g = |c: usize, r: usize| self.heights.get(r * self.cols + c).copied().unwrap_or(0.0);
        let top = g(x0, y0) * (1.0 - tx) + g(x1, y0) * tx;
        let bottom = g(x0, y1) * (1.0 - tx) + g(x1, y1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

impl Index {
    /// The ground within `radius_m` of the place as a [`Dem`] of `step_m` cells, or `Ok(None)` when the folder does not
    /// cover all of that area.
    pub fn dem_around(&self, lat: f64, lon: f64, radius_m: f64, step_m: f64) -> Result<Option<Dem>, String> {
        if !(lat.is_finite() && lon.is_finite() && radius_m.is_finite() && radius_m > 0.0 && step_m.is_finite() && step_m >= 1.0 && lat.abs() < 85.0)
        {
            return Ok(None);
        }
        let side = (2.0 * radius_m / step_m).ceil() as usize;
        if side == 0 || side > 4000 {
            return Ok(None);
        }
        let scale = EQUATOR_M * lat.to_radians().cos();
        let delta = step_m / scale;
        let (cx, cy) = map_tiles::project(lat, lon);
        let half = delta * side as f64 / 2.0;
        let (nw, se) = ((cx - half, cy - half), (cx + half, cy + half));
        // the kilometre squares the area touches: all must be there
        let corners = [(nw.0, nw.1), (se.0, nw.1), (nw.0, se.1), (se.0, se.1)].map(|(x, y)| {
            let (la, lo) = map_tiles::unproject(x, y);
            to_utm(la, lo)
        });
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for (e, n) in corners {
            x0 = x0.min(e);
            x1 = x1.max(e);
            y0 = y0.min(n);
            y1 = y1.max(n);
        }
        let mut needed: Vec<usize> = Vec::new();
        for kx in (x0 / 1000.0).floor() as i64..=(x1 / 1000.0).floor() as i64 {
            for ky in (y0 / 1000.0).floor() as i64..=(y1 / 1000.0).floor() as i64 {
                let Some(i) = self.covering(kx as f64 * 1000.0 + 500.0, ky as f64 * 1000.0 + 500.0) else { return Ok(None) };
                if !needed.contains(&i) {
                    needed.push(i);
                }
            }
        }
        if needed.len() > 400 {
            return Ok(None);
        }
        let mut reduced: HashMap<usize, Reduced> = HashMap::new();
        for i in needed {
            let e = self.files.get(i).ok_or("the folder changed")?;
            reduced.insert(i, reduce(e, step_m).ok_or_else(|| format!("could not read {}", e.path.display()))?);
        }
        let mut heights = Vec::with_capacity(side * side);
        for j in 0..side {
            for i in 0..side {
                let (la, lo) = map_tiles::unproject(nw.0 + (i as f64 + 0.5) * delta, nw.1 + (j as f64 + 0.5) * delta);
                let (e, n) = to_utm(la, lo);
                let h = self.covering(e, n).and_then(|f| reduced.get(&f)).map_or(0.0, |r| r.at(e, n));
                heights.push(h);
            }
        }
        Ok(Some(Dem { width: side, height: side, heights, nw, se, m_per_px: step_m }))
    }
}

// ------------------------------------------------------------------------------------------- the folder in use

static FOLDER: Mutex<Option<(String, Result<Index, String>)>> = Mutex::new(None);

/// Sets the folder of height files (empty: none). It is scanned here.
pub fn set_folder(path: &str) {
    let path = path.trim();
    let mut g = FOLDER.lock().unwrap_or_else(PoisonError::into_inner);
    if g.as_ref().is_some_and(|(p, _)| p == path) || (path.is_empty() && g.is_none()) {
        return;
    }
    *g = (!path.is_empty()).then(|| (path.to_string(), Index::scan(Path::new(path))));
}

/// `None` without a folder; else how many files it has, or why it cannot be used.
pub fn status() -> Option<Result<usize, String>> {
    FOLDER.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(|(_, r)| r.as_ref().map(Index::files).map_err(Clone::clone))
}

/// The fine ground round the place when the folder covers it.
pub fn dem(lat: f64, lon: f64) -> Result<Option<Dem>, String> {
    match FOLDER.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        Some((_, Ok(index))) => index.dem_around(lat, lon, RADIUS_M, STEP_M),
        _ => Ok(None),
    }
}

/// A folder of height files is set and readable.
pub fn in_use() -> bool {
    matches!(status(), Some(Ok(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utm_agrees_with_known_values_and_round_trips() {
        // on the central meridian of zone 30, 40 N: the northing is 4 427 757.2 m (the standard value)
        let (e, n) = to_utm(40.0, -3.0);
        assert!((e - 500_000.0).abs() < 1e-6, "{e}");
        assert!((n - 4_427_757.218).abs() < 0.5, "{n}");
        assert!(to_utm(0.0, -3.0).1.abs() < 1e-6);
        for (lat, lon) in [(43.3, -2.0), (43.0, -2.6), (43.6, -1.4), (36.0, -5.0), (42.0, 0.0)] {
            let (e, n) = to_utm(lat, lon);
            let (la, lo) = from_utm(e, n);
            assert!((la - lat).abs() < 1e-9 && (lo - lon).abs() < 1e-9, "{lat},{lon} -> {la},{lo}");
        }
    }

    /// Writes a box of `side` cells of 1 m whose height is `f(easting, northing)`.
    fn write_box(dir: &Path, name: &str, x: f64, y: f64, side: usize, f: impl Fn(f64, f64) -> f64) {
        let mut s = format!("ncols {side}\nnrows {side}\nxllcorner {x}\nyllcorner {y}\ncellsize 1\nNODATA_value -9999\n");
        for r in 0..side {
            let n = y + (side - r) as f64 - 0.5;
            let row: Vec<String> = (0..side).map(|c| format!("{:.2}", f(x + c as f64 + 0.5, n))).collect();
            s.push_str(&row.join(" "));
            s.push('\n');
        }
        std::fs::write(dir.join(name), s).unwrap();
    }

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("light-studio-fine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_covered_place_gets_a_fine_grid_that_follows_the_ground() {
        let (lat, lon) = (43.2, -2.0);
        let (e, n) = to_utm(lat, lon);
        let (kx, ky) = ((e / 1000.0).floor() as i64, (n / 1000.0).floor() as i64);
        let dir = folder("cover");
        // a slope that rises 0.1 m for every metre east, over the 5 × 5 km round the place
        for i in -2..=2 {
            for j in -2..=2 {
                let (x, y) = (((kx + i) * 1000) as f64, ((ky + j) * 1000) as f64);
                write_box(&dir, &format!("b_{i}_{j}.asc"), x, y, 1000, |ee, _| 100.0 + (ee - e) * 0.1);
            }
        }
        let idx = Index::scan(&dir).unwrap();
        assert_eq!(idx.files(), 25);
        let dem = idx.dem_around(lat, lon, 1500.0, 4.0).unwrap().expect("covered");
        assert_eq!((dem.width, dem.height), (750, 750));
        assert_eq!(dem.m_per_px, 4.0);
        // the middle is at the place: about 100 m; 1 km to the east it is 100 m higher
        let mid = dem.heights[375 * 750 + 375];
        let east = dem.heights[375 * 750 + 375 + 250];
        assert!((mid - 100.0).abs() < 1.5, "{mid}");
        assert!((east - mid - 100.0).abs() < 3.0, "{east} {mid}");
        // north-up: the grid's corners surround the place
        let (wx, wy) = map_tiles::project(lat, lon);
        assert!(dem.nw.0 < wx && wx < dem.se.0 && dem.nw.1 < wy && wy < dem.se.1);
    }

    #[test]
    fn a_gap_in_the_boxes_means_the_global_terrain_is_used() {
        let (lat, lon) = (43.2, -2.0);
        let (e, n) = to_utm(lat, lon);
        let (kx, ky) = ((e / 1000.0).floor() as i64, (n / 1000.0).floor() as i64);
        let dir = folder("gap");
        write_box(&dir, "only.asc", (kx * 1000) as f64, (ky * 1000) as f64, 1000, |_, _| 5.0);
        let idx = Index::scan(&dir).unwrap();
        assert!(idx.dem_around(lat, lon, 1500.0, 4.0).unwrap().is_none());
        assert!(idx.dem_around(f64::NAN, lon, 1500.0, 4.0).unwrap().is_none());
        assert!(idx.dem_around(lat, lon, -1.0, 4.0).unwrap().is_none());
    }

    #[test]
    fn nodata_is_the_sea_and_bad_files_are_skipped() {
        let dir = folder("bad");
        std::fs::write(dir.join("junk.asc"), "hello\nworld\n").unwrap();
        std::fs::write(dir.join("short.asc"), "ncols 3\nnrows 3\nxllcorner 0\nyllcorner 0\ncellsize 1\nNODATA_value -9999\n1 2 3\n").unwrap();
        std::fs::write(dir.join("huge.asc"), "ncols 999999\nnrows 999999\nxllcorner 0\nyllcorner 0\ncellsize 1\nNODATA_value -9999\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a grid").unwrap();
        let idx = Index::scan(&dir).unwrap();
        assert_eq!(idx.files(), 1, "only the short one has a header that reads");
        let r = reduce(&idx.files[0], 1.0);
        assert!(r.is_none(), "its rows are missing: an error, not a panic");
        assert!(Index::scan(&folder("empty")).is_err());
        assert!(Index::scan(Path::new("/nonexistent/folder")).is_err());
    }

    #[test]
    fn the_folder_setting_reports_its_state() {
        set_folder("/nonexistent/folder");
        assert!(matches!(status(), Some(Err(_))));
        assert!(!in_use());
        assert!(matches!(dem(43.2, -2.0), Ok(None)));
        set_folder("");
        assert!(status().is_none());
    }
}
