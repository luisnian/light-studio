//! Language. The interface is written in English; Spanish is a table from each English text to its translation
//! (`locales/es.json`). A text with no translation is shown as written. Words that depend on where they are used
//! (months, weekdays, compass points) are looked up with a context: the table key is `context|text`.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use light_studio_astro::time::Date;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Spanish,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Spanish];

    /// BCP-47 code.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Spanish => "es",
        }
    }

    /// Its own name.
    pub fn name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Spanish => "Español",
        }
    }

    pub fn parse(code: &str) -> Option<Language> {
        let code = code.trim().to_ascii_lowercase();
        let base = code.split(['-', '_', '.']).next().unwrap_or("");
        match base {
            "en" => Some(Language::English),
            "es" => Some(Language::Spanish),
            _ => None,
        }
    }

    /// The system's language, English when it is not one of ours.
    pub fn from_system() -> Language {
        ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find_map(|v| Language::parse(&v)).unwrap_or_default()
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn language() -> Language {
    if CURRENT.load(Ordering::Relaxed) == 1 { Language::Spanish } else { Language::English }
}

pub fn set_language(language: Language) {
    CURRENT.store(u8::from(language == Language::Spanish), Ordering::Relaxed);
}

fn spanish() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| match serde_json::from_str(include_str!("../locales/es.json")) {
        Ok(table) => table,
        Err(e) => {
            log::error!("the Spanish table is not valid: {e}");
            HashMap::new()
        }
    })
}

/// `text` in the language in use.
pub fn tr(text: &str) -> &str {
    if language() == Language::Spanish
        && let Some(found) = spanish().get(text)
    {
        return found.as_str();
    }
    text
}

/// `text` as used in `context` ("month", "weekday", "compass"…).
pub fn tr_ctx<'a>(context: &str, text: &'a str) -> &'a str {
    if language() == Language::Spanish
        && let Some(found) = spanish().get(&format!("{context}|{text}"))
    {
        return found.as_str();
    }
    text
}

/// A translated template with its `{name}` holes filled: `tr_format!("{n} files", n = 3)`.
#[macro_export]
macro_rules! tr_format {
    ($template:literal $(, $name:ident = $value:expr)* $(,)?) => {{
        #[allow(unused_mut)]
        let mut text = $crate::i18n::tr($template).to_string();
        $( text = text.replace(concat!("{", stringify!($name), "}"), &$value.to_string()); )*
        text
    }};
}
pub use tr_format;

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// A day written out from its ISO form: "Tue 27 Oct 2026" (with the weekday when `weekday` is set).
pub fn date_group_label(iso: &str, weekday: bool) -> String {
    let Some(d) = Date::parse(iso) else { return iso.to_string() };
    let month = tr_ctx("month", MONTHS.get((d.month as usize).wrapping_sub(1)).copied().unwrap_or(""));
    let day = format!("{} {} {}", d.day, month, d.year);
    if weekday {
        let wd = tr_ctx("weekday", WEEKDAYS.get(d.weekday() as usize).copied().unwrap_or(""));
        format!("{wd} {day}")
    } else {
        day
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_are_read_from_codes() {
        assert_eq!(Language::parse("es_ES.UTF-8"), Some(Language::Spanish));
        assert_eq!(Language::parse("en-GB"), Some(Language::English));
        assert_eq!(Language::parse("de"), None);
    }

    #[test]
    fn holes_are_filled_and_days_are_written_out() {
        set_language(Language::English);
        assert_eq!(tr_format!("{n} of {m}", n = 2, m = 5), "2 of 5");
        assert_eq!(date_group_label("2026-10-27", false), "27 Oct 2026");
        assert_eq!(date_group_label("2026-10-27", true), "Tue 27 Oct 2026");
        assert_eq!(date_group_label("nonsense", true), "nonsense");
        set_language(Language::Spanish);
        assert_eq!(tr("no such text in the table"), "no such text in the table");
        assert_eq!(tr_ctx("weekday", "Tue"), "mar");
        assert_eq!(tr_ctx("compass", "W"), "O");
        set_language(Language::English);
        assert_eq!(tr_ctx("compass", "W"), "W");
    }
}
