//! Points of interest as data: a stable id, times and a version for each, and the shared format (version 1) that
//! the PDI web app on the phone writes and reads.
//!
//! The id, not the place's position in the list, says which point is which: opening a record, merging points that
//! came from the phone and importing the same file twice all go by it. Times are UTC (`2026-10-23T08:30:00Z`).

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use web_time::{SystemTime, UNIX_EPOCH};

use super::places::SiteStatus;
use super::{SavedPlace, places};

/// The format's version, written in every file.
pub(super) const SCHEMA_VERSION: u32 = 1;
/// A file of points is read up to this size.
pub(super) const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_TAGS: usize = 20;
const MAX_ATTACHMENTS: usize = 20;
const MAX_ID: usize = 64;

/// A photo kept in the cloud for a point (the phone's), carried through untouched.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Attachment {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub bytes: u64,
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| i64::try_from(d.as_secs()).unwrap_or(0)).unwrap_or(0)
}

/// `unix` seconds as `YYYY-MM-DDTHH:MM:SSZ`.
pub(super) fn iso_utc(unix: i64) -> String {
    let (days, rem) = (unix.div_euclid(86_400), unix.rem_euclid(86_400));
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub(super) fn now_iso() -> String {
    iso_utc(now_secs())
}

/// A fresh version-4 UUID. Random keys from the system, the clock and a counter are mixed, so two ids never repeat,
/// also on a platform whose hasher keys are fixed.
pub(super) fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut bytes = [0u8; 16];
    for (half, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(nanos);
        hasher.write_u64(n);
        hasher.write_usize(half);
        let word = hasher.finish().to_be_bytes();
        chunk.copy_from_slice(&word);
    }
    if let Some(b) = bytes.get_mut(6) {
        *b = (*b & 0x0f) | 0x40;
    }
    if let Some(b) = bytes.get_mut(8) {
        *b = (*b & 0x3f) | 0x80;
    }
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

impl SavedPlace {
    /// Gives a point what a record needs and may lack (points saved before ids existed): an id, its times, a version
    /// and its origin.
    pub(super) fn ensure_identity(&mut self) {
        if self.id.trim().is_empty() {
            self.id = new_id();
            // a point that had no id is new to the Worker
            self.dirty = true;
        }
        if self.created_at.is_empty() {
            self.created_at = now_iso();
        }
        if self.updated_at.is_empty() {
            self.updated_at = self.created_at.clone();
        }
        if self.version == 0 {
            self.version = 1;
        }
        if self.origin.is_empty() {
            self.origin = "desktop".into();
        }
    }

    /// Notes that the point was edited here: a new version, now.
    pub fn touch(&mut self) {
        self.dirty = true;
        self.version = self.version.saturating_add(1);
        self.updated_at = now_iso();
    }

    /// The same point apart from its version and edit time (to tell whether an edit changed anything).
    pub(super) fn same_content(&self, other: &SavedPlace) -> bool {
        // (the notebook is not part of the record the phone and the Worker know: writing in it is no new version)
        let strip = |p: &SavedPlace| SavedPlace {
            version: 0,
            updated_at: String::new(),
            base: 0,
            dirty: false,
            conflict: false,
            fresh: false,
            notebook: Default::default(),
            ..p.clone()
        };
        strip(self) == strip(other)
    }

    /// Keeps every field within what the app and the format allow (also for points read from a file).
    pub(super) fn tidy(&mut self) {
        let cut = |s: &mut String, n: usize| {
            if s.chars().count() > n {
                *s = s.chars().take(n).collect();
            }
        };
        cut(&mut self.id, MAX_ID);
        cut(&mut self.name, 80);
        cut(&mut self.best, 200);
        cut(&mut self.notes, places::MAX_NOTES);
        cut(&mut self.description, places::MAX_DESCRIPTION);
        cut(&mut self.category, 40);
        cut(&mut self.origin, 20);
        cut(&mut self.created_at, 40);
        cut(&mut self.updated_at, 40);
        self.tags.retain(|t| !t.trim().is_empty());
        self.tags.truncate(MAX_TAGS);
        for t in &mut self.tags {
            cut(t, 40);
        }
        self.notebook.tidy();
        self.attachments.truncate(MAX_ATTACHMENTS);
        for a in &mut self.attachments {
            cut(&mut a.key, 300);
            cut(&mut a.kind, 60);
        }
    }
}

/// One point as the shared format writes it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Record {
    schema_version: u32,
    id: String,
    name: String,
    description: String,
    latitude: f64,
    longitude: f64,
    status: String,
    best_time: String,
    notes: String,
    category: String,
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
    version: u64,
    origin: String,
    attachments: Vec<Attachment>,
    deleted_at: Option<String>,
}

#[derive(Serialize)]
struct FileOut<'a> {
    schema_version: u32,
    points: &'a [Record],
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FileIn {
    Wrapped { points: Vec<serde_json::Value> },
    Bare(Vec<serde_json::Value>),
}

impl Record {
    fn of(p: &SavedPlace) -> Record {
        Record {
            schema_version: SCHEMA_VERSION,
            id: p.id.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            latitude: p.lat,
            longitude: p.lon,
            status: match p.status {
                SiteStatus::Want => "want",
                SiteStatus::Visited => "visited",
            }
            .into(),
            best_time: p.best.clone(),
            notes: p.notes.clone(),
            category: p.category.clone(),
            tags: p.tags.clone(),
            created_at: p.created_at.clone(),
            updated_at: p.updated_at.clone(),
            version: p.version,
            origin: p.origin.clone(),
            attachments: p.attachments.clone(),
            deleted_at: None,
        }
    }

    /// The record as a point.
    fn into_point(self) -> SavedPlace {
        let mut p = SavedPlace {
            id: self.id,
            name: self.name,
            lat: self.latitude,
            lon: self.longitude,
            status: if self.status.eq_ignore_ascii_case("visited") { SiteStatus::Visited } else { SiteStatus::Want },
            best: self.best_time,
            notes: self.notes,
            description: self.description,
            category: self.category,
            tags: self.tags,
            created_at: self.created_at,
            updated_at: self.updated_at,
            version: self.version,
            origin: self.origin,
            attachments: self.attachments,
            ..Default::default()
        };
        p.tidy();
        p
    }
}

/// The points as a file of the shared format.
pub(super) fn export_json(points: &[SavedPlace]) -> Result<String, String> {
    let records: Vec<Record> = points.iter().map(Record::of).collect();
    serde_json::to_string_pretty(&FileOut { schema_version: SCHEMA_VERSION, points: &records }).map_err(|e| e.to_string())
}

/// What an import did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Imported {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub rejected: usize,
}

/// Merges a file of the shared format into `saved`, by id: a point already there is updated only by a newer version,
/// a new one is added, one marked deleted is removed, and importing the same file
/// again changes nothing.
pub(super) fn import_json(saved: &mut Vec<SavedPlace>, text: &str, max: usize) -> Result<Imported, String> {
    if text.len() > MAX_FILE_BYTES {
        return Err("the file is too large to be a list of points of interest".into());
    }
    let records = match serde_json::from_str::<FileIn>(text).map_err(|e| format!("not a file of points of interest: {e}"))? {
        FileIn::Wrapped { points } => points,
        FileIn::Bare(points) => points,
    };
    let mut out = Imported::default();
    for record in records {
        // one bad record is rejected on its own, not the whole file
        let Ok(record) = serde_json::from_value::<Record>(record) else {
            out.rejected += 1;
            continue;
        };
        let valid_place =
            record.latitude.is_finite() && record.longitude.is_finite() && record.latitude.abs() <= 90.0 && record.longitude.abs() <= 180.0;
        let id = record.id.trim().to_string();
        if id.is_empty() || id.chars().count() > MAX_ID || id.chars().any(char::is_control) || !valid_place && record.deleted_at.is_none() {
            out.rejected += 1;
            continue;
        }
        let at = saved.iter().position(|p| p.id == id);
        if record.deleted_at.is_some() {
            match at {
                Some(i) => {
                    saved.remove(i);
                    out.removed += 1;
                }
                None => out.unchanged += 1,
            }
            continue;
        }
        let mut point = record.into_point();
        point.id = id;
        match at {
            Some(i) => {
                if let Some(old) = saved.get_mut(i) {
                    if point.version > old.version {
                        point.base = old.base;
                        point.dirty = true;
                        point.notebook = std::mem::take(&mut old.notebook);
                        *old = point;
                        old.ensure_identity();
                        out.updated += 1;
                    } else {
                        out.unchanged += 1;
                    }
                }
            }
            None if saved.len() < max => {
                point.ensure_identity();
                point.dirty = true;
                point.origin = if point.origin == "desktop" { "imported".into() } else { point.origin };
                saved.push(point);
                out.added += 1;
            }
            None => out.rejected += 1,
        }
    }
    Ok(out)
}

/// A point deleted here that the Worker still has.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gone {
    pub id: String,
    pub base: u64,
}

/// The record as one of the Worker's points (what it takes in `upsert`): the whole point, with the kinds of place and
/// origins it knows.
pub(super) fn worker_poi(p: &SavedPlace) -> serde_json::Value {
    let mut v = serde_json::to_value(Record::of(p)).unwrap_or(serde_json::Value::Null);
    if let Some(o) = v.as_object_mut() {
        // the Worker sets these itself
        for k in ["schema_version", "version", "updated_at", "deleted_at"] {
            o.remove(k);
        }
        let name = if p.name.trim().is_empty() { format!("{:.5}, {:.5}", p.lat, p.lon) } else { p.name.chars().take(120).collect() };
        o.insert("name".into(), name.into());
        let kind = p.category.trim().to_lowercase();
        let kind = if ["arbol", "costa", "cascada", "mirador"].contains(&kind.as_str()) { kind } else { "otro".into() };
        o.insert("category".into(), kind.into());
        o.insert("origin".into(), if p.origin == "mobile" { "mobile" } else { "desktop" }.into());
        o.insert("accuracy_m".into(), serde_json::Value::Null);
    }
    v
}

/// Merges the points the Worker sent into the list. A point the Worker has changed replaces ours only if ours has not
/// changed since it was last in step; one we have changed is left for the next push (which settles it); one in
/// conflict takes the Worker's version (ours is kept there as a copy). Repeating it changes nothing.
pub(super) fn merge_worker(saved: &mut Vec<SavedPlace>, records: &[serde_json::Value], max: usize, deleted_here: &[Gone]) -> Imported {
    let mut out = Imported::default();
    for value in records {
        let Ok(record) = serde_json::from_value::<Record>(value.clone()) else {
            out.rejected += 1;
            continue;
        };
        let valid_place =
            record.latitude.is_finite() && record.longitude.is_finite() && record.latitude.abs() <= 90.0 && record.longitude.abs() <= 180.0;
        let id = record.id.trim().to_string();
        if id.is_empty() || id.chars().count() > MAX_ID || id.chars().any(char::is_control) || !valid_place && record.deleted_at.is_none() {
            out.rejected += 1;
            continue;
        }
        // deleted here and not yet told to the Worker: it must not come back
        if deleted_here.iter().any(|g| g.id == id) {
            out.unchanged += 1;
            continue;
        }
        let at = saved.iter().position(|p| p.id == id);
        if record.deleted_at.is_some() {
            match at {
                Some(i) if !saved.get(i).is_some_and(|p| p.dirty && !p.conflict) => {
                    saved.remove(i);
                    out.removed += 1;
                }
                _ => out.unchanged += 1,
            }
            continue;
        }
        let mut point = record.into_point();
        point.id = id;
        point.base = point.version;
        point.dirty = false;
        point.conflict = false;
        match at {
            Some(i) => {
                let Some(old) = saved.get_mut(i) else { continue };
                let take = old.conflict || (!old.dirty && point.version > old.base);
                if take {
                    // the notebook is only here: a record from the Worker never carries it
                    point.notebook = std::mem::take(&mut old.notebook);
                    // a point not yet opened stays marked
                    point.fresh = old.fresh;
                    *old = point;
                    out.updated += 1;
                } else {
                    out.unchanged += 1;
                }
            }
            None if saved.len() < max => {
                point.fresh = true;
                saved.push(point);
                out.added += 1;
            }
            None => out.rejected += 1,
        }
    }
    out
}

/// Points saved before the Worker existed (or by an older version) have no record of what it knows: those that came from
/// the phone are in step, the rest are new to it and go up with the next push.
pub(super) fn prepare_sync(saved: &mut [SavedPlace]) {
    for p in saved {
        p.ensure_identity();
        if p.base == 0 && !p.dirty {
            if p.origin == "mobile" {
                p.base = p.version;
            } else {
                p.dirty = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(name: &str, lat: f64, lon: f64) -> SavedPlace {
        let mut p = SavedPlace { name: name.into(), lat, lon, ..Default::default() };
        p.ensure_identity();
        p
    }

    #[test]
    fn times_are_utc_and_ids_are_uuids_that_never_repeat() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_792_744_200), "2026-10-23T08:30:00Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_utc(-1), "1969-12-31T23:59:59Z");
        let ids: std::collections::HashSet<String> = (0..2000).map(|_| new_id()).collect();
        assert_eq!(ids.len(), 2000);
        let id = new_id();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.iter().map(|s| s.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12]);
        assert!(parts[2].starts_with('4') && "89ab".contains(&parts[3][..1]), "{id}");
    }

    fn with_notebook(mut p: SavedPlace) -> SavedPlace {
        p.notebook.gear = "trípode, ND1000".into();
        p.notebook.shots.push(super::super::notebook::Shot { title: "Luna sobre el faro".into(), ..Default::default() });
        p
    }

    #[test]
    fn writing_in_the_notebook_is_not_a_new_version_for_the_phone() {
        let before = point("Faro", 43.39, -1.79);
        let after = with_notebook(before.clone());
        assert!(after.same_content(&before), "the notebook is outside the shared record");
        let mut changed = before.clone();
        changed.notes = "x".into();
        assert!(!changed.same_content(&before));
    }

    #[test]
    fn a_sync_from_the_worker_never_wipes_the_notebook() {
        let mut mine = with_notebook(point("Faro", 43.39, -1.79));
        mine.base = mine.version;
        mine.dirty = false;
        // the Worker has a newer version of the same point (renamed on the phone)
        let mut theirs = point("Faro de Higer", 43.39, -1.79);
        theirs.id = mine.id.clone();
        theirs.version = mine.version + 1;
        let records = vec![serde_json::to_value(Record::of(&theirs)).expect("serialises")];
        let mut saved = vec![mine.clone()];
        let out = merge_worker(&mut saved, &records, 100, &[]);
        assert_eq!(out.updated, 1);
        assert_eq!(saved[0].name, "Faro de Higer");
        assert_eq!(saved[0].notebook, mine.notebook, "the notebook survives the update");
    }

    #[test]
    fn importing_a_newer_file_keeps_the_notebook() {
        let mine = with_notebook(point("Faro", 43.39, -1.79));
        let mut newer = point("Faro nuevo", 43.39, -1.79);
        newer.id = mine.id.clone();
        newer.version = mine.version + 1;
        let file = export_json(&[newer]).expect("exports");
        let mut saved = vec![mine.clone()];
        let out = import_json(&mut saved, &file, 100).expect("imports");
        assert_eq!(out.updated, 1);
        assert_eq!((saved[0].name.as_str(), &saved[0].notebook), ("Faro nuevo", &mine.notebook));
        // and what goes out to the phone has no notebook in it
        let exported = export_json(&saved).expect("exports");
        assert!(!exported.contains("trípode") && !exported.contains("notebook"));
        let worker = worker_poi(&saved[0]).to_string();
        assert!(!worker.contains("trípode"));
    }

    #[test]
    fn a_point_without_identity_gets_one_and_editing_bumps_its_version() {
        let mut p = SavedPlace { name: "Old".into(), ..Default::default() };
        assert!(p.id.is_empty());
        p.ensure_identity();
        let (id, created) = (p.id.clone(), p.created_at.clone());
        assert!(!id.is_empty() && p.version == 1 && p.origin == "desktop" && p.updated_at == created);
        p.ensure_identity();
        assert_eq!((p.id.clone(), p.created_at.clone()), (id, created), "kept");
        let before = p.clone();
        p.notes = "x".into();
        assert!(!p.same_content(&before));
        p.touch();
        assert_eq!(p.version, 2);
    }

    #[test]
    fn export_then_import_is_lossless_and_repeating_it_changes_nothing() {
        let mut a = point("Roble", 43.12345, -2.12345);
        a.status = SiteStatus::Visited;
        a.tags = vec!["árbol".into(), "paisaje".into()];
        a.best = "Atardecer".into();
        a.attachments = vec![Attachment { key: "pdi/1.jpg".into(), kind: "image/jpeg".into(), bytes: 1234 }];
        let text = export_json(&[a.clone(), point("Cala", 43.3, -2.0)]).unwrap();
        assert!(text.contains("\"schema_version\": 1") && text.contains("\"latitude\": 43.12345") && text.contains("\"status\": \"visited\""));
        let mut other = Vec::new();
        let r = import_json(&mut other, &text, 100).unwrap();
        assert_eq!((r.added, r.updated, r.unchanged), (2, 0, 0));
        assert_eq!(other[0].id, a.id);
        assert_eq!((other[0].status, other[0].tags.clone(), other[0].best.clone()), (SiteStatus::Visited, a.tags.clone(), "Atardecer".into()));
        assert_eq!(other[0].attachments, a.attachments);
        let again = import_json(&mut other, &text, 100).unwrap();
        assert_eq!((again.added, again.updated, again.unchanged), (0, 0, 2));
        assert_eq!(other.len(), 2, "no duplicates");
    }

    #[test]
    fn a_newer_version_updates_and_an_older_one_is_ignored() {
        let mut local = vec![point("Roble", 43.0, -2.0)];
        let id = local[0].id.clone();
        let file = |name: &str, version: u64| {
            format!("{{\"points\":[{{\"id\":\"{id}\",\"name\":\"{name}\",\"latitude\":43.0,\"longitude\":-2.0,\"version\":{version}}}]}}")
        };
        assert_eq!(import_json(&mut local, &file("Viejo", 1), 10).unwrap().unchanged, 1);
        assert_eq!(local[0].name, "Roble");
        let r = import_json(&mut local, &file("Nuevo", 3), 10).unwrap();
        assert_eq!(r.updated, 1);
        assert_eq!((local[0].name.as_str(), local[0].version), ("Nuevo", 3));
    }

    #[test]
    fn deleted_points_are_removed_and_bad_records_rejected_without_a_panic() {
        let mut local = vec![point("A", 1.0, 1.0), point("B", 2.0, 2.0)];
        let gone = local[0].id.clone();
        let text = format!(
            "[{{\"id\":\"{gone}\",\"deleted_at\":\"2026-10-23T08:30:00Z\"}},{{\"id\":\"\",\"latitude\":1,\"longitude\":1}},\
             {{\"id\":\"x\",\"latitude\":999,\"longitude\":1}},{{\"id\":\"y\",\"latitude\":null}},{{\"id\":\"never-seen\",\"deleted_at\":\"t\"}}]"
        );
        let r = import_json(&mut local, &text, 10).unwrap();
        assert_eq!((r.removed, r.rejected, r.unchanged, r.added), (1, 3, 1, 0));
        assert_eq!(local.len(), 1);
        assert!(import_json(&mut local, "not json", 10).is_err());
        assert!(import_json(&mut local, "{\"points\": 3}", 10).is_err());
        assert!(import_json(&mut local, &" ".repeat(MAX_FILE_BYTES + 1), 10).is_err());
    }

    #[test]
    fn an_import_stops_at_the_limit_and_says_how_many_it_could_not_take() {
        let mut local = vec![point("A", 1.0, 1.0)];
        let text = "[{\"id\":\"1\",\"latitude\":1,\"longitude\":1},{\"id\":\"2\",\"latitude\":2,\"longitude\":2}]";
        let r = import_json(&mut local, text, 2).unwrap();
        assert_eq!((r.added, r.rejected), (1, 1));
    }

    #[test]
    fn a_point_new_from_the_phone_is_marked_fresh_once_and_an_update_does_not_mark_it() {
        let rec = |version: u64, name: &str| serde_json::json!({"id":"p1","name":name,"latitude":43.3,"longitude":-1.9,"version":version});
        let mut saved = Vec::new();
        let r = merge_worker(&mut saved, &[rec(1, "Faro")], 10, &[]);
        assert_eq!((r.added, saved[0].fresh), (1, true));
        // opened: the mark goes, and a repeat of the same record does not bring it back
        saved[0].fresh = false;
        merge_worker(&mut saved, &[rec(1, "Faro")], 10, &[]);
        assert!(!saved[0].fresh);
        // an update of a point not yet opened keeps the mark; one already opened stays unmarked
        saved[0].fresh = true;
        let r = merge_worker(&mut saved, &[rec(2, "Faro nuevo")], 10, &[]);
        assert_eq!((r.updated, saved[0].fresh, saved[0].name.as_str()), (1, true, "Faro nuevo"));
        saved[0].fresh = false;
        merge_worker(&mut saved, &[rec(3, "Faro 3")], 10, &[]);
        assert!(!saved[0].fresh);
        // the mark alone is not a change of content
        let mut b = saved[0].clone();
        b.fresh = true;
        assert!(saved[0].same_content(&b));
        // settings from before the mark read as not fresh
        let old: SavedPlace = serde_json::from_str(r#"{"name":"x","lat":1,"lon":2}"#).unwrap();
        assert!(!old.fresh);
    }
}
