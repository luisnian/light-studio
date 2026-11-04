//! Linking phones to the Worker (the PDI Worker): a one-use code asked for with the main key and typed in the phone's app,
//! and the list of the phones linked, each of which can be revoked. The calls are `crate::services::pdi_sync`; each runs
//! on a worker thread (an activity row) and its result is applied here, on the UI thread.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};

use crate::services::pdi_sync::{self, Device, Keys, PairCode};

use crate::App;

/// How long a code lasts (the Worker's own limit), in seconds.
pub const CODE_SECS: f64 = 600.0;

/// What to ask the Worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    Code,
    Devices,
    Revoke(String),
}

enum Done {
    Code(PairCode),
    Devices(Vec<Device>),
    Revoked,
}

#[derive(Default)]
struct Shared {
    done: Option<Result<Done, String>>,
}

/// The state Settings shows.
#[derive(Default)]
pub struct Pair {
    shared: Arc<Mutex<Shared>>,
    busy: bool,
    /// Where the key is read from (the follow-up requests use the same).
    keys: Option<Arc<dyn Keys + Send + Sync>>,
    /// The code in force and when it arrived (seconds on the UI clock).
    pub code: Option<(PairCode, f64)>,
    /// The phones linked, once asked for.
    pub devices: Option<Vec<Device>>,
    /// The last thing that went wrong or right, as a line of text.
    pub message: Option<String>,
}

impl Pair {
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Seconds left of the code, if there is one in force.
    pub fn left(&self, now: f64) -> Option<f64> {
        self.code.as_ref().map(|(_, at)| CODE_SECS - (now - at)).filter(|s| *s > 0.0)
    }
}

/// Asks the Worker (with the key in the keychain).
pub fn start(app: &mut App, job: Job) {
    start_with(app, job, Arc::new(pdi_sync::Keychain));
}

fn start_with(app: &mut App, job: Job, keys: Arc<dyn Keys + Send + Sync>) {
    if app.pdi_pair.busy {
        return;
    }
    let base = match pdi_sync::valid_url(&app.ui.settings.pdi.url) {
        Ok(b) => b,
        Err(e) => {
            app.pdi_pair.message = Some(crate::i18n::tr(&e).to_string());
            return;
        }
    };
    let shared = app.pdi_pair.shared.clone();
    app.pdi_pair.busy = true;
    app.pdi_pair.keys = Some(keys.clone());
    app.pdi_pair.message = None;
    let spawned = std::thread::Builder::new().name("pdi-pair".into()).spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let key = keys.get()?.ok_or_else(|| "No key for the points-of-interest Worker yet: save it in Settings.".to_string())?;
            let cancel = AtomicBool::new(false);
            let remote = pdi_sync::Worker { base, key, cancel: &cancel };
            let prefix = pdi_sync::find_prefix(&remote)?;
            match job {
                Job::Code => pdi_sync::request_code(&remote, prefix).map(Done::Code),
                Job::Devices => pdi_sync::list_devices(&remote, prefix).map(Done::Devices),
                Job::Revoke(id) => pdi_sync::revoke_device(&remote, prefix, &id).map(|()| Done::Revoked),
            }
        }))
        .unwrap_or_else(|_| Err("Linking phones stopped unexpectedly".to_string()));
        shared.lock().unwrap_or_else(PoisonError::into_inner).done = Some(outcome);
    });
    if let Err(e) = spawned {
        app.pdi_pair.busy = false;
        app.pdi_pair.message = Some(format!("could not start: {e}"));
    }
}

/// Once per frame: applies an answer and, after a code or a revocation, asks for the list again.
pub fn logic(app: &mut App, ctx: &egui::Context) {
    if app.pdi_pair.busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
    let done = app.pdi_pair.shared.lock().unwrap_or_else(PoisonError::into_inner).done.take();
    let Some(outcome) = done else { return };
    app.pdi_pair.busy = false;
    let now = ctx.input(|i| i.time);
    match outcome {
        Err(e) => app.pdi_pair.message = Some(crate::i18n::tr(&e).to_string()),
        Ok(Done::Code(c)) => {
            app.pdi_pair.code = Some((c, now));
            again(app);
        }
        Ok(Done::Devices(list)) => app.pdi_pair.devices = Some(list),
        Ok(Done::Revoked) => {
            app.pdi_pair.message = Some(crate::i18n::tr("The phone's key no longer works").to_string());
            again(app);
        }
    }
}

/// Asks for the list of phones again, reading the key where the last request did.
fn again(app: &mut App) {
    let keys = app.pdi_pair.keys.clone().unwrap_or_else(|| Arc::new(pdi_sync::Keychain));
    start_with(app, Job::Devices, keys);
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

    /// A pretend Worker: health, a code, the list of phones, a revocation. `code_status` is what `/pair` answers.
    fn worker(code_status: u16, log: Arc<Mutex<Vec<String>>>) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut c) = stream else { return };
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = c.read(&mut buf).unwrap_or(0);
                    data.extend_from_slice(&buf[..n]);
                    if n == 0 || String::from_utf8_lossy(&data).contains("\r\n\r\n") {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&data).to_string();
                let mut first = text.lines().next().unwrap_or("").split_whitespace();
                let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
                log.lock().unwrap().push(format!("{method} {path}"));
                let (status, reply) = match (method.as_str(), path.as_str()) {
                    (_, "/api/v1/health") => (200, r#"{"ok":true}"#.to_string()),
                    ("POST", "/api/v1/pair") => (
                        code_status,
                        r#"{"code":"ABCD-EFGH","expires_at":"2026-10-25T11:24:36.390Z","link":"https://pdi.example.com/#pair=ABCDEFGH"}"#.to_string(),
                    ),
                    ("GET", "/api/v1/devices") => (
                        200,
                        r#"{"devices":[{"id":"dfa333a7-e1ca-410c-8ee5-03830eeaecf2","name":"iPhone","created_at":"2026-10-25T11:14:36Z","last_used":null,"revoked":false}]}"#
                            .to_string(),
                    ),
                    ("POST", p) if p.ends_with("/revoke") => (200, r#"{"ok":true}"#.to_string()),
                    _ => (404, "{}".to_string()),
                };
                let _ = c.write_all(
                    format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                        reply.len()
                    )
                    .as_bytes(),
                );
            }
        });
        port
    }

    fn app(port: u16) -> Headless {
        let mut h = Headless::new();
        h.app.ui.settings.pdi.enabled = false;
        h.app.ui.settings.pdi.url = format!("http://127.0.0.1:{port}");
        h
    }

    fn wait(h: &mut Headless) {
        for _ in 0..300 {
            h.settle(std::time::Duration::from_millis(50));
            if !h.app.pdi_pair.busy() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("the request did not end");
    }

    #[test]
    fn a_code_is_shown_then_the_phones_are_listed_and_one_is_revoked() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let port = worker(201, log.clone());
        let mut h = app(port);
        start_with(&mut h.app, Job::Code, Arc::new(Fixed(Some("a-long-enough-key".into()))));
        wait(&mut h);
        let (code, _) = h.app.pdi_pair.code.clone().expect("a code");
        assert_eq!(code.code, "ABCD-EFGH");
        assert!(h.app.pdi_pair.left(h.app.pdi_pair.code.as_ref().unwrap().1 + 5.0).is_some_and(|s| s > 590.0));
        assert!(h.app.pdi_pair.left(h.app.pdi_pair.code.as_ref().unwrap().1 + 601.0).is_none(), "the code lapses");
        // asking for the code was followed by the list
        wait(&mut h);
        assert_eq!(h.app.pdi_pair.devices.as_ref().map(Vec::len), Some(1));
        let id = "dfa333a7-e1ca-410c-8ee5-03830eeaecf2".to_string();
        start_with(&mut h.app, Job::Revoke(id.clone()), Arc::new(Fixed(Some("a-long-enough-key".into()))));
        wait(&mut h);
        assert!(log.lock().unwrap().iter().any(|l| l == &format!("POST /api/v1/devices/{id}/revoke")));
    }

    #[test]
    fn a_refused_key_says_so_and_no_key_asks_for_one() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let port = worker(403, log);
        let mut h = app(port);
        start_with(&mut h.app, Job::Code, Arc::new(Fixed(Some("a-long-enough-key".into()))));
        wait(&mut h);
        assert!(h.app.pdi_pair.code.is_none());
        assert!(h.app.pdi_pair.message.as_deref().is_some_and(|m| m.contains("API_KEY") || m.contains("principal")), "{:?}", h.app.pdi_pair.message);
        start_with(&mut h.app, Job::Code, Arc::new(Fixed(None)));
        wait(&mut h);
        assert!(h.app.pdi_pair.message.is_some());
    }
}
