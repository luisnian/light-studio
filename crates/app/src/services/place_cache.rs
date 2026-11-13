//! What the Mountains view works out for a place (the ground heights all round it and the aerial photo colours of that
//! ground) kept on disk, so looking at the same place again opens at once instead of decoding hundreds of tiles.
//!
//! One file per kind, place and sampling: a small header (magic and the number of items) and the items, little endian.
//! A file that is short, long or from another version is ignored (and read again from the tiles), never trusted.
//! The folder keeps the newest few dozen files.

use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"LCP1";
/// Files kept in the folder.
const MAX_FILES: usize = 48;
/// The most items one file may hold (a sanity cap for what is read back).
const MAX_ITEMS: usize = 1_000_000;

fn dir() -> Option<PathBuf> {
    crate::paths::config_dir().map(|d| d.join("place-cache"))
}

/// The file of `kind` (`h` heights, `p` photo) for a place at `tag`'s sampling, the place rounded to about 11 m.
fn file_in(dir: &Path, kind: &str, tag: &str, lat: f64, lon: f64) -> Option<PathBuf> {
    if !(lat.is_finite() && lon.is_finite()) {
        return None;
    }
    let clean = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>();
    let (la, lo) = ((lat * 1e4).round() as i64, (lon * 1e4).round() as i64);
    Some(dir.join(format!("{}-{}-{la}_{lo}.bin", clean(kind), clean(tag))))
}

fn read_items(file: &Path, size: usize, count: usize) -> Option<Vec<u8>> {
    let bytes = std::fs::read(file).ok()?;
    let n = usize::try_from(u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?)).ok()?;
    let body = bytes.get(8..)?;
    (bytes.get(..4)? == MAGIC && n == count && count <= MAX_ITEMS && body.len() == count.checked_mul(size)?).then(|| body.to_vec())
}

fn write_items(dir: &Path, file: &Path, count: usize, body: &[u8]) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let (Ok(n), tmp) = (u32::try_from(count), file.with_extension("tmp")) else { return };
    let mut bytes = Vec::with_capacity(8 + body.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&n.to_le_bytes());
    bytes.extend_from_slice(body);
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, file).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    // the newest few dozen stay
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "bin"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort();
    let extra = files.len().saturating_sub(MAX_FILES);
    for (_, old) in files.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

fn load_heights_in(dir: &Path, tag: &str, lat: f64, lon: f64, count: usize) -> Option<Vec<f64>> {
    let body = read_items(&file_in(dir, "h", tag, lat, lon)?, 4, count)?;
    let out: Vec<f64> = body.chunks_exact(4).filter_map(|c| Some(f64::from(f32::from_le_bytes(c.try_into().ok()?)))).collect();
    (out.len() == count && out.iter().all(|h| h.is_finite())).then_some(out)
}

fn store_heights_in(dir: &Path, tag: &str, lat: f64, lon: f64, heights: &[f64]) {
    let Some(file) = file_in(dir, "h", tag, lat, lon) else { return };
    let body: Vec<u8> = heights.iter().flat_map(|h| (*h as f32).to_le_bytes()).collect();
    write_items(dir, &file, heights.len(), &body);
}

fn load_photo_in(dir: &Path, tag: &str, lat: f64, lon: f64, count: usize) -> Option<Vec<[u8; 4]>> {
    let body = read_items(&file_in(dir, "p", tag, lat, lon)?, 4, count)?;
    Some(body.chunks_exact(4).filter_map(|c| <[u8; 4]>::try_from(c).ok()).collect::<Vec<_>>()).filter(|v| v.len() == count)
}

fn store_photo_in(dir: &Path, tag: &str, lat: f64, lon: f64, photo: &[[u8; 4]]) {
    let Some(file) = file_in(dir, "p", tag, lat, lon) else { return };
    let body: Vec<u8> = photo.iter().flatten().copied().collect();
    write_items(dir, &file, photo.len(), &body);
}

/// The kept ground heights of a place (`count` of them, sampled as `tag` says), if there are.
pub fn load_heights(tag: &str, lat: f64, lon: f64, count: usize) -> Option<Vec<f64>> {
    load_heights_in(&dir()?, tag, lat, lon, count)
}

/// Keeps the ground heights of a place (best effort).
pub fn store_heights(tag: &str, lat: f64, lon: f64, heights: &[f64]) {
    if let Some(d) = dir() {
        store_heights_in(&d, tag, lat, lon, heights);
    }
}

/// The kept photo colours of a place, if there are.
pub fn load_photo(tag: &str, lat: f64, lon: f64, count: usize) -> Option<Vec<[u8; 4]>> {
    load_photo_in(&dir()?, tag, lat, lon, count)
}

/// Keeps the photo colours of a place (best effort).
pub fn store_photo(tag: &str, lat: f64, lon: f64, photo: &[[u8; 4]]) {
    if let Some(d) = dir() {
        store_photo_in(&d, tag, lat, lon, photo);
    }
}

/// Whether the heights of a place are kept (so a first, rough look is not needed).
pub fn has_heights(tag: &str, lat: f64, lon: f64, count: usize) -> bool {
    load_heights(tag, lat, lon, count).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ls-place-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn heights_and_photo_come_back_as_kept_and_only_for_their_own_place_and_sampling() {
        let d = scratch("roundtrip");
        let h: Vec<f64> = (0..50).map(|i| f64::from(i) * 3.5).collect();
        store_heights_in(&d, "a", 43.3, -1.98, &h);
        assert_eq!(load_heights_in(&d, "a", 43.3, -1.98, 50), Some(h.clone()));
        // a place a few metres off is the same place; another place, sampling or length is not
        assert!(load_heights_in(&d, "a", 43.300_001, -1.980_001, 50).is_some());
        assert!(load_heights_in(&d, "a", 43.4, -1.98, 50).is_none());
        assert!(load_heights_in(&d, "b", 43.3, -1.98, 50).is_none());
        assert!(load_heights_in(&d, "a", 43.3, -1.98, 49).is_none());
        let p: Vec<[u8; 4]> = (0..20u8).map(|i| [i, i + 1, i + 2, 255]).collect();
        store_photo_in(&d, "a", 43.3, -1.98, &p);
        assert_eq!(load_photo_in(&d, "a", 43.3, -1.98, 20), Some(p));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_damaged_file_is_ignored_and_the_folder_keeps_only_the_newest() {
        let d = scratch("damaged");
        store_heights_in(&d, "a", 10.0, 10.0, &[1.0, 2.0, 3.0]);
        let file = file_in(&d, "h", "a", 10.0, 10.0).expect("a file name");
        let bytes = std::fs::read(&file).expect("the file was written");
        std::fs::write(&file, bytes.get(..bytes.len() - 1).expect("long enough")).expect("write");
        assert!(load_heights_in(&d, "a", 10.0, 10.0, 3).is_none());
        std::fs::write(&file, b"nonsense").expect("write");
        assert!(load_heights_in(&d, "a", 10.0, 10.0, 3).is_none());
        for i in 0..(MAX_FILES + 8) {
            store_heights_in(&d, "a", 20.0 + i as f64, 5.0, &[1.0]);
        }
        let n = std::fs::read_dir(&d).expect("folder").flatten().count();
        assert!(n <= MAX_FILES + 1, "{n} files");
        // not a place on Earth: nothing is written or read
        store_heights_in(&d, "a", f64::NAN, 5.0, &[1.0]);
        assert!(load_heights_in(&d, "a", f64::NAN, 5.0, 1).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
