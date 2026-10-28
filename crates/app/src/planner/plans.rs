//! Plans and lining up.
//!
//! - A **plan** is a shot kept for later: the place, the day and time shown, the target if there is one, a name,
//!   notes and a reminder. Plans show as points on the Map; a click there opens the plan here. Each can go to the
//!   system calendar as an `.ics` event (with the day's light in its notes and an alarm before it).
//! - **Lining up** (as in the photographer's planning apps): a target point on the map (right-click it, or the
//!   target button), how far and in which direction it is, and the days and times the Sun or the Moon stand right
//!   behind it.

use light_studio_astro::align::{self, Sight};

use super::*;

/// How many plans are kept.
pub(super) const MAX_PLANS: usize = 300;
/// How long a plan's calendar event lasts (minutes).
const EVENT_MIN: f64 = 60.0;
/// The reminders offered (minutes before; 0 = none).
pub(super) const ALARMS: [(u32, &str); 6] = [(0, "No reminder"), (15, "15 min"), (30, "30 min"), (60, "1 h"), (120, "2 h"), (1440, "1 day")];

/// A shot kept for later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Plan {
    /// Unique among the plans (milliseconds when it was made).
    pub id: u64,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// The place's name, if it had one.
    pub place: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// Minutes after local midnight.
    pub minute: f64,
    /// As in [`PlannerState::utc_offset`]: `None` follows the place.
    pub utc_offset: Option<i32>,
    pub target: Option<[f64; 2]>,
    pub target_height: f64,
    pub notes: String,
    /// Remind this many minutes before (0: no reminder).
    pub alarm: u32,
    /// How the shot is framed (direction, tilt, focal length), when it was kept with the viewfinder showing.
    pub frame: Option<super::viewfinder::FrameShot>,
}

impl Default for Plan {
    fn default() -> Self {
        Plan {
            id: 0,
            name: String::new(),
            lat: 0.0,
            lon: 0.0,
            place: String::new(),
            date: String::new(),
            minute: 0.0,
            utc_offset: None,
            target: None,
            target_height: 0.0,
            notes: String::new(),
            alarm: 60,
            frame: None,
        }
    }
}

impl Plan {
    /// The planner as it was when the plan was kept (for working out its day).
    fn state(&self, base: &PlannerState) -> PlannerState {
        let mut st = base.clone();
        st.lat = self.lat;
        st.lon = self.lon;
        st.place = self.place.clone();
        st.date = self.date.clone();
        st.minute = Some(self.minute);
        st.utc_offset = self.utc_offset;
        st.target = self.target;
        st.target_height = self.target_height;
        st
    }

    /// Whether the numbers read from a file make a plan.
    pub(super) fn is_sane(&self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && self.lat.abs() <= 90.0
            && self.lon.abs() <= 180.0
            && self.minute.is_finite()
            && Date::parse(&self.date).is_some()
            && self.target.is_none_or(|[a, b]| a.is_finite() && b.is_finite() && a.abs() <= 90.0 && b.abs() <= 180.0)
    }

    /// Clips what a file may have made too long or out of range.
    pub(super) fn tidy(&mut self) {
        self.name = self.name.chars().take(120).collect();
        self.notes = self.notes.chars().take(4000).collect();
        self.minute = self.minute.clamp(0.0, 1439.0);
        self.alarm = self.alarm.min(525_600);
        if !self.target_height.is_finite() {
            self.target_height = 0.0;
        }
        self.target_height = self.target_height.clamp(-500.0, 9000.0);
        if self.utc_offset.is_some_and(|o| !(-720..=840).contains(&o)) {
            self.utc_offset = None;
        }
    }
}

/// The sight line to the target from the place, if there is a target. `ground`: the ground heights at the camera and
/// at the target, when known (else both are taken as level); the target's top is `target_height` above its ground and
/// the eye [`align::EYE`] above the camera's.
pub(super) fn sight_of(st: &PlannerState, ground: Option<(f64, f64)>) -> Option<Sight> {
    let (gc, gt) = ground.unwrap_or((0.0, 0.0));
    let rise = gt + st.target_height - (gc + align::EYE);
    st.target.map(|[la, lo]| align::sight(st.lat, st.lon, la, lo, rise))
}

pub(super) fn distance_label(m: f64) -> String {
    if m < 1000.0 {
        format!("{m:.0} m")
    } else if m < 10_000.0 {
        format!("{:.2} km", m / 1000.0)
    } else {
        format!("{:.1} km", m / 1000.0)
    }
}

/// A new plan id: the time in milliseconds, made unique among `plans`.
fn new_id(plans: &[Plan]) -> u64 {
    let mut id = (now_unix() * 1000.0).max(1.0) as u64;
    while plans.iter().any(|p| p.id == id) {
        id += 1;
    }
    id
}

/// Opens the plan with this id in the planner (place, day, time, target). False when there is none.
pub fn open_plan(app: &mut App, id: u64) -> bool {
    let Some(p) = app.ui.planner.plans.iter().find(|p| p.id == id).cloned() else { return false };
    let st = &mut app.ui.planner;
    set_place(st, p.lat, p.lon, p.place.clone());
    st.date = p.date.clone();
    st.minute = Some(p.minute);
    st.utc_offset = p.utc_offset;
    st.target = p.target;
    st.target_height = p.target_height;
    st.open_plan = Some(id);
    if let Some(frame) = p.frame {
        frame.apply(st);
    }
    if let Some([la, lo]) = p.target {
        // both ends in sight
        st.map.lat = (p.lat + la) / 2.0;
        st.map.lon = (p.lon + lo) / 2.0;
    }
    true
}

// ---- the calendar event -----------------------------------------------------------------------

/// Text for an `.ics` property value: backslashes, commas, semicolons and line ends escaped.
fn ics_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// A content line folded at 75 bytes (never inside a character), as RFC 5545 asks, ended with CRLF.
fn ics_line(out: &mut String, line: &str) {
    let mut used = 0;
    for c in line.chars() {
        let n = c.len_utf8();
        if used + n > 75 {
            out.push_str("\r\n ");
            used = 1;
        }
        out.push(c);
        used += n;
    }
    out.push_str("\r\n");
}

/// `YYYYMMDDTHHMMSSZ` for a Unix time.
fn ics_utc(unix: f64) -> String {
    let secs = unix.floor() as i64;
    let d = Date::from_unix(secs as f64, 0);
    let s = secs.rem_euclid(86_400);
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", d.year, d.month, d.day, s / 3600, s % 3600 / 60, s % 60)
}

/// The plan as a calendar file: one event at the plan's time with the day's light in its notes and the reminder.
pub(super) fn ics(plan: &Plan, base: &PlannerState, now: f64) -> String {
    let st = plan.state(base);
    let c = compute(&st, now);
    let start = c.start + plan.minute * 60.0;
    let p = &c.plan;
    let tr = |s: &str| crate::i18n::tr(s).to_string();
    let at = |t: Option<f64>| t.map_or_else(|| "—".to_string(), |t| clock(t, c.offset));
    let span = |s: Option<light_studio_astro::Span>| {
        s.map_or_else(|| "—".to_string(), |s| format!("{}–{}", clock(s.start, c.offset), clock(s.end, c.offset)))
    };
    let mut notes = Vec::new();
    if !plan.notes.trim().is_empty() {
        notes.push(plan.notes.trim().to_string());
        notes.push(String::new());
    }
    notes.push(format!("{}: {} · {}: {}", tr("Sunrise"), at(p.sun.rise), tr("Sunset"), at(p.sun.set)));
    notes.push(format!("{}: {} / {}", tr("Golden hour"), span(p.sun.golden_am), span(p.sun.golden_pm)));
    notes.push(format!("{}: {} / {}", tr("Blue hour"), span(p.sun.blue_am), span(p.sun.blue_pm)));
    let phase = moon_phase(start);
    notes.push(format!(
        "{}: {} ({:.0} %) · {}: {} · {}: {}",
        tr("Moon"),
        tr(phase.name.label()),
        phase.illumination * 100.0,
        tr("Moonrise"),
        at(p.moon.rise),
        tr("Moonset"),
        at(p.moon.set)
    ));
    if let Some(w) = c.night_core.first() {
        notes.push(format!("{}: {}–{}", tr("Milky Way core"), clock(w.start, c.offset), clock(w.end, c.offset)));
    }
    if let Some(s) = sight_of(&st, None) {
        notes.push(format!("{}: {} · {}", tr("Target"), distance_label(s.distance), az_text(s.azimuth)));
    }
    notes.push(format!("{:.5}, {:.5}", plan.lat, plan.lon));
    let location = if plan.place.trim().is_empty() { format!("{:.5}, {:.5}", plan.lat, plan.lon) } else { plan.place.trim().to_string() };
    let name = if plan.name.trim().is_empty() { tr("Photo plan") } else { plan.name.trim().to_string() };

    let mut out = String::new();
    for l in ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//Light Studio//Planner//EN", "CALSCALE:GREGORIAN", "METHOD:PUBLISH", "BEGIN:VEVENT"] {
        ics_line(&mut out, l);
    }
    ics_line(&mut out, &format!("UID:plan-{}@raw-studio", plan.id));
    ics_line(&mut out, &format!("DTSTAMP:{}", ics_utc(now)));
    ics_line(&mut out, &format!("DTSTART:{}", ics_utc(start)));
    ics_line(&mut out, &format!("DTEND:{}", ics_utc(start + EVENT_MIN * 60.0)));
    ics_line(&mut out, &format!("SUMMARY:{}", ics_text(&name)));
    ics_line(&mut out, &format!("LOCATION:{}", ics_text(&location)));
    ics_line(&mut out, &format!("GEO:{:.6};{:.6}", plan.lat, plan.lon));
    ics_line(&mut out, &format!("DESCRIPTION:{}", ics_text(&notes.join("\n"))));
    ics_line(&mut out, &format!("URL:https://maps.apple.com/?ll={:.6},{:.6}", plan.lat, plan.lon));
    if plan.alarm > 0 {
        for l in ["BEGIN:VALARM", "ACTION:DISPLAY"] {
            ics_line(&mut out, l);
        }
        ics_line(&mut out, &format!("DESCRIPTION:{}", ics_text(&name)));
        ics_line(&mut out, &format!("TRIGGER:-PT{}M", plan.alarm));
        ics_line(&mut out, "END:VALARM");
    }
    for l in ["END:VEVENT", "END:VCALENDAR"] {
        ics_line(&mut out, l);
    }
    out
}

/// A file name for the plan's event (letters, digits and dashes).
fn ics_file_name(plan: &Plan) -> String {
    let base: String = plan
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base: String = base.chars().take(48).collect();
    let base = if base.is_empty() { "plan".to_string() } else { base };
    format!("{base}-{}-{}.ics", plan.date, plan.id)
}

/// Writes the plan's event and hands it to the calendar app (Calendar on macOS, the default app elsewhere).
pub(super) fn send_to_calendar(app: &mut App, ctx: &egui::Context, id: u64) {
    let Some(plan) = app.ui.planner.plans.iter().find(|p| p.id == id).cloned() else { return };
    let text = ics(&plan, &app.ui.planner, now_unix());
    let Some(dir) = crate::paths::config_dir().map(|d| d.join("plans")) else {
        app.toast(ctx, crate::i18n::tr("There is nowhere to write the calendar event").to_string());
        return;
    };
    let path = dir.join(ics_file_name(&plan));
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, text)) {
        app.toast(ctx, format!("{}: {e}", path.display()));
        return;
    }
    let shown = path.to_string_lossy().to_string();
    match open_in_calendar(&shown) {
        Ok(()) => app.toast(ctx, crate::i18n::tr("Sent to the Calendar: choose the calendar to add it to").to_string()),
        Err(e) => app.toast(ctx, crate::i18n::tr("Calendar event saved: {path}").replace("{path}", &format!("{shown} ({e})"))),
    }
}

/// Hands the event file to the calendar app: Calendar on a Mac, the default program elsewhere.
fn open_in_calendar(path: &str) -> Result<(), String> {
    use std::process::Command;
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.args(["-a", "Calendar", path]);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", path]);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(path);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

impl Page<'_> {
    /// A new plan at a spot of the map, for the planner's day and time, with its record open for editing.
    pub(super) fn new_plan_at(&self, st: &mut PlannerState, rt: &mut Runtime, lat: f64, lon: f64, site: Option<&str>) {
        let site = site.map(str::trim).filter(|s| !s.is_empty());
        let name = match site {
            Some(s) => format!("{s} · {}", short_date(self.c.date)),
            None => format!("{lat:.3}°, {lon:.3}° · {}", short_date(self.c.date)),
        };
        let mut plan = Plan {
            id: new_id(&st.plans),
            name,
            lat,
            lon,
            place: site.unwrap_or_default().to_string(),
            date: self.c.date.iso(),
            minute: self.minute.clamp(0.0, 1439.0).round(),
            utc_offset: st.utc_offset,
            target: st.target,
            target_height: st.target_height,
            notes: String::new(),
            alarm: st.alarm,
            frame: st.show_viewfinder.then(|| super::viewfinder::FrameShot::of(st)),
        };
        plan.tidy();
        st.open_plan = Some(plan.id);
        rt.plan_popup = Some(plan.id);
        rt.site_open = None;
        rt.edit_of = Some(super::popup::Shown::Plan(plan.id));
        st.plans.push(plan);
        if st.plans.len() > MAX_PLANS {
            st.plans.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Plan {
            id: 7,
            name: "Faro, al atardecer; con niebla".into(),
            lat: 43.39,
            lon: -1.79,
            place: "Hondarribia".into(),
            date: "2026-10-18".into(),
            minute: 19.0 * 60.0 + 5.0,
            utc_offset: Some(120),
            notes: "Llevar trípode\nmarea baja".into(),
            alarm: 90,
            ..Plan::default()
        }
    }

    #[test]
    fn a_plan_becomes_a_calendar_event_with_its_reminder() {
        let base = PlannerState::default();
        let text = ics(&plan(), &base, 1_760_000_000.0);
        assert!(text.starts_with("BEGIN:VCALENDAR\r\n") && text.ends_with("END:VCALENDAR\r\n"));
        // 19:05 at UTC+2 is 17:05 UTC
        assert!(text.contains("DTSTART:20261018T170500Z\r\n"), "{text}");
        assert!(text.contains("DTEND:20261018T180500Z\r\n"));
        assert!(text.contains("SUMMARY:Faro\\, al atardecer\\; con niebla\r\n"));
        assert!(text.contains("TRIGGER:-PT90M\r\n"));
        assert!(text.contains("GEO:43.390000;-1.790000"));
        assert!(text.contains("Llevar trípode\\nmarea baja"));
        // folded lines stay within 75 bytes
        assert!(text.split("\r\n").all(|l| l.len() <= 75), "{text}");
        // no reminder, no alarm
        let quiet = ics(&Plan { alarm: 0, ..plan() }, &base, 1_760_000_000.0);
        assert!(!quiet.contains("VALARM"));
    }

    #[test]
    fn folding_never_splits_a_character() {
        let mut out = String::new();
        ics_line(&mut out, &format!("DESCRIPTION:{}", "ñ".repeat(100)));
        for l in out.split("\r\n") {
            assert!(l.len() <= 75);
        }
        assert_eq!(out.replace("\r\n ", "").trim_end(), format!("DESCRIPTION:{}", "ñ".repeat(100)));
    }

    #[test]
    fn the_file_name_is_plain() {
        assert_eq!(ics_file_name(&plan()), "Faro-al-atardecer-con-niebla-2026-10-18-7.ics");
        assert_eq!(ics_file_name(&Plan { name: "../..".into(), ..plan() }), "plan-2026-10-18-7.ics");
    }

    #[test]
    fn plans_read_from_a_bad_file_are_dropped_or_tidied() {
        assert!(plan().is_sane());
        assert!(!Plan { lat: f64::NAN, ..plan() }.is_sane());
        assert!(!Plan { date: "mañana".into(), ..plan() }.is_sane());
        assert!(!Plan { target: Some([95.0, 0.0]), ..plan() }.is_sane());
        let mut p = Plan { minute: 5000.0, alarm: u32::MAX, target_height: f64::INFINITY, utc_offset: Some(9999), ..plan() };
        p.tidy();
        assert_eq!((p.minute, p.alarm, p.target_height, p.utc_offset), (1439.0, 525_600, 0.0, None));
    }

    #[test]
    fn opening_a_plan_sets_the_planner_on_it() {
        let mut app = super::super::tests::new_app();
        app.ui.planner.plans.push(Plan { target: Some([43.40, -1.80]), target_height: 30.0, ..plan() });
        assert!(open_plan(&mut app, 7));
        let st = &app.ui.planner;
        assert_eq!((st.lat, st.lon, st.date.as_str(), st.minute, st.target_height), (43.39, -1.79, "2026-10-18", Some(1145.0), 30.0));
        assert_eq!(st.open_plan, Some(7));
        assert!(!open_plan(&mut app, 8));
    }
}
