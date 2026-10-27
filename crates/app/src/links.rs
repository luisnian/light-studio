//! Opening an address in the system's browser.

use std::process::Command;

/// Only web addresses are opened.
pub fn allowed(url: &str) -> bool {
    let url = url.trim();
    (url.starts_with("https://") || url.starts_with("http://")) && !url.contains(char::is_whitespace)
}

/// Opens `url` with the system's default handler.
pub fn open(url: &str) -> Result<(), String> {
    if !allowed(url) {
        return Err(crate::i18n::tr("That address cannot be opened").to_string());
    }
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler").arg(url);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| format!("{}: {e}", crate::i18n::tr("Could not open the browser")))
}

#[cfg(test)]
mod tests {
    use super::allowed;

    #[test]
    fn only_web_addresses() {
        assert!(allowed("https://maps.apple.com/?daddr=1,2"));
        assert!(allowed("http://localhost:8787"));
        assert!(!allowed("file:///etc/passwd"));
        assert!(!allowed("javascript:alert(1)"));
        assert!(!allowed("https://a b"));
    }
}
