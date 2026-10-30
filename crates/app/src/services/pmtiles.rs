//! A reader for PMTiles version 3 archives holding raster tiles (PNG, JPEG or WebP): one local file with a whole region's
//! map, so the map works with no network. Vector archives (MVT) are recognised and refused, not drawn.
//!
//! Format: the PMTiles v3 specification (public, CC0): a 127-byte header, directories of (tile id, offset, length, run)
//! entries, and the tile data. Tile ids walk the Hilbert curve level after level. Every number read from the file is
//! checked: lengths are capped, offsets must lie inside the file, directory depth is bounded.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Mutex, PoisonError};

const HEADER: usize = 127;
/// A directory (decompressed) larger than this is refused.
const MAX_DIR_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TILE_BYTES: u64 = 8 * 1024 * 1024;
/// Leaf directories kept in memory.
const LEAF_CACHE: usize = 64;
/// Directory levels followed (the format allows root and one leaf level; a little more is tolerated).
const MAX_DEPTH: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compression {
    None,
    Gzip,
}

/// What the archive holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileType {
    Png,
    Jpeg,
    Webp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    id: u64,
    offset: u64,
    length: u64,
    run: u64,
}

/// An open archive.
#[derive(Debug)]
pub struct Archive {
    file: Mutex<File>,
    len: u64,
    root: Vec<Entry>,
    leaf_offset: u64,
    leaf_length: u64,
    data_offset: u64,
    data_length: u64,
    internal: Compression,
    tile: Compression,
    pub tile_type: TileType,
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// West, south, east, north in degrees.
    pub bounds: [f64; 4],
    /// The credit the archive asks for (its metadata's `attribution`, with markup removed).
    pub attribution: String,
    leaves: Mutex<HashMap<u64, Vec<Entry>>>,
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn compression(code: u8) -> Result<Compression, String> {
    match code {
        1 => Ok(Compression::None),
        2 => Ok(Compression::Gzip),
        3 => Err("this map file is compressed with brotli, which is not supported".into()),
        4 => Err("this map file is compressed with zstd, which is not supported".into()),
        _ => Err("this map file has an unknown compression".into()),
    }
}

/// The tile id of a tile: the tiles of the lower levels first, then the Hilbert curve index in its level.
pub fn tile_id(z: u8, x: u32, y: u32) -> Option<u64> {
    if z > 31 {
        return None;
    }
    let n = 1u64 << z;
    if u64::from(x) >= n || u64::from(y) >= n {
        return None;
    }
    let below = ((1u64 << (2 * u32::from(z))) - 1) / 3;
    let (mut x, mut y) = (u64::from(x), u64::from(y));
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u64::from(x & s > 0);
        let ry = u64::from(y & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = n - 1 - x;
                y = n - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    below.checked_add(d)
}

fn varint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..10 {
        let byte = *b.get(*at)?;
        *at += 1;
        v |= u64::from(byte & 0x7f).checked_shl(7 * i)?;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

fn parse_directory(b: &[u8]) -> Option<Vec<Entry>> {
    let mut at = 0;
    let n = usize::try_from(varint(b, &mut at)?).ok()?;
    // four numbers per entry, a byte each at least
    if n > b.len() {
        return None;
    }
    let mut e = vec![Entry { id: 0, offset: 0, length: 0, run: 0 }; n];
    let mut last = 0u64;
    for x in &mut e {
        last = last.checked_add(varint(b, &mut at)?)?;
        x.id = last;
    }
    for x in &mut e {
        x.run = varint(b, &mut at)?;
    }
    for x in &mut e {
        x.length = varint(b, &mut at)?;
    }
    for i in 0..n {
        let v = varint(b, &mut at)?;
        let prev = if i > 0 { e.get(i - 1).map(|p| p.offset.saturating_add(p.length)) } else { Some(0) }?;
        let off = if v == 0 && i > 0 { prev } else { v.checked_sub(1)? };
        e.get_mut(i)?.offset = off;
    }
    Some(e)
}

fn inflate(bytes: &[u8], c: Compression, cap: u64) -> Result<Vec<u8>, String> {
    match c {
        Compression::None => {
            if bytes.len() as u64 > cap {
                return Err("a part of the map file is too large".into());
            }
            Ok(bytes.to_vec())
        }
        Compression::Gzip => {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(bytes).take(cap + 1).read_to_end(&mut out).map_err(|_| "a part of the map file is damaged".to_string())?;
            if out.len() as u64 > cap {
                return Err("a part of the map file is too large".into());
            }
            Ok(out)
        }
    }
}

impl Archive {
    fn read_at(&self, offset: u64, length: u64, cap: u64) -> Result<Vec<u8>, String> {
        if length > cap || offset.checked_add(length).is_none_or(|end| end > self.len) {
            return Err("the map file points outside itself".into());
        }
        let mut f = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; usize::try_from(length).map_err(|_| "too large".to_string())?];
        f.read_exact(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }

    /// Opens an archive.
    pub fn open(path: &Path) -> Result<Archive, String> {
        let mut file = File::open(path).map_err(|e| format!("could not open the map file: {e}"))?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut h = vec![0u8; HEADER];
        file.read_exact(&mut h).map_err(|_| "this is not a PMTiles map file".to_string())?;
        if h.get(0..7) != Some(b"PMTiles") || h.get(7) != Some(&3) {
            return Err("this is not a PMTiles version 3 map file".into());
        }
        let g = |at| u64_at(&h, at).ok_or_else(|| "the map file header is damaged".to_string());
        let (root_off, root_len, meta_off, meta_len, leaf_off, leaf_len, data_off, data_len) =
            (g(8)?, g(16)?, g(24)?, g(32)?, g(40)?, g(48)?, g(56)?, g(64)?);
        let byte = |at: usize| h.get(at).copied().ok_or_else(|| "the map file header is damaged".to_string());
        let internal = compression(byte(97)?)?;
        let tile = match byte(98)? {
            1 => Compression::None,
            2 => Compression::Gzip,
            _ => return Err("this map file's tiles use a compression that is not supported".into()),
        };
        let tile_type = match byte(99)? {
            1 => return Err("this map file holds vector tiles; Light Studio draws raster maps (PNG, JPEG or WebP)".into()),
            2 => TileType::Png,
            3 => TileType::Jpeg,
            4 => TileType::Webp,
            5 => return Err("this map file holds AVIF tiles, which are not supported".into()),
            _ => return Err("this map file holds an unknown kind of tile".into()),
        };
        let (min_zoom, max_zoom) = (byte(100)?, byte(101)?);
        let deg = |at| i32_at(&h, at).map(|v| f64::from(v) / 1e7).ok_or_else(|| "the map file header is damaged".to_string());
        let bounds = [deg(102)?, deg(106)?, deg(110)?, deg(114)?];
        let mut a = Archive {
            file: Mutex::new(file),
            len,
            root: Vec::new(),
            leaf_offset: leaf_off,
            leaf_length: leaf_len,
            data_offset: data_off,
            data_length: data_len,
            internal,
            tile,
            tile_type,
            min_zoom,
            max_zoom,
            bounds,
            attribution: String::new(),
            leaves: Mutex::new(HashMap::new()),
        };
        if data_off.checked_add(data_len).is_none_or(|e| e > len) {
            return Err("the map file is shorter than its header says (an incomplete download?)".into());
        }
        let root = inflate(&a.read_at(root_off, root_len, MAX_DIR_BYTES)?, internal, MAX_DIR_BYTES)?;
        a.root = parse_directory(&root).ok_or("the map file's index is damaged")?;
        if let Ok(meta) = a.read_at(meta_off, meta_len, 1024 * 1024).and_then(|b| inflate(&b, internal, 1024 * 1024))
            && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&meta)
            && let Some(text) = v.get("attribution").and_then(|x| x.as_str())
        {
            a.attribution = strip_markup(text);
        }
        Ok(a)
    }

    /// The bytes of a tile (decompressed), `Ok(None)` when the archive has no such tile.
    pub fn tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, String> {
        if z < self.min_zoom || z > self.max_zoom {
            return Ok(None);
        }
        let Some(id) = tile_id(z, x, y) else { return Ok(None) };
        let mut dir: Vec<Entry> = self.root.clone();
        for _ in 0..MAX_DEPTH {
            let i = dir.partition_point(|e| e.id <= id);
            let Some(e) = i.checked_sub(1).and_then(|i| dir.get(i)).copied() else { return Ok(None) };
            if e.run == 0 {
                // a leaf directory
                if e.offset.checked_add(e.length).is_none_or(|end| end > self.leaf_length) {
                    return Err("the map file's index points outside itself".into());
                }
                dir = self.leaf(e)?;
                continue;
            }
            if id >= e.id.saturating_add(e.run) {
                return Ok(None);
            }
            if e.offset.checked_add(e.length).is_none_or(|end| end > self.data_length) {
                return Err("the map file points outside its tile data".into());
            }
            let raw = self.read_at(self.data_offset + e.offset, e.length, MAX_TILE_BYTES)?;
            return inflate(&raw, self.tile, MAX_TILE_BYTES).map(Some);
        }
        Err("the map file's index is nested too deep".into())
    }

    fn leaf(&self, e: Entry) -> Result<Vec<Entry>, String> {
        if let Some(d) = self.leaves.lock().unwrap_or_else(PoisonError::into_inner).get(&e.offset) {
            return Ok(d.clone());
        }
        let raw = self.read_at(self.leaf_offset + e.offset, e.length, MAX_DIR_BYTES)?;
        let dir = parse_directory(&inflate(&raw, self.internal, MAX_DIR_BYTES)?).ok_or("the map file's index is damaged")?;
        let mut cache = self.leaves.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= LEAF_CACHE {
            cache.clear();
        }
        cache.insert(e.offset, dir.clone());
        Ok(dir)
    }

    /// A one-line description for Settings.
    pub fn describe(&self) -> String {
        format!(
            "zoom {}–{}, {:.2}°…{:.2}° E, {:.2}°…{:.2}° N",
            self.min_zoom, self.max_zoom, self.bounds[0], self.bounds[2], self.bounds[1], self.bounds[3]
        )
    }
}

/// Markup (`<a href…>`) removed from a credit line, entities left alone.
fn strip_markup(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    fn put_varint(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }

    /// A directory from (id, run, length) entries laid out one after the other from `base`.
    fn directory(entries: &[(u64, u64, u64)], base: u64) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, entries.len() as u64);
        let mut last = 0;
        for e in entries {
            put_varint(&mut out, e.0 - last);
            last = e.0;
        }
        for e in entries {
            put_varint(&mut out, e.1);
        }
        for e in entries {
            put_varint(&mut out, e.2);
        }
        let mut off = base;
        for e in entries {
            put_varint(&mut out, off + 1);
            off += e.2;
        }
        out
    }

    fn gz(b: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(b).unwrap();
        e.finish().unwrap()
    }

    /// A one-tile-per-id archive: tiles `(z, x, y, bytes)`; `leaves` puts the entries into a leaf directory under a root
    /// that points to it, and `gzip` compresses the directories.
    pub(crate) fn build(tiles: &[(u8, u32, u32, Vec<u8>)], leaves: bool, gzip: bool, kind: u8) -> Vec<u8> {
        let mut t: Vec<(u64, &Vec<u8>)> = tiles.iter().map(|(z, x, y, b)| (tile_id(*z, *x, *y).unwrap(), b)).collect();
        t.sort_by_key(|x| x.0);
        let mut data = Vec::new();
        let mut entries = Vec::new();
        for (id, b) in &t {
            entries.push((*id, 1, b.len() as u64));
            data.extend_from_slice(b);
        }
        let pack = |d: Vec<u8>| if gzip { gz(&d) } else { d };
        let (root, leaf) = if leaves {
            let leaf = pack(directory(&entries, 0));
            let root = pack(directory(&[(entries[0].0, 0, leaf.len() as u64)], 0));
            (root, leaf)
        } else {
            (pack(directory(&entries, 0)), Vec::new())
        };
        let meta = pack(br#"{"attribution":"<a href=\"https://example.org\">Example</a> maps"}"#.to_vec());
        let root_off = HEADER as u64;
        let meta_off = root_off + root.len() as u64;
        let leaf_off = meta_off + meta.len() as u64;
        let data_off = leaf_off + leaf.len() as u64;
        let mut h = Vec::new();
        h.extend_from_slice(b"PMTiles");
        h.push(3);
        for v in [root_off, root.len() as u64, meta_off, meta.len() as u64, leaf_off, leaf.len() as u64, data_off, data.len() as u64, 0, 0, 0] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        h.extend_from_slice(&[1, if gzip { 2 } else { 1 }, 1, kind, 0, 4]);
        for v in [-30_000_000i32, 400_000_000, 30_000_000, 450_000_000] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        h.push(2);
        h.extend_from_slice(&0i32.to_le_bytes());
        h.extend_from_slice(&0i32.to_le_bytes());
        assert_eq!(h.len(), HEADER);
        [h, root, meta, leaf, data].concat()
    }

    fn write(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("light-studio-pmtiles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn tile_ids_follow_the_hilbert_curve_as_the_format_numbers_them() {
        assert_eq!(tile_id(0, 0, 0), Some(0));
        // level 1: (0,0) (0,1) (1,1) (1,0)
        assert_eq!([(0, 0), (0, 1), (1, 1), (1, 0)].map(|(x, y)| tile_id(1, x, y).unwrap()), [1, 2, 3, 4]);
        // level 2: the curve's order of the sixteen tiles
        let order = [(0, 0), (1, 0), (1, 1), (0, 1), (0, 2), (0, 3), (1, 3), (1, 2), (2, 2), (2, 3), (3, 3), (3, 2), (3, 1), (2, 1), (2, 0), (3, 0)];
        for (i, (x, y)) in order.into_iter().enumerate() {
            assert_eq!(tile_id(2, x, y), Some(5 + i as u64), "({x},{y})");
        }
        assert_eq!(tile_id(2, 4, 0), None);
        assert_eq!(tile_id(40, 0, 0), None);
    }

    #[test]
    fn reads_tiles_from_a_flat_archive() {
        let tiles = vec![(3, 4, 2, b"aaa".to_vec()), (3, 5, 2, b"bbbb".to_vec()), (4, 8, 4, b"cc".to_vec())];
        let p = write("flat.pmtiles", &build(&tiles, false, false, 2));
        let a = Archive::open(&p).unwrap();
        assert_eq!((a.min_zoom, a.max_zoom, a.tile_type), (0, 4, TileType::Png));
        assert_eq!(a.attribution, "Example maps");
        assert_eq!(a.tile(3, 4, 2).unwrap().unwrap(), b"aaa");
        assert_eq!(a.tile(3, 5, 2).unwrap().unwrap(), b"bbbb");
        assert_eq!(a.tile(4, 8, 4).unwrap().unwrap(), b"cc");
        assert_eq!(a.tile(3, 6, 2).unwrap(), None, "a tile the archive does not have");
        assert_eq!(a.tile(9, 0, 0).unwrap(), None, "deeper than the archive");
    }

    #[test]
    fn reads_tiles_through_a_gzipped_leaf_directory() {
        let tiles = vec![(2, 1, 1, b"one".to_vec()), (2, 2, 3, b"two".to_vec()), (3, 0, 7, b"three".to_vec())];
        let p = write("leaf.pmtiles", &build(&tiles, true, true, 3));
        let a = Archive::open(&p).unwrap();
        assert_eq!(a.tile_type, TileType::Jpeg);
        for (z, x, y, b) in &tiles {
            assert_eq!(&a.tile(*z, *x, *y).unwrap().unwrap(), b);
        }
        assert_eq!(a.tile(2, 0, 0).unwrap(), None);
    }

    #[test]
    fn refuses_what_it_cannot_draw_or_read() {
        let tiles = vec![(1, 0, 0, b"x".to_vec())];
        let vector = write("vector.pmtiles", &build(&tiles, false, false, 1));
        assert!(Archive::open(&vector).unwrap_err().contains("vector"));
        assert!(Archive::open(&write("empty.pmtiles", b"")).is_err());
        assert!(Archive::open(&write("text.pmtiles", &[b'x'; 300])).is_err());
        assert!(Archive::open(Path::new("/nonexistent/none.pmtiles")).is_err());
        // cut short: the header says more than the file has
        let mut cut = build(&tiles, false, false, 2);
        cut.truncate(cut.len() - 1);
        assert!(Archive::open(&write("cut.pmtiles", &cut)).unwrap_err().contains("shorter"));
    }

    #[test]
    fn hostile_numbers_are_errors_not_panics() {
        let tiles = vec![(1, 0, 0, b"x".to_vec()), (1, 1, 1, b"y".to_vec())];
        let good = build(&tiles, false, false, 2);
        // every single byte of the header and the index set to garbage in turn
        for i in 0..good.len().min(HEADER + 40) {
            for v in [0u8, 0xff, 0x80] {
                let mut b = good.clone();
                b[i] = v;
                if let Ok(a) = Archive::open(&write("fuzz.pmtiles", &b)) {
                    for (z, x, y) in [(0, 0, 0), (1, 0, 0), (1, 1, 1), (3, 2, 2)] {
                        let _ = a.tile(z, x, y);
                    }
                }
            }
        }
        assert!(parse_directory(&[0xff; 12]).is_none());
        assert!(parse_directory(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]).is_none());
    }

    #[test]
    fn credits_lose_their_markup() {
        assert_eq!(strip_markup("<a href=\"x\">© Foo</a>  | <b>Bar</b>"), "© Foo | Bar");
    }
}
