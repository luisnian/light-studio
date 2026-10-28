//! Points of interest from the phone: the PDI Worker holds the points noted on the iPhone, and Light Studio
//! downloads the new and changed ones (`GET /sync/pull?cursor=`) so they appear on the map by themselves.
//!
//! This module only talks to the Worker and keeps its key; merging the points into the map is the planner's
//! `import` (by id, idempotent), so pulling the same points ten times leaves one of each.
//!
//! The Worker's key is kept in the macOS keychain (`/usr/bin/security`, fixed arguments, no shell) or, on Windows, in a
//! file encrypted for the user's own login. It never goes in
//! `ui.json`, a log or a message, and it is only ever sent over `https://` (plain `http://` is accepted for the
//! computer itself, which the tests use). `LIGHT_STUDIO_PDI_KEY` stands in for the keychain where there is none.

use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Settings ▸ General ▸ Points of Interest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PdiConfig {
    /// Look for new points when Light Studio opens and every few minutes.
    pub enabled: bool,
    /// The Worker's address.
    pub url: String,
}

impl Default for PdiConfig {
    fn default() -> Self {
        PdiConfig { enabled: true, url: "https://pdi.luisnietofoto.com".into() }
    }
}

/// What one pull brought.
#[derive(Debug, Default, PartialEq)]
pub struct Pulled {
    /// Where the API lives on the Worker (`/api/v1` or nothing).
    pub prefix: String,
    /// The points (contract records, as the Worker sent them).
    pub points: Vec<Value>,
    /// Where to continue next time.
    pub cursor: String,
}

/// The Worker, as far as a pull needs it (so it can be tested without a network).
pub trait Remote {
    /// `GET path` (starting with `/`): the status and the body.
    fn get_bytes(&self, path: &str) -> Result<(u16, Vec<u8>), String>;
    /// `POST path` with a JSON body: the status and the answer.
    fn post_json(&self, path: &str, body: &str) -> Result<(u16, String), String>;
    /// The same, with the body as text.
    fn get(&self, path: &str) -> Result<(u16, String), String> {
        self.get_bytes(path).map(|(s, b)| (s, String::from_utf8_lossy(&b).into_owned()))
    }
}

/// Most points one pull takes in (a guard against a server that never says it is done).
const MAX_PAGES: usize = 500;
const PAGE: usize = 200;
/// Largest answer taken from the Worker (a photo is at most 10 MB).
const MAX_BODY: usize = 16 * 1024 * 1024;
/// Largest photo kept, and how many are fetched in one go.
const MAX_PHOTO: u64 = 10 * 1024 * 1024;
const MAX_PHOTOS_PER_SYNC: usize = 60;

/// Whether the cursor is something the Worker could have given: digits, letters and a few separators.
fn clean_cursor(c: &str) -> bool {
    c.len() <= 200 && c.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '=' | ':'))
}

/// Where the API lives: the Worker answers JSON on `/api/v1/health` (anything outside `/api/` is the phone app's page,
/// which answers 200 with HTML, so it is never taken for the API).
pub fn find_prefix(remote: &dyn Remote) -> Result<&'static str, String> {
    const PREFIX: &str = "/api/v1";
    match remote.get(&format!("{PREFIX}/health"))? {
        (200, body) if serde_json::from_str::<Value>(&body).is_ok() => Ok(PREFIX),
        (401 | 403, _) => Err("The Worker did not accept the key: check it in Settings.".into()),
        (s, _) => Err(format!("That address does not look like the points-of-interest Worker (it answered {s}).")),
    }
}

/// Downloads everything after `cursor`, page by page.
pub fn pull(remote: &dyn Remote, cursor: &str, cancel: &AtomicBool) -> Result<Pulled, String> {
    use std::sync::atomic::Ordering;
    if !clean_cursor(cursor) {
        return Err("The saved synchronisation position is not valid".into());
    }
    let prefix = find_prefix(remote)?;
    let mut out = Pulled { prefix: prefix.to_string(), points: Vec::new(), cursor: cursor.to_string() };
    for _ in 0..MAX_PAGES {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let (status, body) = remote.get(&format!("{prefix}/sync/pull?cursor={}&limit={PAGE}", out.cursor))?;
        match status {
            200 => {}
            401 | 403 => return Err("The Worker did not accept the key: check it in Settings.".into()),
            s => return Err(format!("The Worker answered {s}")),
        }
        let v: Value = serde_json::from_str(&body).map_err(|_| "The Worker's answer could not be read".to_string())?;
        if let Some(list) = v.get("pois").and_then(Value::as_array) {
            out.points.extend(list.iter().cloned());
        }
        // the cursor may be a number or a string
        let next = match v.get("cursor") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => out.cursor.clone(),
        };
        if !clean_cursor(&next) {
            return Err("The Worker sent a position that cannot be used".into());
        }
        let more = v.get("has_more").and_then(Value::as_bool).unwrap_or(false);
        let moved = next != out.cursor;
        out.cursor = next;
        if !more || !moved {
            return Ok(out);
        }
    }
    Err("The Worker has too many points to download in one go; try again".into())
}

// ------------------------------------------------------------------------------------------------------- sending

/// How one operation of a push ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Taken: the point is now at this version on the Worker (`duplicate` — already taken before — counts too).
    Accepted { version: u64 },
    /// The Worker's version was not the one the change was made on: nothing was overwritten; what was sent is kept there
    /// as a copy called «… (conflicto)».
    Conflict { server_version: u64 },
    /// The point was not valid.
    Rejected { reason: String },
}

/// One operation's result: the point's id and how it ended.
#[derive(Clone, Debug, PartialEq)]
pub struct OpResult {
    pub id: String,
    pub outcome: Outcome,
}

/// A change to a point made here, as the Worker takes it. `base_version` is the version the Worker last confirmed (0
/// for a new point); `op_id` is the same for the same change however many times it is sent.
pub fn upsert_op(op_id: &str, base_version: u64, poi: Value) -> Value {
    serde_json::json!({"op_id": op_id, "type": "upsert", "base_version": base_version, "poi": poi})
}

/// A point deleted here.
pub fn delete_op(op_id: &str, id: &str, base_version: u64) -> Value {
    serde_json::json!({"op_id": op_id, "type": "delete", "id": id, "base_version": base_version})
}

/// The Worker takes at most this many operations, and this many bytes, in one request.
const MAX_OPS: usize = 100;
const MAX_REQUEST: usize = 900 * 1024;

/// Sends the operations in order, in as few requests as the limits allow, and returns one result per operation.
/// Stops at the first request that fails; the results so far are returned with the error.
pub fn push(remote: &dyn Remote, prefix: &str, ops: &[Value], cancel: &AtomicBool) -> (Vec<OpResult>, Option<String>) {
    use std::sync::atomic::Ordering;
    let mut done = Vec::new();
    let mut i = 0;
    while i < ops.len() {
        if cancel.load(Ordering::Relaxed) {
            return (done, Some("Cancelled".into()));
        }
        // one request: up to 100 operations and about 900 kB
        let mut size = 0;
        let mut n = 0;
        for op in ops.iter().skip(i).take(MAX_OPS) {
            let len = op.to_string().len();
            if n > 0 && size + len > MAX_REQUEST {
                break;
            }
            size += len;
            n += 1;
        }
        let batch = ops.get(i..i + n).unwrap_or_default();
        let body = serde_json::json!({ "ops": batch }).to_string();
        let (status, text) = match remote.post_json(&format!("{prefix}/sync/push"), &body) {
            Ok(r) => r,
            Err(e) => return (done, Some(e)),
        };
        match status {
            200 => {}
            401 | 403 => return (done, Some("The Worker did not accept the key: check it in Settings.".into())),
            s => return (done, Some(format!("The Worker answered {s} when sending the points"))),
        }
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            return (done, Some("The Worker's answer could not be read".into()));
        };
        let results = v.get("results").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        if results.len() != batch.len() {
            return (done, Some("The Worker answered a different number of results than points sent".into()));
        }
        for (op, r) in batch.iter().zip(results) {
            let id = r
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| op.get("id").and_then(Value::as_str))
                .or_else(|| op.pointer("/poi/id").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string();
            let num = |k: &str| r.get(k).and_then(Value::as_u64).unwrap_or(0);
            let outcome = match r.get("status").and_then(Value::as_str) {
                Some("accepted" | "duplicate") => Outcome::Accepted { version: num("version") },
                Some("conflict") => Outcome::Conflict { server_version: num("server_version") },
                _ => Outcome::Rejected { reason: r.get("reason").and_then(Value::as_str).unwrap_or("not accepted").chars().take(200).collect() },
            };
            done.push(OpResult { id, outcome });
        }
        i += n;
    }
    (done, None)
}

/// Tells the Worker how far this computer has read (it is for tidying up later; a failure changes nothing).
pub fn ack(remote: &dyn Remote, prefix: &str, cursor: &str) {
    let _ = remote
        .post_json(&format!("{prefix}/sync/ack"), &serde_json::json!({"device": "mac", "cursor": cursor.parse::<u64>().unwrap_or(0)}).to_string());
}

/// Whether a photo's key (an object name in the Worker's storage) can be used as part of an address and a file name.
fn clean_photo_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 200
        && !key.contains("..")
        && !key.starts_with('/')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

/// Where the photo with this key is kept on this computer (`dir` is the photo folder), or `None` for a key that cannot
/// be a file name.
pub fn photo_file(dir: &std::path::Path, key: &str) -> Option<std::path::PathBuf> {
    clean_photo_key(key).then(|| dir.join(key.replace('/', "__")))
}

/// Downloads the photos in `keys` that are not in `dir` yet, one by one; a photo that fails is left for the next time.
/// Returns how many arrived.
pub fn fetch_photos(remote: &dyn Remote, prefix: &str, keys: &[String], dir: &std::path::Path, cancel: &AtomicBool) -> usize {
    use std::sync::atomic::Ordering;
    let mut got = 0;
    if std::fs::create_dir_all(dir).is_err() {
        return 0;
    }
    for key in keys {
        if got >= MAX_PHOTOS_PER_SYNC || cancel.load(Ordering::Relaxed) {
            break;
        }
        let Some(file) = photo_file(dir, key) else { continue };
        if file.exists() {
            continue;
        }
        let Ok((200, bytes)) = remote.get_bytes(&format!("{prefix}/attachments/{key}")) else { continue };
        if bytes.is_empty() || bytes.len() as u64 > MAX_PHOTO {
            continue;
        }
        // whole or not at all: a half-written photo would look broken
        let tmp = file.with_extension("part");
        if std::fs::write(&tmp, &bytes).is_ok() && std::fs::rename(&tmp, &file).is_ok() {
            got += 1;
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    got
}

/// The photo keys of points that are still there (not marked deleted), from records as the Worker sends them.
pub fn photo_keys(points: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    for p in points {
        if p.get("deleted_at").is_some_and(|d| !d.is_null()) {
            continue;
        }
        for a in p.get("attachments").and_then(Value::as_array).into_iter().flatten() {
            let image = a.get("type").and_then(Value::as_str).is_none_or(|t| t.starts_with("image/"));
            if let (true, Some(k)) = (image, a.get("key").and_then(Value::as_str)) {
                out.push(k.to_string());
            }
        }
    }
    out
}

/// A downloaded photo as RGBA8 (at most 1280 px on its long side).
pub fn decode_photo(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut img = image::load_from_memory(bytes).ok()?;
    if img.width().max(img.height()) > 1280 {
        img = img.thumbnail(1280, 1280);
    }
    let img = img.to_rgba8();
    let (w, h) = (usize::try_from(img.width()).ok()?, usize::try_from(img.height()).ok()?);
    (w > 0 && h > 0).then(|| (w, h, img.into_raw()))
}

/// A key is kept only if it can be handed to `security` without any quoting: visible ASCII without quotes, backslash,
/// backtick or `$`.
pub fn valid_key(key: &str) -> bool {
    (8..=200).contains(&key.len()) && key.chars().all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '\\' | '`' | '$'))
}

/// A new key for the Worker and the phone: 256 random bits as 64 hexadecimal characters, from the system's random source.
pub fn generate_key() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "The system could not make a random key.".to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

// ---- linking phones ------------------------------------------------------------------------------

/// A one-use code to link a phone, as the Worker gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairCode {
    /// Eight characters in two groups, `BZTY-TV5G`.
    pub code: String,
    pub expires_at: String,
    /// The address that links the browser it is opened in (not the installed app: there the code is typed).
    pub link: String,
}

/// A phone linked to the Worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub last_used: Option<String>,
    pub revoked: bool,
}

/// What a status the linking calls do not expect means, in words.
fn link_error(status: u16, body: &str) -> String {
    match status {
        401 => "The Worker did not accept the key: check it in Settings.".into(),
        403 => "Linking phones needs the Worker's main key (API_KEY), not a phone's key.".into(),
        404 => "The Worker does not know that phone.".into(),
        429 => "The Worker has too many codes or attempts open: wait a few minutes.".into(),
        s => serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| format!("The Worker answered {s}")),
    }
}

/// Asks the Worker for a code to link a phone (needs the main key).
pub fn request_code(remote: &dyn Remote, prefix: &str) -> Result<PairCode, String> {
    let (status, body) = remote.post_json(&format!("{prefix}/pair"), "{}")?;
    if !matches!(status, 200 | 201) {
        return Err(link_error(status, &body));
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| "The Worker's answer was not understood".to_string())?;
    let text = |k: &str| v[k].as_str().map(str::to_string);
    let code = text("code").filter(|c| c.len() <= 16 && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-'));
    match (code, text("expires_at"), text("link")) {
        (Some(code), Some(expires_at), Some(link)) => Ok(PairCode { code, expires_at, link }),
        _ => Err("The Worker's answer was not understood".into()),
    }
}

/// The phones linked to the Worker (needs the main key).
pub fn list_devices(remote: &dyn Remote, prefix: &str) -> Result<Vec<Device>, String> {
    let (status, body) = remote.get(&format!("{prefix}/devices"))?;
    if status != 200 {
        return Err(link_error(status, &body));
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| "The Worker's answer was not understood".to_string())?;
    let list = v["devices"].as_array().ok_or_else(|| "The Worker's answer was not understood".to_string())?;
    Ok(list
        .iter()
        .filter_map(|d| {
            Some(Device {
                id: d["id"].as_str().filter(|i| clean_device_id(i))?.to_string(),
                name: d["name"].as_str().unwrap_or("").chars().take(60).collect(),
                created_at: d["created_at"].as_str().unwrap_or("").to_string(),
                last_used: d["last_used"].as_str().map(str::to_string),
                revoked: d["revoked"].as_bool().unwrap_or(false),
            })
        })
        .collect())
}

/// Whether `id` can go into an address as it is.
fn clean_device_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Stops a linked phone's key from working at once.
pub fn revoke_device(remote: &dyn Remote, prefix: &str, id: &str) -> Result<(), String> {
    if !clean_device_id(id) {
        return Err("The Worker does not know that phone.".into());
    }
    let (status, body) = remote.post_json(&format!("{prefix}/devices/{id}/revoke"), "{}")?;
    if status == 200 { Ok(()) } else { Err(link_error(status, &body)) }
}

/// The Worker's address, checked: `https://`, or `http://` only for this computer.
pub fn valid_url(url: &str) -> Result<String, String> {
    let u = crate::net::parse_address(url).ok_or_else(|| "The Worker's address is not valid: check it in Settings.".to_string())?;
    let local = matches!(u.host.as_str(), "localhost" | "127.0.0.1" | "::1");
    if !u.secure && !local {
        return Err("The Worker's address must start with https://".into());
    }
    Ok(url.trim().trim_end_matches('/').to_string())
}

/// Where the key lives.
pub trait Keys {
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, key: &str) -> Result<(), String>;
}

#[cfg(target_os = "macos")]
const SERVICE: &str = "Light Studio PDI";
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "worker";

/// Where the Worker's key is kept: the macOS keychain, or on Windows a file only this user can decrypt (a stand-in
/// key from the environment where there is none).
pub struct Keychain;

impl Keys for Keychain {
    fn get(&self) -> Result<Option<String>, String> {
        if let Ok(k) = std::env::var("LIGHT_STUDIO_PDI_KEY")
            && valid_key(k.trim())
        {
            return Ok(Some(k.trim().to_string()));
        }
        platform::get()
    }

    fn set(&self, key: &str) -> Result<(), String> {
        if !SUPPORTED {
            return Err("The key can only be kept on the Mac or on Windows".into());
        }
        if !valid_key(key) {
            return Err("That key is not valid (8 to 200 letters, digits or symbols, without quotes)".into());
        }
        platform::set(key)
    }
}

/// Whether this build can keep the key (macOS keychain, Windows protected file).
pub const SUPPORTED: bool = cfg!(any(target_os = "macos", windows));

#[cfg(target_os = "macos")]
mod platform {
    use super::{ACCOUNT, SERVICE, valid_key};

    pub fn get() -> Result<Option<String>, String> {
        let out = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", SERVICE, "-a", ACCOUNT, "-w"])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("could not read the keychain: {e}"))?;
        if out.status.code() == Some(44) {
            return Ok(None);
        }
        if !out.status.success() {
            return Err("could not read the key from the keychain".into());
        }
        let key = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(valid_key(&key).then_some(key))
    }

    pub fn set(key: &str) -> Result<(), String> {
        use std::io::Write;
        // the key goes in through the standard input, not the arguments, so it never shows in the process list
        let mut child = std::process::Command::new("/usr/bin/security")
            .arg("-i")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("could not use the keychain: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "add-generic-password -U -s \"{SERVICE}\" -a \"{ACCOUNT}\" -w \"{key}\"")
                .map_err(|e| format!("could not use the keychain: {e}"))?;
        }
        let status = child.wait().map_err(|e| format!("could not use the keychain: {e}"))?;
        if status.success() { Ok(()) } else { Err("The keychain did not accept the key".into()) }
    }
}

/// Windows: the key is encrypted with the user's own Windows login (DPAPI, through PowerShell, the key going in by
/// the standard input) and the result is kept in the settings folder; only this user on this computer can read it.
#[cfg(windows)]
mod platform {
    use super::valid_key;
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const NO_WINDOW: u32 = 0x0800_0000;
    const PROTECT: &str = "Add-Type -AssemblyName System.Security; $k=[Console]::In.ReadToEnd().Trim(); \
        [Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($k),$null,\
        [Security.Cryptography.DataProtectionScope]::CurrentUser))";
    const UNPROTECT: &str = "Add-Type -AssemblyName System.Security; $b=[Console]::In.ReadToEnd().Trim(); \
        [Text.Encoding]::UTF8.GetString([Security.Cryptography.ProtectedData]::Unprotect([Convert]::FromBase64String($b),$null,\
        [Security.Cryptography.DataProtectionScope]::CurrentUser))";

    fn file() -> Result<std::path::PathBuf, String> {
        crate::paths::config_dir().map(|d| d.join("pdi-key.bin")).ok_or_else(|| "There is no settings folder to keep the key in".to_string())
    }

    fn powershell(script: &str, input: &str) -> Result<String, String> {
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
            .creation_flags(NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not protect the key: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input.as_bytes()).map_err(|e| format!("could not protect the key: {e}"))?;
        }
        let out = child.wait_with_output().map_err(|e| format!("could not protect the key: {e}"))?;
        if !out.status.success() {
            return Err("Windows could not protect or read the key".into());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn get() -> Result<Option<String>, String> {
        let path = file()?;
        let Ok(stored) = std::fs::read_to_string(&path) else { return Ok(None) };
        let key = powershell(UNPROTECT, stored.trim())?;
        Ok(valid_key(&key).then_some(key))
    }

    pub fn set(key: &str) -> Result<(), String> {
        let path = file()?;
        let protected = powershell(PROTECT, key)?;
        if protected.is_empty() {
            return Err("Windows could not protect or read the key".into());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("could not keep the key: {e}"))?;
        }
        std::fs::write(&path, protected).map_err(|e| format!("could not keep the key: {e}"))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    pub fn get() -> Result<Option<String>, String> {
        Ok(None)
    }

    pub fn set(_: &str) -> Result<(), String> {
        Err("The key can only be kept on the Mac or on Windows".into())
    }
}

/// The real Worker over HTTPS.
#[cfg(not(target_arch = "wasm32"))]
pub struct Worker<'a> {
    pub base: String,
    pub key: String,
    pub cancel: &'a AtomicBool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Remote for Worker<'_> {
    fn post_json(&self, path: &str, body: &str) -> Result<(u16, String), String> {
        self.post(path, body)
    }

    fn get_bytes(&self, path: &str) -> Result<(u16, Vec<u8>), String> {
        let r = self.request(path, None, 20)?;
        Ok((r.status, r.body))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Worker<'_> {
    /// One call to the Worker: a GET, or a POST with a JSON body.
    fn request(&self, path: &str, body: Option<&[u8]>, total_secs: u64) -> Result<crate::net::Reply, String> {
        use crate::net;
        if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let url = format!("{}{path}", self.base);
        let mut headers =
            vec![("Accept", "application/json".to_string()), ("Authorization", format!("Bearer {}", self.key)), ("x-device", "mac".to_string())];
        let t = net::Timeouts::secs(8, total_secs);
        let answer = match body {
            None => net::get(&url, &headers, t, MAX_BODY),
            Some(b) => {
                headers.push(("Content-Type", "application/json".to_string()));
                net::post(&url, &headers, b, t, MAX_BODY)
            }
        };
        answer.map_err(|e| match e {
            net::NetError::BadUrl => "The Worker's address is not valid".to_string(),
            net::NetError::TooBig => "The Worker answered too much".to_string(),
            net::NetError::Unreachable(_) => "Could not reach the points-of-interest Worker (is the computer online?)".to_string(),
        })
    }

    fn post(&self, path: &str, body: &str) -> Result<(u16, String), String> {
        let r = self.request(path, Some(body.as_bytes()), 40)?;
        Ok((r.status, String::from_utf8_lossy(&r.body).into_owned()))
    }
}

#[cfg(test)]
mod tests {
    /// A pretend Worker that answers each path with a fixed status and body.
    struct Canned(Vec<(&'static str, u16, &'static str)>, std::cell::RefCell<Vec<String>>);
    impl Remote for Canned {
        fn get_bytes(&self, path: &str) -> Result<(u16, Vec<u8>), String> {
            self.1.borrow_mut().push(format!("GET {path}"));
            let (_, s, b) = self.0.iter().find(|(p, _, _)| *p == path).ok_or("unexpected")?;
            Ok((*s, b.as_bytes().to_vec()))
        }
        fn post_json(&self, path: &str, _: &str) -> Result<(u16, String), String> {
            self.1.borrow_mut().push(format!("POST {path}"));
            let (_, s, b) = self.0.iter().find(|(p, _, _)| *p == path).ok_or("unexpected")?;
            Ok((*s, (*b).to_string()))
        }
    }

    #[test]
    fn a_code_is_asked_for_and_read() {
        let w = Canned(
            vec![(
                "/api/v1/pair",
                201,
                r#"{"code":"BZTY-TV5G","expires_at":"2026-10-25T11:24:36.390Z","link":"https://pdi.luisnietofoto.com/#pair=BZTYTV5G"}"#,
            )],
            Default::default(),
        );
        let c = request_code(&w, "/api/v1").unwrap();
        assert_eq!((c.code.as_str(), c.link.as_str()), ("BZTY-TV5G", "https://pdi.luisnietofoto.com/#pair=BZTYTV5G"));
        // a phone's key is refused, a wrong answer is an error and not a panic
        let w = Canned(vec![("/api/v1/pair", 403, r#"{"error":"forbidden"}"#)], Default::default());
        assert!(request_code(&w, "/api/v1").unwrap_err().contains("main key"));
        let w = Canned(vec![("/api/v1/pair", 201, "{}")], Default::default());
        assert!(request_code(&w, "/api/v1").is_err());
        let w = Canned(vec![("/api/v1/pair", 429, "")], Default::default());
        assert!(request_code(&w, "/api/v1").unwrap_err().contains("wait"));
    }

    #[test]
    fn phones_are_listed_and_revoked() {
        let body = r#"{"devices":[{"id":"dfa333a7-e1ca-410c-8ee5-03830eeaecf2","name":"iPhone","created_at":"2026-10-25T11:14:36.420Z","last_used":null,"revoked":false},{"id":"../x","name":"bad"}]}"#;
        let w = Canned(
            vec![("/api/v1/devices", 200, body), ("/api/v1/devices/dfa333a7-e1ca-410c-8ee5-03830eeaecf2/revoke", 200, r#"{"ok":true}"#)],
            Default::default(),
        );
        let list = list_devices(&w, "/api/v1").unwrap();
        assert_eq!(list.len(), 1, "an id that is not an id is left out");
        assert_eq!((list[0].name.as_str(), list[0].last_used.clone(), list[0].revoked), ("iPhone", None, false));
        revoke_device(&w, "/api/v1", &list[0].id).unwrap();
        assert!(revoke_device(&w, "/api/v1", "../etc").is_err(), "nothing is sent for a bad id");
        assert_eq!(w.1.borrow().len(), 2);
    }

    #[test]
    fn a_generated_key_is_long_random_and_fit_for_the_keychain() {
        let (a, b) = (generate_key().unwrap(), generate_key().unwrap());
        assert_eq!(a.len(), 64);
        assert!(valid_key(&a));
        assert_ne!(a, b);
    }

    use super::*;
    use std::cell::RefCell;

    /// A pretend Worker: pages of points keyed by the cursor it is given.
    struct Fake {
        health_at: &'static str,
        seen: RefCell<Vec<String>>,
        status: u16,
    }

    impl Remote for Fake {
        fn post_json(&self, _: &str, _: &str) -> Result<(u16, String), String> {
            Err("not used".into())
        }
        fn get_bytes(&self, path: &str) -> Result<(u16, Vec<u8>), String> {
            self.get_text(path).map(|(s, b)| (s, b.into_bytes()))
        }
    }

    impl Fake {
        fn get_text(&self, path: &str) -> Result<(u16, String), String> {
            self.seen.borrow_mut().push(path.to_string());
            if path.ends_with("/health") {
                return Ok(if path == format!("{}/health", self.health_at) { (200, r#"{"ok":true}"#.into()) } else { (200, "<html>".into()) });
            }
            if self.status != 200 {
                return Ok((self.status, String::new()));
            }
            let page = |pois: &str, cursor: &str, more: bool| Ok((200, format!(r#"{{"pois":[{pois}],"cursor":{cursor},"has_more":{more}}}"#)));
            if path.contains("cursor=0&") {
                page(r#"{"id":"a"},{"id":"b"}"#, "2", true)
            } else if path.contains("cursor=2&") {
                page(r#"{"id":"c"}"#, "\"3\"", false)
            } else {
                page("", "3", false)
            }
        }
    }

    #[test]
    fn pulls_every_page_from_the_api_prefix_and_returns_the_new_cursor() {
        {
            let health_at = "/api/v1";
            let f = Fake { health_at, seen: RefCell::new(Vec::new()), status: 200 };
            let r = pull(&f, "0", &AtomicBool::new(false)).unwrap();
            let ids: Vec<_> = r.points.iter().map(|p| p["id"].as_str().unwrap().to_string()).collect();
            assert_eq!((ids, r.cursor.as_str()), (vec!["a".into(), "b".into(), "c".into()], "3"));
            assert!(f.seen.borrow().iter().any(|p| p.starts_with(&format!("{health_at}/sync/pull"))));
            // nothing new: the same cursor comes back and no point
            let again = pull(&f, "3", &AtomicBool::new(false)).unwrap();
            assert!(again.points.is_empty() && again.cursor == "3");
        }
    }

    #[test]
    fn a_refused_key_a_wrong_address_and_a_bad_cursor_give_messages_not_panics() {
        let f = Fake { health_at: "/api/v1", seen: RefCell::new(Vec::new()), status: 401 };
        assert!(pull(&f, "0", &AtomicBool::new(false)).unwrap_err().contains("key"));
        let wrong = Fake { health_at: "/nowhere", seen: RefCell::new(Vec::new()), status: 200 };
        assert!(pull(&wrong, "0", &AtomicBool::new(false)).unwrap_err().contains("does not look like"));
        assert!(pull(&f, "0&x=1", &AtomicBool::new(false)).is_err());
        let stop = AtomicBool::new(true);
        let ok = Fake { health_at: "/api/v1", seen: RefCell::new(Vec::new()), status: 200 };
        assert_eq!(pull(&ok, "0", &stop).unwrap_err(), "Cancelled");
    }

    #[test]
    fn keys_and_addresses_are_checked_before_they_are_used() {
        assert!(valid_key("abcDEF123-_.="));
        assert!(!valid_key("short"));
        assert!(!valid_key("has space in it 123"));
        assert!(!valid_key("quote\"inside12"));
        assert!(!valid_key("dollar$inside12"));
        assert_eq!(valid_url("https://pdi.luisnietofoto.com/").unwrap(), "https://pdi.luisnietofoto.com");
        assert!(valid_url("http://pdi.luisnietofoto.com").is_err());
        assert!(valid_url("http://127.0.0.1:8787").is_ok());
        assert!(valid_url("ftp://x").is_err());
    }

    #[test]
    fn photos_are_downloaded_once_whole_and_only_for_clean_keys() {
        struct Photos(RefCell<Vec<String>>);
        impl Remote for Photos {
            fn post_json(&self, _: &str, _: &str) -> Result<(u16, String), String> {
                Err("not used".into())
            }
            fn get_bytes(&self, path: &str) -> Result<(u16, Vec<u8>), String> {
                self.0.borrow_mut().push(path.to_string());
                if path.ends_with("gone") { Ok((404, Vec::new())) } else { Ok((200, b"jpegbytes".to_vec())) }
            }
        }
        let dir = std::env::temp_dir().join(format!("pdi-photos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = Photos(RefCell::new(Vec::new()));
        let keys: Vec<String> = ["poi1/att1", "../evil", "poi2/gone", "a b"].iter().map(|k| k.to_string()).collect();
        assert_eq!(fetch_photos(&r, "/api/v1", &keys, &dir, &AtomicBool::new(false)), 1);
        assert_eq!(std::fs::read(dir.join("poi1__att1")).unwrap(), b"jpegbytes");
        assert_eq!(r.0.borrow().len(), 2, "the unclean keys never reach the network: {:?}", r.0.borrow());
        // again: what is there is not fetched twice
        assert_eq!(fetch_photos(&r, "/api/v1", &keys, &dir, &AtomicBool::new(false)), 0);
        assert_eq!(r.0.borrow().len(), 3);
        assert!(!dir.join("poi1__att1.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let pts: Vec<Value> = serde_json::from_str(r#"[{"attachments":[{"key":"k1","type":"image/jpeg"}]},{"deleted_at":"2026-10-23T10:00:00Z","attachments":[{"key":"k2"}]},{"attachments":[{"key":"k3","type":"video/mp4"}]}]"#).unwrap();
        assert_eq!(photo_keys(&pts), vec!["k1".to_string()]);
    }

    /// A pretend Worker for sending: answers each operation as the test decides, and remembers the requests.
    struct Sender {
        requests: RefCell<Vec<Value>>,
    }

    impl Remote for Sender {
        fn get_bytes(&self, _: &str) -> Result<(u16, Vec<u8>), String> {
            Err("not used".into())
        }
        fn post_json(&self, _: &str, body: &str) -> Result<(u16, String), String> {
            let v: Value = serde_json::from_str(body).unwrap();
            self.requests.borrow_mut().push(v.clone());
            let results: Vec<Value> = v["ops"]
                .as_array()
                .unwrap()
                .iter()
                .map(|op| {
                    let id = op["poi"]["id"].as_str().or_else(|| op["id"].as_str()).unwrap();
                    match id {
                        "conflicted" => serde_json::json!({"id": id, "status": "conflict", "server_version": 5, "copy_id": "c"}),
                        "bad" => serde_json::json!({"op_id": op["op_id"], "status": "rejected", "reason": "name obligatorio"}),
                        "again" => serde_json::json!({"id": id, "version": 3, "status": "duplicate"}),
                        _ => serde_json::json!({"id": id, "version": op["base_version"].as_u64().unwrap() + 1, "status": "accepted"}),
                    }
                })
                .collect();
            Ok((200, serde_json::json!({"results": results, "cursor": 9}).to_string()))
        }
    }

    #[test]
    fn a_push_reports_each_operation_and_splits_into_requests_of_at_most_100() {
        let s = Sender { requests: RefCell::new(Vec::new()) };
        let mut ops: Vec<Value> = (0..230).map(|i| upsert_op(&format!("mac-op-{i:05}"), 0, serde_json::json!({"id": format!("p{i}")}))).collect();
        ops.push(upsert_op("mac-op-conflict", 2, serde_json::json!({"id": "conflicted"})));
        ops.push(upsert_op("mac-op-bad", 0, serde_json::json!({"id": "bad"})));
        ops.push(upsert_op("mac-op-again", 2, serde_json::json!({"id": "again"})));
        ops.push(delete_op("mac-del-gone", "p1", 4));
        let (results, err) = push(&s, "/api/v1", &ops, &AtomicBool::new(false));
        assert_eq!(err, None);
        assert_eq!(results.len(), ops.len());
        assert_eq!(s.requests.borrow().iter().map(|r| r["ops"].as_array().unwrap().len()).collect::<Vec<_>>(), vec![100, 100, 34]);
        assert_eq!(results[0], OpResult { id: "p0".into(), outcome: Outcome::Accepted { version: 1 } });
        assert_eq!(results[230].outcome, Outcome::Conflict { server_version: 5 });
        assert_eq!(results[231], OpResult { id: "bad".into(), outcome: Outcome::Rejected { reason: "name obligatorio".into() } });
        assert_eq!(results[232].outcome, Outcome::Accepted { version: 3 });
        assert_eq!(results[233], OpResult { id: "p1".into(), outcome: Outcome::Accepted { version: 5 } });
        // the shape the Worker documents
        let first = &s.requests.borrow()[0]["ops"][0];
        assert_eq!((first["type"].as_str(), first["base_version"].as_u64(), first["poi"]["id"].as_str()), (Some("upsert"), Some(0), Some("p0")));
    }

    #[test]
    fn a_push_that_cannot_go_through_says_so_and_keeps_what_was_done() {
        struct Down;
        impl Remote for Down {
            fn get_bytes(&self, _: &str) -> Result<(u16, Vec<u8>), String> {
                Err("down".into())
            }
            fn post_json(&self, _: &str, _: &str) -> Result<(u16, String), String> {
                Ok((401, String::new()))
            }
        }
        let (r, e) = push(&Down, "/api/v1", &[upsert_op("mac-op-00001", 0, serde_json::json!({"id": "a"}))], &AtomicBool::new(false));
        assert!(r.is_empty() && e.unwrap().contains("key"));
        let (r, e) = push(&Down, "/api/v1", &[delete_op("mac-op-00002", "a", 1)], &AtomicBool::new(true));
        assert!(r.is_empty() && e.as_deref() == Some("Cancelled"));
    }
}
