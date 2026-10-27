//! Where Light Studio keeps what it saves on this computer: settings, tile caches and the photos that came from the phone.

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Clone, Copy)]
enum Platform {
    Mac,
    Windows,
    Other,
}

/// The settings folder. `LIGHT_STUDIO_CONFIG_DIR` replaces it (an empty value means no folder: nothing is saved).
pub fn config_dir() -> Option<PathBuf> {
    let platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Other
    };
    resolve(platform, |name| std::env::var_os(name))
}

fn resolve(platform: Platform, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = env("LIGHT_STUDIO_CONFIG_DIR") {
        return (!dir.is_empty()).then(|| PathBuf::from(dir));
    }
    match platform {
        Platform::Mac => env("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/Light Studio")),
        Platform::Windows => env("APPDATA").map(|a| PathBuf::from(a).join("Light Studio")),
        Platform::Other => env("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join("light-studio")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(platform: Platform, vars: &'static [(&'static str, &'static str)]) -> Option<PathBuf> {
        resolve(platform, |n| vars.iter().find(|(k, _)| *k == n).map(|(_, v)| OsString::from(v)))
    }

    #[test]
    fn each_system_has_its_place() {
        assert_eq!(with(Platform::Mac, &[("HOME", "/Users/a")]), Some("/Users/a/Library/Application Support/Light Studio".into()));
        assert_eq!(with(Platform::Windows, &[("APPDATA", "C:\\A")]), Some(PathBuf::from("C:\\A").join("Light Studio")));
        assert_eq!(with(Platform::Other, &[("HOME", "/h")]), Some("/h/.config/light-studio".into()));
        assert_eq!(with(Platform::Other, &[("XDG_CONFIG_HOME", "/x"), ("HOME", "/h")]), Some("/x/light-studio".into()));
    }

    #[test]
    fn the_override_wins_and_empty_means_none() {
        assert_eq!(with(Platform::Mac, &[("LIGHT_STUDIO_CONFIG_DIR", "/t"), ("HOME", "/h")]), Some("/t".into()));
        assert_eq!(with(Platform::Mac, &[("LIGHT_STUDIO_CONFIG_DIR", ""), ("HOME", "/h")]), None);
    }
}
