//! The app's only door to the network: plain GET and POST over HTTPS (or `http://` for this computer, in tests), with
//! timeouts and a limit on how much of the answer is read. Everything that talks to a service goes through here.

use std::time::Duration;

/// Why a request did not get an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// The address cannot be used.
    BadUrl,
    /// The server could not be reached, or stopped answering.
    Unreachable(String),
    /// The answer is larger than the limit given.
    TooBig,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::BadUrl => write!(f, "unusable address"),
            NetError::Unreachable(e) => write!(f, "could not reach the server: {e}"),
            NetError::TooBig => write!(f, "the answer is too large"),
        }
    }
}

/// How long to wait.
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    pub connect: Duration,
    /// For the whole request, answer included.
    pub total: Duration,
}

impl Timeouts {
    pub const fn secs(connect: u64, total: u64) -> Timeouts {
        Timeouts { connect: Duration::from_secs(connect), total: Duration::from_secs(total) }
    }
}

/// What a server answered.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    /// The first header called `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// An address split into what the callers check: whether it is secure and which host it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub secure: bool,
    pub host: String,
}

/// Reads `http://` and `https://` addresses.
pub fn parse_address(url: &str) -> Option<Address> {
    let url = url.trim();
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else {
        let r = url.strip_prefix("http://")?;
        (false, r)
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("").to_string()
    } else {
        authority.split(':').next().unwrap_or("").to_string()
    };
    (!host.is_empty() && !host.contains(char::is_whitespace)).then_some(Address { secure, host })
}

fn agent(t: Timeouts) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(t.connect))
        .timeout_global(Some(t.total))
        .http_status_as_error(false)
        .user_agent("Light Studio")
        .build()
        .into()
}

fn finish(mut resp: ureq::http::Response<ureq::Body>, max_body: usize) -> Result<Reply, NetError> {
    let status = resp.status().as_u16();
    let headers = resp.headers().iter().filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string()))).collect();
    let body = resp.body_mut().with_config().limit(max_body as u64).read_to_vec().map_err(|e| match e {
        ureq::Error::BodyExceedsLimit(_) => NetError::TooBig,
        e => NetError::Unreachable(e.to_string()),
    })?;
    Ok(Reply { status, headers, body })
}

fn map_err(e: ureq::Error) -> NetError {
    match e {
        ureq::Error::BadUri(_) | ureq::Error::Http(_) => NetError::BadUrl,
        e => NetError::Unreachable(e.to_string()),
    }
}

/// GET `url`, reading at most `max_body` bytes of the answer.
pub fn get(url: &str, headers: &[(&str, String)], t: Timeouts, max_body: usize) -> Result<Reply, NetError> {
    parse_address(url).ok_or(NetError::BadUrl)?;
    let mut req = agent(t).get(url);
    for (k, v) in headers {
        req = req.header(*k, v.as_str());
    }
    finish(req.call().map_err(map_err)?, max_body)
}

/// POST `body` to `url`.
pub fn post(url: &str, headers: &[(&str, String)], body: &[u8], t: Timeouts, max_body: usize) -> Result<Reply, NetError> {
    parse_address(url).ok_or(NetError::BadUrl)?;
    let mut req = agent(t).post(url);
    for (k, v) in headers {
        req = req.header(*k, v.as_str());
    }
    finish(req.send(body).map_err(map_err)?, max_body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A server on this computer that answers every request with `answer` and returns the request it got.
    pub(crate) fn serve_once(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 8192];
            // the head and the body may arrive apart: read until the client goes quiet
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            s.write_all(answer.as_bytes()).unwrap();
            String::from_utf8_lossy(&got).into_owned()
        });
        (url, h)
    }

    #[test]
    fn addresses_are_read() {
        assert_eq!(parse_address("https://pdi.example.com/api"), Some(Address { secure: true, host: "pdi.example.com".into() }));
        assert_eq!(parse_address("http://localhost:8787/x").map(|a| a.host), Some("localhost".into()));
        assert_eq!(parse_address("http://[::1]:80/").map(|a| a.host), Some("::1".into()));
        assert_eq!(parse_address("ftp://x"), None);
        assert_eq!(parse_address("https://"), None);
    }

    #[test]
    fn a_get_reads_status_headers_and_body() {
        let (url, h) = serve_once("HTTP/1.1 429 Too Many\r\nRetry-After: 7\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi");
        let r = get(&url, &[("Accept", "text/plain".into())], Timeouts::secs(2, 5), 100).unwrap();
        assert_eq!((r.status, r.body.as_slice(), r.header("retry-after")), (429, &b"hi"[..], Some("7")));
        assert!(h.join().unwrap().to_lowercase().contains("accept: text/plain"));
    }

    #[test]
    fn a_big_answer_is_refused() {
        let (url, _h) = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789");
        assert_eq!(get(&url, &[], Timeouts::secs(2, 5), 4).unwrap_err(), NetError::TooBig);
    }

    #[test]
    fn a_post_sends_its_body() {
        let (url, h) = serve_once("HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let r = post(&url, &[("Content-Type", "application/json".into())], b"{\"a\":1}", Timeouts::secs(2, 5), 100).unwrap();
        assert_eq!(r.status, 201);
        let req = h.join().unwrap();
        assert!(req.starts_with("POST ") && req.contains("{\"a\":1}"));
    }

    #[test]
    fn nothing_listening_is_unreachable() {
        assert!(matches!(get("http://127.0.0.1:1/", &[], Timeouts::secs(1, 2), 10), Err(NetError::Unreachable(_))));
    }
}
