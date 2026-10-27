//! Points of interest shared with the phone (the PDI Worker). One synchronisation, when Light Studio opens, every few
//! minutes while it is open, from the map's cloud button, File ▸ Sync Points of Interest and Settings:
//!
//! 1. download what is new on the Worker (and the photos that have not come yet) and merge it into the map by id;
//! 2. send what changed here: new and edited points, and the ones deleted here;
//! 3. if anything was sent (or refused as a conflict), download again: the Worker's echo, the copies it keeps of
//!    conflicting changes, and the Worker's version of a point in conflict.
//!
//! The steps are `crate::services::pdi_sync`; each runs on a worker thread (an activity row) and its result is applied
//! here, on the UI thread, which then starts the next. Nothing is ever overwritten without a copy: a conflict keeps the
//! Worker's version on the map and ours as «… (conflicto)».

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};

use crate::App;
use crate::planner::{PushSummary, Sent};
use crate::services::pdi_sync::{self, Keys, OpResult, Pulled};

/// How often it looks again while the app stays open (seconds).
const EVERY_SECS: f64 = 600.0;

/// What a worker thread brings back.
enum Done {
    Pulled(Pulled),
    /// The results so far, and why the rest did not go.
    Pushed(Vec<OpResult>, Option<String>),
}

#[derive(Default)]
struct Shared {
    running: bool,
    done: Option<Result<Done, String>>,
}

/// Where a synchronisation is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Idle,
    Pulling,
    Pushing,
    /// The download that follows a push.
    Settling,
}

/// What this synchronisation did, for the message at the end.
#[derive(Default)]
struct Totals {
    added: u64,
    updated: u64,
    removed: u64,
    sent: usize,
    conflicts: usize,
    rejected: Vec<String>,
}

/// What a run needs to start its next step.
struct Run {
    keys: Arc<dyn Keys + Send + Sync>,
    base: String,
    photos: Option<std::path::PathBuf>,
    sent: Vec<Sent>,
    /// Download again from the start (a conflict happened).
    reload: bool,
    totals: Totals,
}

/// The synchronisation in flight or last finished.
#[derive(Default)]
pub struct Pdi {
    shared: Arc<Mutex<Shared>>,
    stage: Stage,
    run: Option<Run>,
    launched: bool,
    next_at: f64,
    announce: bool,
    /// How the last synchronisation ended, for Settings (a line of text).
    pub last: Option<String>,
    /// Whether the last synchronisation ended without an error (`None`: none has ended yet).
    pub last_ok: Option<bool>,
}

impl Pdi {
    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn running(&self) -> bool {
        self.stage != Stage::Idle
    }
}

/// Starts a synchronisation. `announce`: say how it went even when nothing changed or it failed.
pub fn start(app: &mut App, announce: bool) -> Result<Value, String> {
    start_with(app, announce, Arc::new(pdi_sync::Keychain), photo_dir())
}

/// Where the phone's photos are kept: a folder in the configuration folder.
pub fn photo_dir() -> Option<std::path::PathBuf> {
    crate::paths::config_dir().map(|d| d.join("pdi-photos"))
}

/// `start` with the place the key is read from chosen by the caller (the tests use a pretend keychain).
fn start_with(app: &mut App, announce: bool, keys: Arc<dyn Keys + Send + Sync>, photos: Option<std::path::PathBuf>) -> Result<Value, String> {
    if app.pdi.running() {
        return Err(crate::i18n::tr("Already looking for points of interest").to_string());
    }
    let base = pdi_sync::valid_url(&app.ui.settings.pdi.url).map_err(|e| crate::i18n::tr(&e).to_string())?;
    app.pdi.announce = announce;
    app.pdi.last = None;
    app.pdi.run = Some(Run { keys, base, photos, sent: Vec::new(), reload: false, totals: Totals::default() });
    pull(app, false)?;
    Ok(json!({"started": true}))
}

/// Saves the Worker's key in the keychain.
pub fn save_key(key: &str) -> Result<(), String> {
    pdi_sync::Keychain.set(key.trim()).map_err(|e| crate::i18n::tr(&e).to_string())
}

/// Runs `job` on a worker thread: the result lands in `shared.done`.
fn spawn(
    app: &mut App,
    _label: &'static str,
    job: impl FnOnce(&AtomicBool, &str, String) -> Result<Done, String> + Send + 'static,
) -> Result<(), String> {
    let Some(run) = &app.pdi.run else { return Err("no synchronisation".into()) };
    let keys = run.keys.clone();
    let base = run.base.clone();
    // never raised: a step is short and harmless, so it neither needs a cancel button nor holds up quitting
    let cancel = Arc::new(AtomicBool::new(false));
    let worker = app.pdi.shared.clone();
    app.pdi.lock().running = true;
    let spawned = std::thread::Builder::new().name("pdi-sync".into()).spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let key = keys.get()?.ok_or_else(|| "No key for the points-of-interest Worker yet: save it in Settings.".to_string())?;
            job(&cancel, &base, key)
        }))
        .unwrap_or_else(|_| Err("Looking for points of interest stopped unexpectedly".to_string()));
        let mut sh = worker.lock().unwrap_or_else(PoisonError::into_inner);
        sh.running = false;
        sh.done = Some(outcome);
    });
    spawned.map(|_| ()).map_err(|e| {
        app.pdi.lock().running = false;
        format!("could not start: {e}")
    })
}

/// The download step. `settle`: the one after a push (from the start when something was refused).
fn pull(app: &mut App, settle: bool) -> Result<(), String> {
    let (photos, reload) = match &app.pdi.run {
        Some(r) => (r.photos.clone(), r.reload),
        None => return Err("no synchronisation".into()),
    };
    let cursor = if reload || app.ui.planner.pdi_cursor.is_empty() { "0".to_string() } else { app.ui.planner.pdi_cursor.clone() };
    // the photos of the points already on the map that have not come yet are fetched too
    let known: Vec<String> = app
        .ui
        .planner
        .saved
        .iter()
        .flat_map(|p| p.attachments.iter())
        .filter(|a| a.kind.is_empty() || a.kind.starts_with("image/"))
        .map(|a| a.key.clone())
        .collect();
    app.pdi.stage = if settle { Stage::Settling } else { Stage::Pulling };
    let res = spawn(app, "Looking for points of interest from the phone", move |cancel, base, key| {
        let remote = pdi_sync::Worker { base: base.to_string(), key, cancel };
        let pulled = pdi_sync::pull(&remote, &cursor, cancel)?;
        if let Some(dir) = &photos {
            let mut wanted = known;
            wanted.extend(pdi_sync::photo_keys(&pulled.points));
            wanted.dedup();
            pdi_sync::fetch_photos(&remote, &pulled.prefix, &wanted, dir, cancel);
        }
        // how far this computer has read (for tidying up on the Worker; a failure changes nothing)
        pdi_sync::ack(&remote, &pulled.prefix, &pulled.cursor);
        Ok(Done::Pulled(pulled))
    });
    if res.is_err() {
        app.pdi.stage = Stage::Idle;
        app.pdi.run = None;
    }
    res
}

/// The sending step.
fn push(app: &mut App, ops: Vec<Value>, sent: Vec<Sent>) -> Result<(), String> {
    if let Some(r) = &mut app.pdi.run {
        r.sent = sent;
    }
    app.pdi.stage = Stage::Pushing;
    spawn(app, "Sending points of interest to the phone", move |cancel, base, key| {
        let remote = pdi_sync::Worker { base: base.to_string(), key, cancel };
        let prefix = pdi_sync::find_prefix(&remote)?;
        let (results, err) = pdi_sync::push(&remote, prefix, &ops, cancel);
        Ok(Done::Pushed(results, err))
    })
}

/// Ends the synchronisation: the message.
fn finish(app: &mut App, ctx: &egui::Context, error: Option<String>) {
    app.pdi.stage = Stage::Idle;
    let t = app.pdi.run.take().map(|r| r.totals).unwrap_or_default();
    let loud = app.pdi.announce;
    let tr = |s: &str| crate::i18n::tr(s).to_string();
    let mut parts = Vec::new();
    if t.added + t.updated + t.removed > 0 {
        parts.push(
            tr("From the phone: {a} new, {u} updated, {r} removed")
                .replace("{a}", &t.added.to_string())
                .replace("{u}", &t.updated.to_string())
                .replace("{r}", &t.removed.to_string()),
        );
    }
    if t.sent > 0 {
        parts.push(tr("Sent to the phone: {n}").replace("{n}", &t.sent.to_string()));
    }
    if t.conflicts > 0 {
        parts.push(
            tr("{n} in conflict: the phone's version is kept and yours is saved as a copy (conflicto)").replace("{n}", &t.conflicts.to_string()),
        );
    }
    if let Some(reason) = t.rejected.first() {
        parts.push(tr("{n} not accepted by the Worker: {why}").replace("{n}", &t.rejected.len().to_string()).replace("{why}", reason));
    }
    let changed = !parts.is_empty();
    match error {
        None => {
            let line = if changed { parts.join(". ") } else { tr("Points of interest are up to date") };
            app.pdi.last = Some(line.clone());
            app.pdi.last_ok = Some(true);
            if loud || changed {
                app.toast(ctx, line);
            }
        }
        Some(e) => {
            let text = tr(&e);
            let line = if changed { format!("{}. {text}", parts.join(". ")) } else { text };
            app.pdi.last = Some(line.clone());
            app.pdi.last_ok = Some(e == "Cancelled" && app.pdi.last_ok.unwrap_or(false));
            if loud && e != "Cancelled" {
                app.toast_error(ctx, line);
            } else if changed {
                app.toast(ctx, line);
            }
        }
    }
}

/// Once per frame: the launch and periodic synchronisations, and the next step of one in progress.
pub fn logic(app: &mut App, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    if app.ui.settings.pdi.enabled && app.styled && !app.pdi.running() {
        let due = !app.pdi.launched || now >= app.pdi.next_at;
        if due {
            app.pdi.launched = true;
            app.pdi.next_at = now + EVERY_SECS;
            let _ = start(app, false);
        }
    }
    if app.pdi.running() {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
    let done = app.pdi.lock().done.take();
    let Some(outcome) = done else { return };
    let stage = app.pdi.stage;
    match outcome {
        Err(e) => finish(app, ctx, Some(e)),
        Ok(Done::Pulled(p)) => {
            let merged = crate::planner::merge_phone_points(app, &p.points);
            // the position moves on only once the points are in
            app.ui.planner.pdi_cursor = p.cursor;
            if let Some(r) = &mut app.pdi.run {
                let n = |k: &str| merged.get(k).and_then(Value::as_u64).unwrap_or(0);
                r.totals.added += n("added");
                r.totals.updated += n("updated");
                r.totals.removed += n("removed");
                r.reload = false;
            }
            if stage == Stage::Pulling {
                let (ops, sent) = crate::planner::phone_push_ops(app);
                if ops.is_empty() {
                    finish(app, ctx, None);
                } else if let Err(e) = push(app, ops, sent) {
                    finish(app, ctx, Some(e));
                }
            } else {
                finish(app, ctx, None);
            }
        }
        Ok(Done::Pushed(results, error)) => {
            let sent = app.pdi.run.as_ref().map(|r| r.sent.clone()).unwrap_or_default();
            let sum: PushSummary = crate::planner::apply_phone_push(app, &sent, &results);
            if let Some(r) = &mut app.pdi.run {
                r.totals.sent += sum.sent;
                r.totals.conflicts += sum.conflicts;
                r.totals.rejected.extend(sum.rejected.clone());
                r.reload = sum.reload;
            }
            if error.is_some() {
                finish(app, ctx, error);
            } else if sum.sent + sum.conflicts > 0 {
                if let Err(e) = pull(app, true) {
                    finish(app, ctx, Some(e));
                }
            } else {
                finish(app, ctx, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Headless;
    use std::io::{Read, Write};

    struct Fixed(Option<String>);
    impl Keys for Fixed {
        fn get(&self) -> Result<Option<String>, String> {
            Ok(self.0.clone())
        }
        fn set(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    /// What the pretend Worker holds and was told.
    #[derive(Default)]
    struct Server {
        /// The points it holds, as it would send them: (seq, point).
        points: Vec<(u64, Value)>,
        ops: Vec<Value>,
        auth: Vec<String>,
    }

    /// A pretend Worker on this computer (`/api/v1`): health, pull by cursor, push (accepting at the next version, a
    /// conflict for the id "clash").
    fn worker(state: Arc<Mutex<Server>>) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut c) = stream else { return };
                let mut data = Vec::new();
                let mut buf = [0u8; 8192];
                let (head, body) = loop {
                    let n = c.read(&mut buf).unwrap_or(0);
                    data.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&data).to_string();
                    if let Some(at) = text.find("\r\n\r\n") {
                        let head = text[..at].to_string();
                        let want = head
                            .lines()
                            .find_map(|l| l.to_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok()))
                            .unwrap_or(0);
                        if data.len() >= at + 4 + want || n == 0 {
                            break (head, text[at + 4..].to_string());
                        }
                    } else if n == 0 {
                        break (text, String::new());
                    }
                };
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let mut s = state.lock().unwrap();
                if let Some(l) = head.lines().find(|l| l.to_lowercase().starts_with("authorization:")) {
                    s.auth.push(l.to_string());
                }
                let reply = if path == "/api/v1/health" {
                    r#"{"ok":true}"#.to_string()
                } else if path.starts_with("/api/v1/sync/pull") {
                    let cursor: u64 = path.split("cursor=").nth(1).and_then(|r| r.split('&').next()).and_then(|v| v.parse().ok()).unwrap_or(0);
                    let list: Vec<Value> = s.points.iter().filter(|(seq, _)| *seq > cursor).map(|(_, p)| p.clone()).collect();
                    let top = s.points.iter().map(|(q, _)| *q).max().unwrap_or(0).max(cursor);
                    json!({"pois": list, "cursor": top, "has_more": false}).to_string()
                } else if path == "/api/v1/sync/push" {
                    let ops: Vec<Value> = serde_json::from_str::<Value>(&body).ok().and_then(|v| v["ops"].as_array().cloned()).unwrap_or_default();
                    let mut results = Vec::new();
                    for op in ops {
                        s.ops.push(op.clone());
                        let id = op["poi"]["id"].as_str().or_else(|| op["id"].as_str()).unwrap_or("").to_string();
                        if id == "clash" {
                            results.push(json!({"id": id, "status": "conflict", "server_version": 4}));
                            continue;
                        }
                        let next = s.points.iter().map(|(q, _)| *q).max().unwrap_or(0) + 1;
                        let version = op["base_version"].as_u64().unwrap_or(0) + 1;
                        if op["type"] == "delete" {
                            s.points.retain(|(_, p)| p["id"] != id.as_str());
                            let mut gone = json!({"id": id, "name": "x", "latitude": 1.0, "longitude": 1.0, "version": version});
                            gone["deleted_at"] = json!("2026-10-23T15:39:36.223Z");
                            s.points.push((next, gone));
                        } else {
                            let mut p = op["poi"].clone();
                            p["version"] = json!(version);
                            s.points.retain(|(_, q)| q["id"] != id.as_str());
                            s.points.push((next, p));
                        }
                        results.push(json!({"id": id, "version": version, "status": "accepted"}));
                    }
                    json!({"results": results, "cursor": 1}).to_string()
                } else {
                    "{}".to_string()
                };
                drop(s);
                let _ = c.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                        reply.len()
                    )
                    .as_bytes(),
                );
            }
        });
        port
    }

    fn phone_point(id: &str, name: &str, version: u64) -> Value {
        json!({"schema_version":1,"id":id,"name":name,"latitude":43.1,"longitude":-2.1,"status":"want","version":version,"origin":"mobile",
               "created_at":"2026-10-23T08:30:00Z","updated_at":"2026-10-23T08:30:00Z","deleted_at":null})
    }

    fn app() -> Headless {
        let mut h = Headless::new();
        h.app.ui.settings.pdi.enabled = false;
        h
    }

    /// One whole synchronisation.
    fn sync(h: &mut Headless, port: u16) {
        h.app.ui.settings.pdi.url = format!("http://127.0.0.1:{port}");
        h.app.pdi.last = None;
        start_with(&mut h.app, true, Arc::new(Fixed(Some("a-long-enough-key".into()))), None).expect("starts");
        for _ in 0..300 {
            h.settle(std::time::Duration::from_millis(50));
            if !h.app.pdi.running() && h.app.pdi.last.is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("the synchronisation did not end: {:?}", h.app.pdi.stage);
    }

    fn mine(h: &Headless, name: &str) -> crate::planner::SavedPlace {
        h.app.ui.planner.saved.iter().find(|p| p.name == name).cloned().unwrap_or_else(|| panic!("no point called {name}"))
    }

    #[test]
    fn both_sides_end_with_the_same_points_and_repeating_it_changes_nothing() {
        let server = Arc::new(Mutex::new(Server::default()));
        server.lock().unwrap().points.push((1, phone_point("11111111-2222-4333-8444-555555555555", "Roble singular", 1)));
        let port = worker(server.clone());
        let mut h = app();
        h.app.ui.planner.saved.clear();
        h.app.ui.planner.saved.push(crate::planner::SavedPlace {
            name: "Faro de Higer".into(),
            lat: 43.39,
            lon: -1.79,
            origin: "desktop".into(),
            ..Default::default()
        });
        for round in 0..3 {
            sync(&mut h, port);
            assert_eq!(h.app.ui.planner.saved.len(), 2, "round {round}: {:?}", h.app.pdi.last);
            let srv = server.lock().unwrap();
            assert_eq!(srv.points.len(), 2, "the Worker has both");
            // the Mac's point went up once, as a new one on the contract's shape
            let ups: Vec<&Value> = srv.ops.iter().filter(|o| o["poi"]["name"] == "Faro de Higer").collect();
            assert_eq!(ups.len(), 1, "round {round}: sent more than once");
            let op = ups[0];
            assert_eq!((op["type"].as_str(), op["base_version"].as_u64()), (Some("upsert"), Some(0)));
            assert!(op["op_id"].as_str().is_some_and(|s| (8..=64).contains(&s.len())));
            assert_eq!(op["poi"]["category"], "otro");
            assert_eq!(op["poi"]["origin"], "desktop");
            assert!(op["poi"]["accuracy_m"].is_null());
            assert!(op["poi"].get("version").is_none() && op["poi"].get("updated_at").is_none());
            assert!(srv.auth.iter().all(|l| l.contains("Bearer a-long-enough-key")));
        }
        let faro = mine(&h, "Faro de Higer");
        assert!(!faro.dirty && faro.base == 1 && faro.version == 1, "{faro:?}");
        let roble = mine(&h, "Roble singular");
        assert!(!roble.dirty && roble.base == 1 && roble.origin == "mobile");
        // everything is in step: the map's cloud turns green
        assert_eq!(h.app.pdi.last_ok, Some(true));
        assert!(h.app.ui.planner.saved.iter().all(|p| !p.dirty && !p.conflict && p.base > 0));
    }

    #[test]
    fn an_edit_goes_up_with_its_base_version_and_a_deletion_as_a_delete() {
        let server = Arc::new(Mutex::new(Server::default()));
        server.lock().unwrap().points.push((1, phone_point("11111111-2222-4333-8444-555555555555", "Roble singular", 1)));
        server.lock().unwrap().points.push((2, phone_point("66666666-2222-4333-8444-555555555555", "Cascada", 3)));
        let port = worker(server.clone());
        let mut h = app();
        h.app.ui.planner.saved.clear();
        sync(&mut h, port);
        assert_eq!(h.app.ui.planner.saved.len(), 2);
        // edit one here
        {
            let p = h.app.ui.planner.saved.iter_mut().find(|p| p.name == "Roble singular").unwrap();
            p.notes = "Pista desde el aparcamiento".into();
            p.touch();
        }
        sync(&mut h, port);
        {
            let srv = server.lock().unwrap();
            let op = srv.ops.last().unwrap();
            assert_eq!(
                (op["type"].as_str(), op["base_version"].as_u64(), op["poi"]["notes"].as_str()),
                (Some("upsert"), Some(1), Some("Pista desde el aparcamiento"))
            );
        }
        assert_eq!((mine(&h, "Roble singular").base, mine(&h, "Roble singular").dirty), (2, false));
        // delete the other one here: the Worker is told, and it does not come back
        let at = h.app.ui.planner.saved.iter().position(|p| p.name == "Cascada").unwrap();
        let gone = h.app.ui.planner.saved.remove(at);
        h.app.ui.planner.pdi_deleted.push(crate::planner::Gone { id: gone.id.clone(), base: gone.base });
        sync(&mut h, port);
        let srv = server.lock().unwrap();
        let op = srv.ops.last().unwrap();
        assert_eq!((op["type"].as_str(), op["id"].as_str(), op["base_version"].as_u64()), (Some("delete"), Some(gone.id.as_str()), Some(3)));
        drop(srv);
        assert!(h.app.ui.planner.pdi_deleted.is_empty());
        assert!(h.app.ui.planner.saved.iter().all(|p| p.name != "Cascada"));
    }

    #[test]
    fn a_deletion_on_the_phone_removes_the_point_here_but_not_one_with_unsent_changes() {
        let server = Arc::new(Mutex::new(Server::default()));
        server.lock().unwrap().points.push((1, phone_point("11111111-2222-4333-8444-555555555555", "Roble singular", 1)));
        server.lock().unwrap().points.push((2, phone_point("66666666-2222-4333-8444-555555555555", "Cascada", 1)));
        let port = worker(server.clone());
        let mut h = app();
        h.app.ui.planner.saved.clear();
        sync(&mut h, port);
        assert_eq!(h.app.ui.planner.saved.len(), 2);
        {
            let mut srv = server.lock().unwrap();
            for (seq, id) in [(3u64, "11111111-2222-4333-8444-555555555555"), (4, "66666666-2222-4333-8444-555555555555")] {
                let mut p = phone_point(id, "x", 2);
                p["deleted_at"] = json!("2026-10-23T15:39:36.223Z");
                srv.points.push((seq, p));
            }
        }
        // the Cascada was edited here before the next synchronisation
        h.app.ui.planner.saved.iter_mut().find(|p| p.name == "Cascada").unwrap().touch();
        // (the Worker would answer that edit with a conflict: it has the point deleted)
        sync(&mut h, port);
        assert!(h.app.ui.planner.saved.iter().all(|p| p.name != "Roble singular"), "the phone's deletion applies");
        assert!(h.app.ui.planner.saved.iter().any(|p| p.name == "Cascada"), "unsent changes are not thrown away");
    }

    #[test]
    fn a_conflict_keeps_the_workers_point_marks_ours_and_downloads_again_from_the_start() {
        let server = Arc::new(Mutex::new(Server::default()));
        server.lock().unwrap().points.push((1, phone_point("clash", "Del móvil", 4)));
        let port = worker(server.clone());
        let mut h = app();
        h.app.ui.planner.saved.clear();
        sync(&mut h, port);
        {
            let p = &mut h.app.ui.planner.saved[0];
            p.notes = "cambio mío".into();
            p.touch();
        }
        // meanwhile the phone changed it too: version 5 at the Worker
        server.lock().unwrap().points.push((2, phone_point("clash", "Del móvil (nuevo)", 5)));
        sync(&mut h, port);
        let p = &h.app.ui.planner.saved[0];
        assert_eq!((p.name.as_str(), p.base, p.dirty, p.conflict), ("Del móvil (nuevo)", 5, false, false), "{:?}", h.app.pdi.last);
        assert!(h.app.pdi.last.as_deref().is_some_and(|l| l.contains("conflict")), "{:?}", h.app.pdi.last);
    }

    #[test]
    fn without_a_key_or_with_a_bad_address_it_says_so_and_changes_nothing() {
        let mut h = app();
        h.app.ui.settings.pdi.url = "http://pdi.example.com".into();
        assert!(start_with(&mut h.app, true, Arc::new(Fixed(None)), None).is_err(), "plain http to another computer is refused");
        assert!(!h.app.pdi.running());
        h.app.ui.settings.pdi.url = "http://127.0.0.1:9".into();
        let before = h.app.ui.planner.saved.len();
        start_with(&mut h.app, true, Arc::new(Fixed(None)), None).expect("starts");
        for _ in 0..200 {
            h.settle(std::time::Duration::from_millis(50));
            if !h.app.pdi.running() {
                break;
            }
        }
        assert!(h.app.pdi.last.as_deref().is_some_and(|l| l.contains("Ajustes") || l.contains("Settings")), "{:?}", h.app.pdi.last);
        assert_eq!(h.app.ui.planner.saved.len(), before);
    }
}
