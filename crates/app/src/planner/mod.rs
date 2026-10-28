//! The planner: when to shoot. For a place and a day it shows where the Sun, the Moon and the Milky Way's core are hour by
//! hour, the golden and blue hours, the twilights, the Moon's phase, the Milky Way calendar of the year, which meteor
//! showers are close, and the lens arithmetic that goes with it (depth of field, star-trail exposure, angle of view).
//!
//! - The chart plots the three altitudes over the day on the twilight colours; click or drag it to move the time. The
//!   sky dial beside it shows the same moment from above: the paths across the sky, where each body rises and sets, and
//!   where each one is now.
//! - The place comes from a name search, from the centre of the map, from where this computer is, or typed as latitude
//!   and longitude. The UTC offset is guessed for the place and can be set by hand.
//! - All the sums live in [`light_studio_astro`]; this module only draws them.

mod bodies;
mod calc;
mod camtool;
mod compass;
mod coords;
mod eclipses;
mod find;
mod horizon;
mod infopanel;
mod layout;
mod lineup;
mod map;
mod phone;
mod places;
mod plans;
mod pois;
pub use pois::Gone;
mod popup;
mod shade;
mod skycal;
mod skylayers;
mod strip;
mod tools;
mod year;

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Align, Align2, Color32, Margin, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use light_studio_astro::moon::{Principal, moon_phase, next_phase};
use light_studio_astro::optics::{self, SENSORS};
use light_studio_astro::plan::{
    ASTRONOMICAL, CIVIL, DayPlan, GOLDEN_BOTTOM, GOLDEN_TOP, NAUTICAL, NightProfile, Skies, SunState, night_profile, plan_day, skies_at,
};
use light_studio_astro::sky::SHOWERS;
use light_studio_astro::sun::{Horizontal, Observer};
use light_studio_astro::time::{DAY, Date, auto_utc_offset_min};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use layout::Tab;
pub use places::SiteStatus;
pub use plans::{Plan, open_plan};
pub use skylayers::LineStyle;

use crate::App;
use crate::date_picker::{DatePicker, PickedDate, Precision};
use crate::icons::Icon;
use crate::text_field::TextField;
use crate::theme::{R_CARD, R_CONTROL, Tokens};
use crate::widgets::{icon_button, text_button};

/// The chart and the dial sample the day every ten minutes.
const SAMPLE_STEP: f64 = 600.0;
const SAMPLES: usize = 145;
/// Third-stop f-numbers the aperture stepper walks through.
const F_STOPS: [f64; 31] = [
    1.0, 1.2, 1.4, 1.6, 1.8, 2.0, 2.2, 2.5, 2.8, 3.2, 3.5, 4.0, 4.5, 5.0, 5.6, 6.3, 7.1, 8.0, 9.0, 10.0, 11.0, 13.0, 14.0, 16.0, 18.0, 20.0, 22.0,
    25.0, 29.0, 32.0, 45.0,
];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// What the map shows. Each mode draws only its own things and has its own icons on the map's rail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MapMode {
    Pois,
    Plans,
    #[default]
    Planner,
}

pub(super) const MAP_MODES: [(MapMode, &str, &str); 3] =
    [(MapMode::Pois, "pois", "Points of Interest"), (MapMode::Plans, "plans", "Plans"), (MapMode::Planner, "planner", "Planner")];

impl MapMode {
    pub(super) fn icon(self) -> Icon {
        match self {
            MapMode::Pois => Icon::Pin,
            MapMode::Plans => Icon::Calendar,
            MapMode::Planner => Icon::Planner,
        }
    }
}

/// What the view remembers between sessions: the place, the day, the camera.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlannerState {
    /// Where the last download of the phone's points of interest ended ("" = from the start).
    pub pdi_cursor: String,
    /// Points deleted here that the Worker still has: its version of each when it was last confirmed.
    pub pdi_deleted: Vec<pois::Gone>,
    /// A place was chosen (otherwise the first showing keeps the default place).
    pub placed: bool,
    pub lat: f64,
    pub lon: f64,
    /// The name of the place, when it came from a search.
    pub place: String,
    /// The day as `YYYY-MM-DD`; empty is today.
    pub date: String,
    /// Minutes east of UTC; `None` guesses it from the place and day.
    pub utc_offset: Option<i32>,
    /// The time shown, minutes after local midnight; `None` is now (today) or sunset.
    pub minute: Option<f64>,
    /// Which of the Sun, Moon and Milky Way core are drawn.
    pub layers: [bool; 3],
    /// Index into [`SENSORS`].
    pub sensor: usize,
    pub focal: f64,
    pub aperture: f64,
    /// Focus distance, metres.
    pub distance: f64,
    pub megapixels: f64,
    /// Where the planner's map looks (centre and zoom).
    pub map: crate::map::MapView,
    /// Places kept by name, to come back to.
    pub saved: Vec<SavedPlace>,
    /// Time-lapse: the event's length (hours), the clip's (seconds) and its frame rate.
    pub tl_event_h: f64,
    pub tl_clip: f64,
    pub tl_fps: f64,
    /// Neutral density: the shutter time without the filter (seconds) and the filter's stops.
    pub nd_base: f64,
    pub nd_stops: f64,
    /// Star trails: the exposure to check (seconds).
    pub trail_s: f64,
    /// What to line the Sun or the Moon up with (latitude, longitude), and its top's height above the camera (m).
    pub target: Option<[f64; 2]>,
    pub target_height: f64,
    /// Shots kept for later (shown on the Map too).
    pub plans: Vec<Plan>,
    /// The plan last opened or kept (highlighted in the list).
    pub open_plan: Option<u64>,
    /// The reminder new plans get (minutes before; 0: none).
    pub alarm: u32,
    /// The side panel's tab, and whether the panel is hidden (the map alone, with the time bar).
    pub tab: Tab,
    pub panel_hidden: bool,
    /// The time bar spans the whole map instead of its compact width.
    pub time_wide: bool,
    /// The time bar hides itself (a thin handle stays) until the pointer comes to the bottom edge.
    pub time_auto: bool,
    /// What the map shows: only the points of interest (red pins), only the plans (blue pins), or the planner's own
    /// lines and tools.
    pub map_mode: MapMode,
    /// The mountains strip (the skyline all round, with the Sun and the Moon on it) over the map.
    pub show_horizon: bool,
    /// The terrain's shadows at the time shown, over the map.
    pub show_shadows: bool,
    /// David Lorenz's light pollution atlas over the map.
    pub show_light: bool,
    /// How the Sun's and the Moon's lines are drawn on the map.
    pub sun_style: LineStyle,
    pub moon_style: LineStyle,
    /// The Milky Way's arc, the map tinted by the kind of light, the Sun's and Moon's discs as wedges, and lines that
    /// follow the Earth's curve.
    pub show_arc: bool,
    pub show_twilight: bool,
    /// The track of the next solar eclipse over the map.
    pub show_eclipse: bool,
    pub show_size: bool,
    pub long_lines: bool,
    /// The meteor shower (an index into the showers) whose radiant the map shows.
    pub meteor: Option<usize>,
    /// The strip of information panels over the map: shown, its page, and the shower it follows.
    pub strip: bool,
    pub strip_page: usize,
    /// Which section of the information panel is open (none: any number past the last).
    pub info_open: usize,
    pub shower: usize,
    /// Find the dates: the Sun (else the Moon) at an azimuth (and altitude), within a tolerance, over some days.
    pub find_sun: bool,
    pub find_az: f64,
    pub find_alt: f64,
    pub find_use_alt: bool,
    pub find_tol: f64,
    pub find_days: f64,
    /// Calculators: ISO, the neutral density filter's exposure value table, print and viewing sizes, the sharp
    /// range wanted, the subject's size, the other sensor, and how far a star may smear.
    pub iso: f64,
    pub print_cm: f64,
    pub view_cm: f64,
    pub dof_near: f64,
    pub dof_far: f64,
    pub subject_m: f64,
    pub to_sensor: usize,
    pub stars_px: f64,
    /// The camera drawn on the map: nothing, its field of view, or its depth of field (0, 1, 2); where it points
    /// (azimuth, degrees); and whether it is held upright.
    pub cam_tool: u8,
    pub cam_az: f64,
    pub portrait: bool,
    /// A click on the planner's map sets the target (the target button), not the place.
    #[serde(skip)]
    pub target_mode: bool,
}

/// A place kept by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedPlace {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// Its record: want to go or visited, the best time and notes.
    pub status: SiteStatus,
    pub best: String,
    pub notes: String,
    /// What the place is, in a few lines (a point of interest's description).
    pub description: String,
    /// Who it is: a stable id (the position in the list is not one), when it was made and last edited (UTC), how
    /// many times it was edited, and where it came from (`desktop`, `mobile`, `imported`). See [`pois`].
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: u64,
    pub origin: String,
    /// A kind of place (tree, coast, waterfall…), free text.
    pub category: String,
    pub tags: Vec<String>,
    /// Photos kept in the cloud for it (from the phone), carried through untouched.
    pub attachments: Vec<pois::Attachment>,
    /// Synchronisation with the Worker: the version it last confirmed (0 = it does not know the point), whether the
    /// point changed here since, and whether the Worker refused such a change (a conflict, to be settled by taking the
    /// Worker's version).
    pub base: u64,
    pub dirty: bool,
    pub conflict: bool,
}

impl Default for SavedPlace {
    fn default() -> Self {
        SavedPlace {
            name: String::new(),
            lat: 0.0,
            lon: 0.0,
            status: SiteStatus::Want,
            best: String::new(),
            notes: String::new(),
            description: String::new(),
            id: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            version: 0,
            origin: String::new(),
            category: String::new(),
            tags: Vec::new(),
            attachments: Vec::new(),
            base: 0,
            dirty: false,
            conflict: false,
        }
    }
}

/// How many places can be kept (a limit for a damaged file, far above what anyone saves).
const MAX_SAVED: usize = 10_000;

impl Default for PlannerState {
    fn default() -> Self {
        PlannerState {
            placed: false,
            lat: 40.4168,
            lon: -3.7038,
            place: String::new(),
            date: String::new(),
            utc_offset: None,
            minute: None,
            layers: [true; 3],
            sensor: 0,
            focal: 24.0,
            aperture: 2.8,
            distance: 3.0,
            megapixels: 24.0,
            map: crate::map::MapView { lat: 40.4168, lon: -3.7038, zoom: 9.0 },
            saved: Vec::new(),
            tl_event_h: 3.0,
            tl_clip: 20.0,
            tl_fps: 25.0,
            nd_base: 1.0 / 125.0,
            nd_stops: 6.0,
            trail_s: 20.0,
            target: None,
            target_height: 0.0,
            plans: Vec::new(),
            open_plan: None,
            alarm: 60,
            target_mode: false,
            tab: Tab::Sun,
            pdi_cursor: String::new(),
            pdi_deleted: Vec::new(),
            panel_hidden: false,
            time_wide: false,
            time_auto: false,
            map_mode: MapMode::Planner,
            show_horizon: false,
            show_shadows: false,
            show_light: false,
            sun_style: LineStyle::Standard,
            moon_style: LineStyle::Standard,
            show_arc: true,
            show_twilight: false,
            show_eclipse: false,
            show_size: false,
            long_lines: false,
            meteor: None,
            strip: true,
            strip_page: 0,
            info_open: 1,
            shower: 4,
            find_sun: true,
            find_az: 90.0,
            find_alt: 10.0,
            find_use_alt: false,
            find_tol: 2.0,
            find_days: 365.0,
            iso: 100.0,
            print_cm: 60.0,
            view_cm: 100.0,
            dof_near: 2.0,
            dof_far: 10.0,
            subject_m: 1.8,
            to_sensor: 1,
            stars_px: 1.0,
            cam_tool: 0,
            cam_az: 90.0,
            portrait: false,
        }
    }
}

impl PlannerState {
    /// Brings numbers read from a settings file into range.
    fn sane(&mut self) {
        let d = PlannerState::default();
        if !(self.lat.is_finite() && self.lat.abs() <= 90.0) {
            self.lat = d.lat;
        }
        if !(self.lon.is_finite() && self.lon.abs() <= 180.0) {
            self.lon = d.lon;
        }
        if self.utc_offset.is_some_and(|o| !(-720..=840).contains(&o)) {
            self.utc_offset = None;
        }
        if self.minute.is_some_and(|m| !m.is_finite()) {
            self.minute = None;
        }
        if self.sensor >= SENSORS.len() {
            self.sensor = 0;
        }
        let clamp = |v: f64, lo: f64, hi: f64, dflt: f64| if v.is_finite() { v.clamp(lo, hi) } else { dflt };
        self.focal = clamp(self.focal, 1.0, 2000.0, d.focal);
        self.aperture = clamp(self.aperture, 0.7, 64.0, d.aperture);
        self.distance = clamp(self.distance, 0.05, 100_000.0, d.distance);
        self.megapixels = clamp(self.megapixels, 0.5, 400.0, d.megapixels);
        self.tl_event_h = clamp(self.tl_event_h, 0.01, 240.0, d.tl_event_h);
        self.tl_clip = clamp(self.tl_clip, 0.1, 3600.0, d.tl_clip);
        self.tl_fps = clamp(self.tl_fps, 1.0, 240.0, d.tl_fps);
        self.nd_base = clamp(self.nd_base, 1.0 / 32_000.0, 3600.0, d.nd_base);
        self.nd_stops = clamp(self.nd_stops, 0.0, 30.0, d.nd_stops);
        self.trail_s = clamp(self.trail_s, 0.1, 7200.0, d.trail_s);
        self.find_az = clamp(self.find_az, 0.0, 360.0, d.find_az);
        self.find_alt = clamp(self.find_alt, -90.0, 90.0, d.find_alt);
        self.find_tol = clamp(self.find_tol, 0.1, 45.0, d.find_tol);
        self.find_days = clamp(self.find_days, 1.0, 400.0, d.find_days).round();
        self.iso = clamp(self.iso, 6.0, 6_400_000.0, d.iso);
        self.print_cm = clamp(self.print_cm, 2.0, 2000.0, d.print_cm);
        self.view_cm = clamp(self.view_cm, 5.0, 10_000.0, d.view_cm);
        self.dof_near = clamp(self.dof_near, 0.1, 100_000.0, d.dof_near);
        self.dof_far = clamp(self.dof_far, 0.2, 1_000_000.0, d.dof_far);
        self.subject_m = clamp(self.subject_m, 0.001, 10_000.0, d.subject_m);
        self.stars_px = clamp(self.stars_px, 0.1, 50.0, d.stars_px);
        self.cam_az = clamp(self.cam_az, 0.0, 360.0, d.cam_az);
        self.cam_tool = self.cam_tool.min(2);
        if self.to_sensor >= SENSORS.len() {
            self.to_sensor = d.to_sensor;
        }
        self.shower = self.shower.min(SHOWERS.len() - 1);
        self.strip_page = self.strip_page.min(strip::PAGES - 1);
        if self.info_open >= infopanel::SECTIONS.len() {
            self.info_open = usize::MAX;
        }
        if self.meteor.is_some_and(|m| m >= SHOWERS.len()) {
            self.meteor = None;
        }
        self.map = self.map.sane();
        self.saved.retain(|p| p.lat.is_finite() && p.lon.is_finite() && p.lat.abs() <= 90.0 && p.lon.abs() <= 180.0);
        self.saved.truncate(MAX_SAVED);
        if self.target.is_some_and(|[a, b]| !(a.is_finite() && b.is_finite() && a.abs() <= 90.0 && b.abs() <= 180.0)) {
            self.target = None;
        }
        self.target_height = clamp(self.target_height, -500.0, 9000.0, 0.0);
        self.alarm = self.alarm.min(525_600);
        self.plans.retain(Plan::is_sane);
        let extra = self.plans.len().saturating_sub(plans::MAX_PLANS);
        self.plans.drain(..extra);
        for p in &mut self.plans {
            p.tidy();
        }
        // every point has an id of its own (a repeated one, as in a hand-edited file, is made new)
        let mut seen = std::collections::HashSet::new();
        for p in &mut self.saved {
            p.tidy();
            p.ensure_identity();
            if !seen.insert(p.id.clone()) {
                p.id = pois::new_id();
                seen.insert(p.id.clone());
            }
        }
    }
}

/// What the online place search has answered, for the text it was asked about.
#[derive(Default)]
struct Online {
    for_query: String,
    hits: Vec<Found>,
    error: Option<String>,
    running: bool,
}

#[derive(Clone, Debug)]
struct Found {
    name: String,
    detail: String,
    lat: f64,
    lon: f64,
}

/// A day worked out for a place: the plan and the curves the chart draws.
struct Computed {
    key: (u64, u64, i64, i32),
    date: Date,
    /// Minutes east of UTC in force.
    offset: i32,
    /// Unix time of the local midnight the day starts at.
    start: f64,
    plan: DayPlan,
    sun: Vec<Horizontal>,
    moon: Vec<Horizontal>,
    core: Vec<Horizontal>,
    next_new: Option<f64>,
    next_full: Option<f64>,
    /// The night starting this evening, with the core up in a dark sky (one stretch across midnight).
    night_core: Vec<light_studio_astro::Span>,
}

/// What the view keeps between frames (not saved).
#[derive(Default)]
pub struct Runtime {
    /// The photos from the phone, decoded.
    phone: phone::Cache,
    /// The directions the Sun or Moon reaches over the days the Find card looks through, and what they were worked out for
    /// (the body, the place, the first day, the number of days).
    find_reach: Option<((bool, u64, u64, u64, u32), Box<[bool; 360]>)>,
    computed: Option<Arc<Computed>>,
    /// Text being typed in the number fields, by widget id.
    fields: HashMap<String, String>,
    search: String,
    searched: String,
    /// The online place search: when the typing last changed, what was last asked, and the answer (from a thread).
    typed_at: f64,
    online_asked: String,
    online: std::sync::Arc<std::sync::Mutex<Online>>,
    /// Coordinates being typed.
    coords: String,
    found: Vec<Found>,
    search_note: Option<String>,
    /// The nights of the year of the day shown, for the year chart, by place, year and offset.
    year: Option<(YearKey, Arc<Vec<NightProfile>>)>,
    /// The Moon's principal phases of the year the calendar shows, by year and offset.
    year_phases: Option<((i32, i32), Vec<(f64, Principal)>)>,
    /// The year and month the calendar was last scrolled to.
    cal_scrolled: Option<(i32, u32)>,
    /// Whether the Sun / Moon / Night menu is open.
    sky_menu: bool,
    /// Whether the calculations card (exposure and focus, time-lapse, filters, star trails) is open over the map.
    calc_open: bool,
    /// Whether the Find card (the Sun or the Moon at a direction) is open over the map, the zoom of its mini-map, and what the
    /// dates shown were looked for (and whether for the Moon).
    find_open: bool,
    find_zoom: f64,
    found_label: String,
    found_moon: bool,
    /// How the table is sorted (see find.rs), backwards or not, the order of the hits and what it was made for.
    found_sort: u8,
    found_desc: bool,
    found_order: Vec<usize>,
    found_sorted_by: Option<(u8, bool, usize)>,
    /// A card (calculations or Find) was open last frame: it is what closed the side panel, which stays closed after it.
    calc_seen: bool,
    /// The section of the calculations card shown.
    calc_section: usize,
    /// Where the map's context menu was opened (latitude, longitude).
    menu_at: Option<(f64, f64)>,
    /// The ground between the place and the target (its heights), and the pair being looked up.
    terrain: Option<lineup::Terrain>,
    terrain_pending: Option<lineup::TerrainKey>,
    /// The skyline round the place (the mountains strip), the place being looked up, and where the strip looks.
    horizon: Option<horizon::Horizon>,
    horizon_pending: Option<horizon::HorizonKey>,
    pano_az: Option<f32>,
    pano_fov: f32,
    /// The ground heights for the shadow map, the place being looked up, and the picture drawn.
    shade_heights: Option<shade::Heights>,
    shade_pending: Option<shade::ShadeKey>,
    shade_picture: Option<shade::Picture>,
    /// The site whose record is open (an index into `saved`).
    site_open: Option<usize>,
    /// The plan whose record is open in front of the map.
    plan_popup: Option<u64>,
    /// The record being edited (a record opens to be read; the Edit button makes it writable).
    edit_of: Option<popup::Shown>,
    /// Whether the map's layers panel is open, and the eclipse whose times are open in the list.
    layers_open: bool,
    /// Whether the map's mode menu is open.
    mode_menu: bool,
    eclipse_open: Option<usize>,
    /// The dates found for the Sun or Moon at a direction, and the eclipses coming, for the place and day they were worked out for.
    found_dates: Option<Vec<light_studio_astro::search::Hit>>,
    eclipse_list: Option<(EclipseKey, Arc<eclipses::Eclipses>)>,
    /// The shadow's track of the solar eclipse nearest the time shown, by the day it was worked out from.
    eclipse_track: Option<(i64, Option<Arc<light_studio_astro::eclipse::EclipsePath>>)>,
    /// The Moon's perigees and apogees round the day shown, by year.
    apsides: Option<(i32, Vec<light_studio_astro::info::Apsis>)>,
    /// The alignments found, for (place, target direction and height, first day).
    align: Option<((u64, u64, u64, u64, i64), Vec<light_studio_astro::Alignment>)>,
}

/// What the eclipse list was worked out for: latitude and longitude bits, and the day it starts.
type EclipseKey = (u64, u64, i64);

/// What the year chart was worked out for: latitude and longitude bits, the year, and the offset in force (`None`: guessed per day).
type YearKey = (u64, u64, i32, Option<i32>);

/// What the page asks the app to do once it has been drawn.
enum Intent {
    /// Look for the points of interest noted on the phone.
    SyncPois,
    /// Open the settings window.
    OpenSettings,
    UseMapCentre,
    Choose(Found),
    JumpTo(Date),
    /// A day and a time (minutes after local midnight), from the alignment list.
    JumpToTime(Date, f64),
    OpenPlan(u64),
    /// A plan's place, day and time on the Planner map (its card closes).
    ShowPlanInPlanner(u64),
    /// A site's record (Sites tab), a web page.
    OpenSite(usize),
    OpenUrl(String),
    SendToCalendar(u64),
    /// Switch every map between satellite and the plain map.
    ToggleSatellite,
    /// Put the place where this computer is (approximately, by IP address).
    LocateMe,
    /// Keep the planner's place as a point of interest and open its record.
    SavePoi,
    /// The list of points of interest has no room for another.
    Full,
}

/// The body colours, tuned for a dark or a light window.
#[derive(Clone, Copy)]
struct Pal {
    sun: Color32,
    golden: Color32,
    blue: Color32,
    moon: Color32,
    core: Color32,
}

fn palette(dark: bool) -> Pal {
    if dark {
        Pal {
            sun: Color32::from_rgb(255, 196, 64),
            golden: Color32::from_rgb(255, 150, 50),
            blue: Color32::from_rgb(96, 150, 255),
            moon: Color32::from_rgb(196, 210, 236),
            core: Color32::from_rgb(198, 134, 255),
        }
    } else {
        Pal {
            sun: Color32::from_rgb(214, 130, 0),
            golden: Color32::from_rgb(222, 120, 20),
            blue: Color32::from_rgb(44, 100, 224),
            moon: Color32::from_rgb(84, 106, 152),
            core: Color32::from_rgb(142, 72, 214),
        }
    }
}

// ---- time and number formatting --------------------------------------------------------------

fn now_unix() -> f64 {
    use web_time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// `HH:MM` on the wall clock of a place `offset` minutes east of UTC.
fn clock(unix: f64, offset: i32) -> String {
    let local = unix + f64::from(offset) * 60.0;
    if !local.is_finite() {
        return "--:--".into();
    }
    let minutes = (local.rem_euclid(DAY) / 60.0).round() as u32 % 1440;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

fn clock_minutes(minute: f64) -> String {
    let m = minute.round().clamp(0.0, 1439.0) as u32;
    format!("{:02}:{:02}", m / 60, m % 60)
}

fn parse_clock(text: &str) -> Option<f64> {
    let (h, m) = text.trim().split_once([':', '.', 'h'])?;
    let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().trim_end_matches(['h', 'H']).parse().ok()?);
    (h < 24 && m < 60).then(|| f64::from(h * 60 + m))
}

fn utc_label(offset: i32) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    format!("UTC{sign}{:02}:{:02}", offset.abs() / 60, offset.abs() % 60)
}

/// `1 h 05 min`, `42 min`.
fn hm(minutes: f64) -> String {
    let m = minutes.round().max(0.0) as u32;
    if m >= 60 { format!("{} h {:02} min", m / 60, m % 60) } else { format!("{m} min") }
}

/// The compass point an azimuth falls in (the English key, translated for display).
fn compass(az: f64) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    POINTS.get((((az.rem_euclid(360.0) + 22.5) / 45.0) as usize) % 8).copied().unwrap_or("N")
}

fn az_text(az: f64) -> String {
    format!("{:.0}° {}", az.rem_euclid(360.0), crate::i18n::tr_ctx("compass", compass(az)))
}

fn alt_text(alt: f64) -> String {
    format!("{alt:.1}°")
}

/// `Fri 14 Jun`.
fn short_date(d: Date) -> String {
    let wd = WEEKDAYS.get(d.weekday() as usize).copied().unwrap_or("");
    let mon = MONTHS.get(d.month.saturating_sub(1) as usize).copied().unwrap_or("");
    format!("{} {} {}", crate::i18n::tr_ctx("weekday", wd), d.day, crate::i18n::tr_ctx("month", mon))
}

/// A distance given in millimetres, in the unit that reads best.
fn distance_text(mm: f64) -> String {
    if !mm.is_finite() {
        return "∞".into();
    }
    if mm < 1000.0 {
        return format!("{:.0} cm", mm / 10.0);
    }
    let m = mm / 1000.0;
    if m < 10.0 {
        format!("{m:.2} m")
    } else if m < 1000.0 {
        format!("{m:.1} m")
    } else {
        format!("{:.1} km", m / 1000.0)
    }
}

fn seconds_text(s: f64) -> String {
    if s < 10.0 {
        format!("{s:.1} s")
    } else if s < 120.0 {
        format!("{s:.0} s")
    } else {
        format!("{} min {:02} s", (s / 60.0).floor(), (s % 60.0).round())
    }
}

// ---- computing --------------------------------------------------------------------------------

fn observer(st: &PlannerState) -> Observer {
    Observer::new(st.lat, st.lon)
}

/// The day and UTC offset in force: the typed ones, else today and a guess for the place.
fn resolve(st: &PlannerState, now: f64) -> (Date, i32) {
    let guess_now = st.utc_offset.unwrap_or_else(|| auto_utc_offset_min(st.lat, st.lon, now));
    let today = Date::from_unix(now, guess_now);
    let date = Date::parse(&st.date).unwrap_or(today);
    let offset = st.utc_offset.unwrap_or_else(|| {
        let noon = date.unix_midnight_utc() + DAY / 2.0 - st.lon * 240.0;
        auto_utc_offset_min(st.lat, st.lon, noon)
    });
    (date, offset)
}

fn compute(st: &PlannerState, now: f64) -> Computed {
    let (date, offset) = resolve(st, now);
    let obs = observer(st);
    let start = date.unix_midnight_utc() - f64::from(offset) * 60.0;
    let series = |f: fn(f64, Observer) -> Horizontal| (0..SAMPLES).map(|i| f(start + i as f64 * SAMPLE_STEP, obs)).collect::<Vec<_>>();
    Computed {
        key: (st.lat.to_bits(), st.lon.to_bits(), date.days(), offset),
        date,
        offset,
        start,
        plan: plan_day(obs, start),
        sun: series(light_studio_astro::sun_horizontal),
        moon: series(light_studio_astro::moon_horizontal),
        core: series(light_studio_astro::core_horizontal),
        next_new: next_phase(start, Principal::New),
        next_full: next_phase(start, Principal::Full),
        night_core: light_studio_astro::dark_core_windows(obs, start),
    }
}

/// The cached day, worked out again when the place, day or offset changed.
fn refresh(st: &PlannerState, rt: &mut Runtime, now: f64) -> Arc<Computed> {
    let (date, offset) = resolve(st, now);
    let key = (st.lat.to_bits(), st.lon.to_bits(), date.days(), offset);
    if let Some(c) = rt.computed.as_ref().filter(|c| c.key == key) {
        return c.clone();
    }
    let c = Arc::new(compute(st, now));
    rt.computed = Some(c.clone());
    c
}

/// The minute of the day shown: the chosen one, else now (today) or sunset.
fn cursor_minute(st: &PlannerState, c: &Computed, now: f64) -> f64 {
    if let Some(m) = st.minute {
        return m.clamp(0.0, 1439.0);
    }
    let now_minute = (now - c.start) / 60.0;
    if (0.0..1440.0).contains(&now_minute) {
        return now_minute;
    }
    c.plan.sun.set.map_or(720.0, |s| ((s - c.start) / 60.0).clamp(0.0, 1439.0))
}

// ---- layout helpers ---------------------------------------------------------------------------

/// One label and value on a line, with an optional colour dot.
fn row(ui: &mut Ui, _id: &str, label: &str, value: &str, dot: Option<Color32>) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let p = ui.painter();
    let mut x = r.left();
    if let Some(c) = dot {
        p.circle_filled(pos2(r.left() + 4.0, r.center().y), 3.0, c);
        x += 14.0;
    }
    p.text(pos2(x, r.center().y), Align2::LEFT_CENTER, crate::i18n::tr(label), t.font(12.5), t.text_dim);
    p.text(pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, value, t.font(12.5), t.text);
}

/// Results as tiles, two to a row: the name small above, the value large (one tile when there is a single item).
///
fn tiles(ui: &mut Ui, items: &[(&str, &str, String, Option<Color32>)]) {
    let t = Tokens::get(ui.ctx());
    let gap = 6.0;
    for pair in items.chunks(2) {
        let w = if pair.len() == 1 && items.len() > 2 { ui.available_width() } else { ((ui.available_width() - gap) / 2.0).max(40.0) };
        let (row_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::hover());
        for (k, (_id, label, value, dot)) in pair.iter().enumerate() {
            let r = Rect::from_min_size(pos2(row_rect.left() + k as f32 * (w + gap), row_rect.top()), vec2(w, 46.0));
            ui.painter().rect_filled(r, crate::theme::R_CONTROL, t.inset.gamma_multiply(0.75));
            let p = ui.painter().with_clip_rect(r);
            let mut x = r.left() + 10.0;
            if let Some(c) = dot {
                p.circle_filled(pos2(x + 3.0, r.top() + 14.0), 3.0, *c);
                x += 11.0;
            }
            p.text(pos2(x, r.top() + 14.0), Align2::LEFT_CENTER, crate::i18n::tr(label), t.font(10.5), t.text_dim);
            p.text(pos2(r.left() + 10.0, r.top() + 31.0), Align2::LEFT_CENTER, value, t.semibold(15.0), t.text);
        }
        ui.add_space(0.0);
    }
}

fn note(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add(egui::Label::new(egui::RichText::new(text).font(t.font(11.5)).color(t.text_dim)).wrap());
}

fn subhead(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    ui.label(egui::RichText::new(crate::i18n::tr(text)).font(t.semibold(11.5)).color(t.text_label));
    ui.add_space(2.0);
}

/// A number typed into a field: shown formatted, parsed when the edit ends. True when it changed.
fn number(ui: &mut Ui, rt: &mut Runtime, id: &str, value: &mut f64, range: (f64, f64), decimals: usize, width: f32) -> bool {
    let shown = format!("{:.*}", decimals, *value);
    let widget = format!("planner:{id}");
    let buf = rt.fields.entry(widget.clone()).or_default();
    if !crate::text_field::was_editing(ui.ctx(), &widget) {
        *buf = shown.clone();
    }
    let r = TextField::singleline(&widget, buf).width(width).show(ui);
    let mut changed = false;
    if r.committed() {
        if let Ok(v) = buf.trim().replace(',', ".").parse::<f64>()
            && v.is_finite()
        {
            let v = v.clamp(range.0, range.1);
            changed = (v - *value).abs() > f64::EPSILON;
            *value = v;
        }
        *buf = format!("{:.*}", decimals, *value);
    } else if r.cancelled() {
        *buf = shown;
    }
    changed
}

fn labelled_number(ui: &mut Ui, rt: &mut Runtime, id: &str, label: &str, value: &mut f64, range: (f64, f64), decimals: usize) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(150.0, 26.0), egui::Layout::left_to_right(Align::Center), |ui| {
            ui.label(egui::RichText::new(crate::i18n::tr(label)).font(t.font(12.5)).color(t.text_dim));
        });
        changed = number(ui, rt, id, value, range, decimals, 78.0);
    });
    changed
}

// ---- the page ---------------------------------------------------------------------------------

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let full = ui.max_rect();
    app.ui.planner.sane();
    let now = now_unix();

    app.ui.planner.placed = true;
    run_search(app, ui.ctx());

    let c = refresh(&app.ui.planner, &mut app.planner, now);
    let obs = observer(&app.ui.planner);
    let minute = cursor_minute(&app.ui.planner, &c, now);
    let at = c.start + minute * 60.0;
    let skies = skies_at(obs, at);
    // the nights of the year, for the year chart (the offset follows the place unless it was set by hand)
    let year_key: YearKey = (app.ui.planner.lat.to_bits(), app.ui.planner.lon.to_bits(), c.date.year, app.ui.planner.utc_offset);
    if app.planner.year.as_ref().is_none_or(|(k, _)| *k != year_key) {
        let profiles = year::profiles(obs, c.date.year, app.ui.planner.utc_offset);
        app.planner.year = Some((year_key, Arc::new(profiles)));
    }
    let year_nights = app.planner.year.as_ref().map(|(_, v)| v.clone()).unwrap_or_default();
    let carto = app.ui.map_carto_key.trim().to_string();
    let satellite = app.ui.map_satellite;
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("planner-plain-map"), !satellite));
    let locating = app.tasks.is_running(LOCATING);
    let syncing = app.pdi.running();
    // the cloud button's colour: green when every point is in step with the phone's Worker, red when something is waiting to
    // go (a new or edited point, a deletion, a conflict) or the last try failed; none before the first answer
    let sync_state = {
        let st = &app.ui.planner;
        let pending = !st.pdi_deleted.is_empty()
            || st.saved.iter().any(|p| p.dirty || p.conflict || (p.base == 0 && p.origin != "mobile"))
            || app.pdi.last_ok == Some(false);
        if pending { Some(false) } else { app.pdi.last_ok }
    };
    if locating {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }
    lineup::fetch_terrain(app);
    horizon::fetch(app);
    shade::fetch(app);
    let mut intents: Vec<Intent> = Vec::new();
    let pal = palette(ui.visuals().dark_mode);
    let st = &mut app.ui.planner;
    let rt = &mut app.planner;
    let tiles = &mut app.map;
    let place = if st.place.is_empty() { format!("{:.4}°, {:.4}°", st.lat, st.lon) } else { st.place.clone() };
    let mut page = Page {
        rail_right: full.right() - 10.0,
        t,
        pal,
        c: &c,
        obs,
        minute,
        skies,
        now,
        year: &year_nights,
        place,
        layers: st.layers,
        tiles,
        carto,
        satellite,
        locating,
        syncing,
        sync_state,
        intents: &mut intents,
    };

    // The planner's time bar and cards belong to the Planner mode.
    let mode = st.map_mode;
    let planner = mode == MapMode::Planner;
    // a card (the calculations, Find) takes the room: the side panel steps aside when it opens and stays away after
    let card = rt.calc_open || rt.find_open;
    if card && !rt.calc_seen {
        st.panel_hidden = true;
    }
    rt.calc_seen = card;
    let mut areas = layout::areas(full, st.panel_hidden, st.time_wide, planner && !card);
    // an auto-hiding time bar floats over the map, which then keeps the room: the bar shows while the pointer is near
    // its place (or a drag is going on) and folds into a handle otherwise
    // the Mountains view has no time bar: the skyline takes its room (the time is still set with the cards)
    let mountains = planner && st.show_horizon;
    // (a card takes the whole room: no time bar under it)
    let time_shown = if mountains || card {
        areas.free.max.y = (full.bottom() - 10.0).max(areas.free.min.y);
        false
    } else if planner && st.time_auto {
        areas.free.max.y = (full.bottom() - 12.0).max(areas.free.min.y);
        let zone = areas.time.expand2(vec2(0.0, 6.0));
        let over = ui.ctx().pointer_hover_pos().is_some_and(|q| zone.contains(q))
            || ui.ctx().input(|i| i.pointer.any_down() && i.pointer.press_origin().is_some_and(|q| zone.contains(q)));
        ui.ctx().animate_bool_with_time(egui::Id::new("planner-time-auto"), over, 0.15) > 0.0
    } else {
        true
    };
    page.rail_right = areas.rail_right;
    let mut blocked = Vec::new();
    if planner {
        if time_shown {
            blocked.push(areas.time);
        }
        blocked.extend(layout::cards_rects(areas.free));
    }
    blocked.extend(areas.side);
    // (the cards in front keep the scroll and the drags to themselves)
    if rt.find_open {
        blocked.push(areas.free.shrink(8.0));
    }
    if rt.calc_open {
        blocked.push(areas.free.shrink(8.0));
    }
    if st.show_horizon && planner {
        // the Mountains take the map's place: the skyline fills the free part (the search box stays over it)
        ui.painter().rect_filled(full, 0.0, page.t.backdrop);
        page.horizon_strip(ui, st, rt, horizon::view_rect(areas.free));
    } else {
        page.map_view(ui, st, rt, full, areas.free, &blocked);
    }
    if planner {
        page.date_time_cards(ui, st, rt, areas.free);
    }
    page.floating_search(ui, rt, areas.free);
    if planner && time_shown {
        page.time_bar(ui, st, areas.time);
        if !st.show_horizon {
            page.compass(ui, areas.free);
        }
    } else if planner && !mountains && !card {
        // the handle that stands for the folded time bar
        let h = Rect::from_center_size(pos2(areas.time.center().x, full.bottom() - 7.0), vec2(72.0, 5.0));
        ui.painter().rect_filled(h, 2.5, page.t.text_dim.gamma_multiply(0.7));
        if !st.show_horizon {
            page.compass(ui, areas.free);
        }
    }
    if let Some(side) = areas.side {
        page.side_panel(ui, st, rt, side);
    }
    // the chosen point of interest or plan, in front of everything
    let mut front =
        ui.new_child(egui::UiBuilder::new().layer_id(egui::LayerId::new(egui::Order::Middle, egui::Id::new("planner-record-popup"))).max_rect(full));
    page.record_popup(&mut front, st, rt, areas.free);
    page.calc_popup(&mut front, st, rt, areas.free);
    page.find_popup(&mut front, st, rt, areas.free);

    for intent in intents {
        apply(app, ui.ctx(), intent);
    }
}

/// Writes the points of interest to `path` in the shared format (see [`pois`]): a temp file renamed over it, so a full
/// disk leaves the old file. Gives how many were written.
pub fn export_pois(app: &App, path: &str) -> Result<usize, String> {
    let points = &app.ui.planner.saved;
    let text = pois::export_json(points)?;
    let target = std::path::Path::new(path);
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("could not write {path}: {e}"))?;
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not write {path}: {e}")
    })?;
    Ok(points.len())
}

/// Merges the points of a file of the shared format into the list (see [`pois::import_json`]): importing the same
/// file again changes nothing.
pub fn import_pois(app: &mut App, path: &str) -> Result<serde_json::Value, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("could not read {path}: {e}"))?.len();
    if len > pois::MAX_FILE_BYTES as u64 {
        return Err(format!("{path} is too large to be a list of points of interest"));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {path}: {e}"))?;
    let r = pois::import_json(&mut app.ui.planner.saved, &text, MAX_SAVED)?;
    Ok(json!({"added": r.added, "updated": r.updated, "removed": r.removed, "unchanged": r.unchanged, "rejected": r.rejected}))
}

/// Merges the points the Worker sent into the map (see [`pois::merge_worker`]); the same points again change nothing.
pub fn merge_phone_points(app: &mut App, points: &[serde_json::Value]) -> serde_json::Value {
    let deleted = app.ui.planner.pdi_deleted.clone();
    let r = pois::merge_worker(&mut app.ui.planner.saved, points, MAX_SAVED, &deleted);
    json!({"added": r.added, "updated": r.updated, "removed": r.removed, "unchanged": r.unchanged, "rejected": r.rejected})
}

/// A change or a deletion sent to the Worker, to be matched with its result: the point's id and, for a change, the
/// version it had here when it was sent (`None` for a deletion).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sent {
    pub id: String,
    pub version: Option<u64>,
}

/// What is to go to the Worker: the points changed here (and new ones) and the ones deleted here. Each change has an
/// `op_id` that is the same every time the same change is sent, so a lost answer never makes a duplicate.
pub fn phone_push_ops(app: &mut App) -> (Vec<serde_json::Value>, Vec<Sent>) {
    use crate::services::pdi_sync::{delete_op, upsert_op};
    pois::prepare_sync(&mut app.ui.planner.saved);
    let short = |id: &str| id.chars().filter(char::is_ascii_alphanumeric).take(8).collect::<String>();
    let mut ops = Vec::new();
    let mut sent = Vec::new();
    for p in app.ui.planner.saved.iter().filter(|p| p.dirty && !p.conflict) {
        let op_id = format!("mac-{}-{}-{}", short(&p.id), p.base, p.version);
        ops.push(upsert_op(&op_id, p.base, pois::worker_poi(p)));
        sent.push(Sent { id: p.id.clone(), version: Some(p.version) });
    }
    for g in &app.ui.planner.pdi_deleted {
        ops.push(delete_op(&format!("mac-del-{}-{}", short(&g.id), g.base), &g.id, g.base));
        sent.push(Sent { id: g.id.clone(), version: None });
    }
    (ops, sent)
}

/// What a push did, for the message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushSummary {
    pub sent: usize,
    pub conflicts: usize,
    pub rejected: Vec<String>,
    /// Something went the Worker's way and has to be downloaded again from the start (a conflict).
    pub reload: bool,
}

/// Applies the results of a push to the points (see [`phone_push_ops`]): accepted changes are in step; a refused one is
/// marked so the Worker's version is taken (ours is kept there as a copy); a deletion is done with either way.
pub fn apply_phone_push(app: &mut App, sent: &[Sent], results: &[crate::services::pdi_sync::OpResult]) -> PushSummary {
    use crate::services::pdi_sync::Outcome;
    let mut sum = PushSummary::default();
    let st = &mut app.ui.planner;
    for (s, r) in sent.iter().zip(results) {
        match (s.version, &r.outcome) {
            (Some(sent_v), Outcome::Accepted { version }) => {
                sum.sent += 1;
                if let Some(p) = st.saved.iter_mut().find(|p| p.id == s.id) {
                    p.base = *version;
                    if p.version == sent_v {
                        p.dirty = false;
                        p.version = *version;
                    } else {
                        // edited again while it was on its way: still to send, on top of this version
                        p.version = p.version.max(version.saturating_add(1));
                    }
                }
            }
            (Some(_), Outcome::Conflict { .. }) => {
                sum.conflicts += 1;
                sum.reload = true;
                if let Some(p) = st.saved.iter_mut().find(|p| p.id == s.id) {
                    p.conflict = true;
                }
            }
            (Some(_), Outcome::Rejected { reason }) => sum.rejected.push(reason.clone()),
            (None, outcome) => {
                st.pdi_deleted.retain(|g| g.id != s.id);
                match outcome {
                    Outcome::Accepted { .. } => sum.sent += 1,
                    // not deleted there: it comes back with the next download
                    Outcome::Conflict { .. } => {
                        sum.conflicts += 1;
                        sum.reload = true;
                    }
                    Outcome::Rejected { reason } => sum.rejected.push(reason.clone()),
                }
            }
        }
    }
    sum
}

/// Opens the record of the point of interest `i` in front of the map.
pub fn open_site(app: &mut App, i: usize) {
    app.planner.site_open = Some(i);
    app.planner.plan_popup = None;
}

fn apply(app: &mut App, ctx: &egui::Context, intent: Intent) {
    match intent {
        Intent::UseMapCentre => {
            let v = app.ui.planner.map;
            set_place(&mut app.ui.planner, v.lat, v.lon, String::new());
        }
        Intent::Choose(f) => {
            let label = if f.detail.is_empty() { f.name.clone() } else { format!("{}, {}", f.name, f.detail) };
            set_place(&mut app.ui.planner, f.lat, f.lon, label);
            app.planner.search.clear();
            app.planner.searched.clear();
            app.planner.found.clear();
        }
        Intent::ToggleSatellite => app.ui.map_satellite = !app.ui.map_satellite,
        Intent::OpenSettings => app.settings_open = true,
        Intent::SyncPois => {
            if let Err(e) = crate::pdi::start(app, true) {
                app.toast(ctx, e);
            }
        }
        Intent::LocateMe => locate_me(app, ctx),
        Intent::SavePoi => {
            let st = &mut app.ui.planner;
            let name = st.place.clone();
            match places::keep(&mut st.saved, &name, st.lat, st.lon) {
                Some(i) => {
                    st.map_mode = MapMode::Pois;
                    app.planner.site_open = Some(i);
                    app.planner.plan_popup = None;
                    app.planner.edit_of = Some(popup::Shown::Site(i));
                }
                None => app.toast(ctx, crate::i18n::tr("The list of points of interest is full").to_string()),
            }
        }
        Intent::Full => app.toast(ctx, crate::i18n::tr("The list of points of interest is full").to_string()),
        Intent::JumpToTime(d, m) => {
            app.ui.planner.date = d.iso();
            app.ui.planner.minute = Some(m.clamp(0.0, 1439.0));
        }
        Intent::OpenSite(i) => open_site(app, i),
        Intent::OpenUrl(url) => {
            if let Err(e) = crate::links::open(&url) {
                app.toast(ctx, e);
            }
        }
        Intent::OpenPlan(id) => {
            if open_plan(app, id) {
                app.planner.plan_popup = Some(id);
                app.planner.site_open = None;
            }
        }
        Intent::ShowPlanInPlanner(id) => {
            if open_plan(app, id) {
                app.ui.planner.map_mode = MapMode::Planner;
                app.planner.plan_popup = None;
                app.planner.site_open = None;
                app.planner.edit_of = None;
            }
        }
        Intent::SendToCalendar(id) => plans::send_to_calendar(app, ctx, id),
        Intent::JumpTo(d) => {
            app.ui.planner.date = d.iso();
            app.ui.planner.minute = None;
        }
    }
}

fn set_place(st: &mut PlannerState, lat: f64, lon: f64, name: String) {
    st.lat = lat.clamp(-90.0, 90.0);
    st.lon = lon.clamp(-180.0, 180.0);
    st.place = name;
    st.placed = true;
    // the map follows the place (and is brought in close enough to see where it is)
    st.map.lat = st.lat;
    st.map.lon = st.lon;
    if st.map.zoom < 6.0 {
        st.map.zoom = 10.0;
    }
}

/// The name the task that looks for this computer's position goes by.
const LOCATING: &str = "Finding your location";

/// Puts the place where this computer is (the system's location, else the IP address): the answer comes later.
fn locate_me(app: &mut App, ctx: &egui::Context) {
    if app.tasks.is_running(LOCATING) {
        return;
    }
    let started = crate::tasks::spawn(app, LOCATING, None, crate::services::locate::lookup, |app, ctx, found| match found {
        Ok(l) => {
            set_place(&mut app.ui.planner, l.lat, l.lon, l.label);
            app.ui.planner.map.zoom = app.ui.planner.map.zoom.max(9.0);
        }
        Err(e) => app.toast(ctx, e),
    });
    match started {
        Ok(()) => app.toast(ctx, crate::i18n::tr("Finding your location…").to_string()),
        Err(e) => app.toast(ctx, e),
    }
}

/// Looks the typed name up in OpenStreetMap's geocoders, one request per pause in the typing.
fn run_search(app: &mut App, ctx: &egui::Context) {
    let q = app.planner.search.trim().to_string();
    let now = now_unix();
    if q != app.planner.searched {
        app.planner.searched = q.clone();
        app.planner.typed_at = now;
        app.planner.found.clear();
        app.planner.search_note = None;
    }
    if q.chars().count() < 3 {
        return;
    }
    let (ready, running) = {
        let on = app.planner.online.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        (on.for_query == q, on.running)
    };
    if !ready && !running && q != app.planner.online_asked {
        let wait = 0.4 - (now - app.planner.typed_at);
        if wait > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        } else {
            start_online(app, &q);
        }
    }
    if running {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    if ready {
        let on = app.planner.online.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !on.hits.is_empty() {
            app.planner.found = on.hits.clone();
            app.planner.search_note = None;
        } else {
            app.planner.search_note = Some(match &on.error {
                Some(e) => crate::i18n::tr(e).to_string(),
                None => crate::i18n::tr("No place by that name").to_string(),
            });
        }
    } else if running && app.planner.found.is_empty() {
        app.planner.search_note = Some(crate::i18n::tr("Searching…").to_string());
    }
}

fn start_online(app: &mut App, q: &str) {
    app.planner.online_asked = q.to_string();
    app.planner.online.lock().unwrap_or_else(std::sync::PoisonError::into_inner).running = true;
    let shared = app.planner.online.clone();
    let (query, lang) = (q.to_string(), crate::i18n::language().code().to_string());
    let spawned = std::thread::Builder::new().name("place-search".into()).spawn({
        let shared = shared.clone();
        move || {
            let outcome = std::panic::catch_unwind(|| crate::services::geocode::search(&query, &lang, 8))
                .unwrap_or_else(|_| Err("The place search stopped unexpectedly".to_string()));
            let mut on = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            on.running = false;
            on.for_query = query;
            match outcome {
                Ok(hits) => {
                    on.hits = hits.into_iter().map(|h| Found { name: h.name, detail: h.detail, lat: h.lat, lon: h.lon }).collect();
                    on.error = None;
                }
                Err(e) => {
                    on.hits.clear();
                    on.error = Some(e);
                }
            }
        }
    });
    if spawned.is_err() {
        shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner).running = false;
    }
}

/// Everything the page draws from, gathered before drawing.
struct Page<'a> {
    /// Right edge of the map's icon rail (see `layout::Areas::rail_right`).
    rail_right: f32,
    t: Tokens,
    pal: Pal,
    c: &'a Computed,
    obs: Observer,
    minute: f64,
    skies: Skies,
    now: f64,
    /// The nights of the year, every few days, for the year chart.
    year: &'a [NightProfile],
    /// The place's name, or its coordinates.
    place: String,
    /// Which of the Sun, Moon and core are drawn (as of the start of the frame).
    layers: [bool; 3],
    /// The map's tiles, and the CARTO key they are fetched with.
    tiles: &'a mut crate::map::Runtime,
    carto: String,
    /// Satellite imagery with place names (else the plain dark/light map).
    satellite: bool,
    /// A «my location» lookup is running.
    locating: bool,
    /// Whether the phone's points of interest are being downloaded.
    syncing: bool,
    /// `Some(true)`: in step with the phone; `Some(false)`: something is waiting to be sent; `None`: not known yet.
    sync_state: Option<bool>,
    intents: &'a mut Vec<Intent>,
}

impl Page<'_> {
    fn search_field(&mut self, ui: &mut Ui, rt: &mut Runtime, width: f32) {
        let t = self.t;
        let (field, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
        let k = ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::new("planner-search-opacity"))).unwrap_or(1.0);
        ui.painter().rect_filled(field, R_CONTROL + 2.0, t.field.gamma_multiply(k));
        ui.painter().rect_stroke(field, R_CONTROL + 2.0, Stroke::new(1.0, t.field_border.gamma_multiply(k)), StrokeKind::Inside);
        crate::icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(field.left() + 15.0, field.center().y), vec2(14.0, 14.0)),
            Icon::Search,
            t.text_dim,
        );
        let mut inner = ui
            .new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(field.left() + 28.0, field.top() + 2.0), field.max - vec2(6.0, 2.0))));
        let mut text = std::mem::take(&mut rt.search);
        let r = TextField::singleline("planner:search", &mut text)
            .hint(crate::i18n::tr("Search for a place"))
            .width(field.width() - 40.0)
            .frame(false)
            .show(&mut inner);
        if r.cancelled() {
            text.clear();
        }
        rt.search = text;
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("planner-search-at"), field));
    }

    fn search_results(&mut self, ui: &mut Ui, rt: &mut Runtime) {
        let t = self.t;
        let q = rt.search.trim().to_string();
        if q.is_empty() || (rt.found.is_empty() && rt.search_note.is_none()) {
            return;
        }
        let Some(field) = ui.ctx().data(|d| d.get_temp::<Rect>(egui::Id::new("planner-search-at"))) else { return };
        let w = 300.0;
        let mut chosen = None;
        egui::Area::new(egui::Id::new("planner-search-results"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(field.left(), field.bottom() + 6.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE
                    .fill(crate::theme::chip_fill(&t))
                    .stroke(Stroke::new(1.0, t.card_stroke))
                    .corner_radius(crate::theme::R_TILE)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_width(w);
                        match &rt.search_note {
                            Some(note_text) => {
                                ui.label(egui::RichText::new(note_text).font(t.font(12.0)).color(t.text_dim));
                            }
                            None => {
                                for f in rt.found.iter() {
                                    let mut job = egui::text::LayoutJob::default();
                                    job.append(&f.name, 0.0, egui::TextFormat::simple(t.semibold(12.5), t.text));
                                    if !f.detail.is_empty() {
                                        job.append(&format!("  {}", f.detail), 0.0, egui::TextFormat::simple(t.font(11.5), t.text_dim));
                                    }
                                    let resp = ui.add(egui::Button::new(job).frame(false).min_size(vec2(w, 26.0)));
                                    if resp.clicked() {
                                        chosen = Some(f.clone());
                                    }
                                }
                            }
                        }
                    });
            });
        if let Some(f) = chosen {
            self.intents.push(Intent::Choose(f));
        }
    }

    // -- the day chart ----------------------------------------------------------------------

    /// The day's chart: the heights over the day on the sky's colours; `h` tall (with the hours under it). `compact`
    /// (the time bar's strip) leaves out the height labels.
    fn chart(&mut self, ui: &mut Ui, st: &mut PlannerState, h: f32, compact: bool) {
        let t = self.t;
        let pal = self.pal;
        let c = self.c;
        let w = ui.available_width();
        let (outer, resp) = ui.allocate_exact_size(vec2(w, h + 18.0), Sense::click_and_drag());
        let left = if compact { 2.0 } else { 36.0 };
        let plot = Rect::from_min_max(pos2(outer.left() + left, outer.top() + 2.0), pos2(outer.right() - 8.0, outer.bottom() - 18.0));
        let p = ui.painter_at(outer);
        let (ymin, ymax) = (-50.0_f32, 90.0_f32);
        let x_of = |unix: f64| plot.left() + ((unix - c.start) / DAY) as f32 * plot.width();
        let y_of = |alt: f64| plot.bottom() - ((alt as f32).clamp(ymin, ymax) - ymin) / (ymax - ymin) * plot.height();
        let dark = ui.visuals().dark_mode;

        // the colour of the sky behind each ten minutes, by the Sun's height
        p.rect_filled(plot, 4.0, t.inset);
        for (i, h) in c.sun.iter().enumerate().take(SAMPLES - 1) {
            let (x0, x1) = (x_of(c.start + i as f64 * SAMPLE_STEP), x_of(c.start + (i + 1) as f64 * SAMPLE_STEP));
            let Some(tint) = sky_tint(h.alt, dark, pal) else { continue };
            p.rect_filled(Rect::from_min_max(pos2(x0, plot.top()), pos2(x1 + 0.5, plot.bottom())), 0.0, tint);
        }
        // altitude grid
        for (alt, label) in [(90.0, "90°"), (60.0, "60°"), (30.0, "30°"), (0.0, "0°"), (-18.0, "−18°")] {
            let y = y_of(alt);
            let strong = alt == 0.0;
            let stroke = Stroke::new(if strong { 1.3 } else { 1.0 }, if strong { t.text_dim } else { t.divider });
            if alt < 0.0 {
                p.extend(Shape::dashed_line(&[pos2(plot.left(), y), pos2(plot.right(), y)], stroke, 4.0, 4.0));
            } else {
                p.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], stroke);
            }
            if !compact {
                p.text(pos2(plot.left() - 5.0, y), Align2::RIGHT_CENTER, label, t.font(10.0), t.text_dim);
            }
        }
        for hour in (0..=24).step_by(3) {
            let x = plot.left() + hour as f32 / 24.0 * plot.width();
            p.line_segment([pos2(x, plot.bottom()), pos2(x, plot.bottom() + 4.0)], Stroke::new(1.0, t.text_dim));
            // the last label ends at the chart's edge instead of running past it
            let align = if hour == 24 { Align2::RIGHT_TOP } else { Align2::CENTER_TOP };
            let align = if hour == 0 { Align2::LEFT_TOP } else { align };
            p.text(pos2(x, plot.bottom() + 5.0), align, format!("{:02}:00", hour % 24), t.font(10.0), t.text_dim);
        }
        // golden and blue hours as bands along the bottom edge
        for (span, color) in
            [(c.plan.sun.golden_am, pal.golden), (c.plan.sun.golden_pm, pal.golden), (c.plan.sun.blue_am, pal.blue), (c.plan.sun.blue_pm, pal.blue)]
        {
            if let Some(s) = span {
                p.rect_filled(Rect::from_min_max(pos2(x_of(s.start), plot.bottom() - 5.0), pos2(x_of(s.end), plot.bottom())), 0.0, color);
            }
        }
        // the curves: the Sun on top
        let curve = |series: &[Horizontal], color: Color32| {
            let mut run: Vec<Pos2> = Vec::new();
            let flush = |run: &mut Vec<Pos2>| {
                if run.len() > 1 {
                    p.add(Shape::line(std::mem::take(run), Stroke::new(2.0, color)));
                }
                run.clear();
            };
            for (i, h) in series.iter().enumerate() {
                if h.alt < f64::from(ymin) {
                    flush(&mut run);
                } else {
                    run.push(pos2(x_of(c.start + i as f64 * SAMPLE_STEP), y_of(h.alt)));
                }
            }
            flush(&mut run);
        };
        if st.layers[2] {
            curve(&c.core, pal.core);
        }
        if st.layers[1] {
            curve(&c.moon, pal.moon);
        }
        if st.layers[0] {
            curve(&c.sun, pal.sun);
        }
        // rise and set marks on the horizon
        let marks = [
            (st.layers[0], pal.sun, [c.plan.sun.rise, c.plan.sun.set]),
            (st.layers[1], pal.moon, [c.plan.moon.rise, c.plan.moon.set]),
            (st.layers[2], pal.core, [c.plan.core.rise, c.plan.core.set]),
        ];
        for (on, color, events) in marks {
            for e in events.into_iter().flatten().filter(|_| on) {
                let at = pos2(x_of(e), y_of(0.0));
                p.circle_filled(at, 4.0, t.chrome);
                p.circle_stroke(at, 4.0, Stroke::new(1.6, color));
            }
        }
        // now, on today's chart
        let now_x = x_of(self.now);
        if (plot.left()..=plot.right()).contains(&now_x) {
            p.extend(Shape::dashed_line(&[pos2(now_x, plot.top()), pos2(now_x, plot.bottom())], Stroke::new(1.0, t.text_dim), 3.0, 3.0));
        }
        // the time shown
        let cx = plot.left() + (self.minute / 1440.0) as f32 * plot.width();
        p.line_segment([pos2(cx, plot.top()), pos2(cx, plot.bottom())], Stroke::new(1.5, t.text));
        for (on, color, h) in
            [(st.layers[0], pal.sun, self.skies.sun), (st.layers[1], pal.moon, self.skies.moon), (st.layers[2], pal.core, self.skies.core)]
        {
            if on && h.alt >= f64::from(ymin) {
                let at = pos2(cx, y_of(h.alt));
                p.circle_filled(at, 5.0, color);
                p.circle_stroke(at, 5.0, Stroke::new(1.5, t.chrome));
            }
        }
        if (resp.dragged() || resp.clicked())
            && let Some(pos) = resp.interact_pointer_pos()
        {
            st.minute = Some((f64::from((pos.x - plot.left()) / plot.width()) * 1440.0).clamp(0.0, 1439.0));
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }

    // -- the sky dial -----------------------------------------------------------------------

    fn dial_card(&mut self, ui: &mut Ui) {
        let t = self.t;
        let pal = self.pal;
        let c = self.c;
        let size = ui.available_width().clamp(220.0, 380.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), size), Sense::hover());
        let centre = rect.center();
        let radius = (size / 2.0 - 24.0).max(40.0);
        let p = ui.painter_at(rect);
        let polar = |az: f64, alt: f64| {
            let r = radius * ((90.0 - alt.clamp(-5.0, 90.0)) / 90.0) as f32;
            let a = az.to_radians();
            pos2(centre.x + r * a.sin() as f32, centre.y - r * a.cos() as f32)
        };
        p.circle_filled(centre, radius, t.inset);
        for alt in [60.0, 30.0] {
            p.circle_stroke(centre, radius * ((90.0 - alt) / 90.0) as f32, Stroke::new(1.0, t.divider));
        }
        p.circle_stroke(centre, radius, Stroke::new(1.3, t.text_dim));
        for deg in (0..360).step_by(30) {
            let a = f64::from(deg).to_radians();
            let (s, co) = (a.sin() as f32, a.cos() as f32);
            let inner = if deg % 90 == 0 { radius - 8.0 } else { radius - 4.0 };
            p.line_segment(
                [pos2(centre.x + inner * s, centre.y - inner * co), pos2(centre.x + radius * s, centre.y - radius * co)],
                Stroke::new(1.0, t.text_dim),
            );
        }
        for (label, az) in [("N", 0.0_f64), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
            let a = az.to_radians();
            let r = radius + 13.0;
            p.text(
                pos2(centre.x + r * a.sin() as f32, centre.y - r * a.cos() as f32),
                Align2::CENTER_CENTER,
                crate::i18n::tr_ctx("compass", label),
                t.semibold(12.0),
                if label == "N" { t.accent } else { t.text_label },
            );
        }
        // where each body rises and sets, and the path it takes above the horizon
        let bodies: [(Color32, &[Horizontal], [Option<f64>; 2]); 3] = [
            (pal.core, &c.core, [c.plan.core.rise_az, c.plan.core.set_az]),
            (pal.moon, &c.moon, [c.plan.moon.rise_az, c.plan.moon.set_az]),
            (pal.sun, &c.sun, [c.plan.sun.rise_az, c.plan.sun.set_az]),
        ];
        let layer_of = [2usize, 1, 0];
        for (n, (color, series, events)) in bodies.into_iter().enumerate() {
            if !self.layers.get(layer_of[n]).copied().unwrap_or(true) {
                continue;
            }
            for az in events.into_iter().flatten() {
                let a = az.to_radians();
                let edge = pos2(centre.x + radius * a.sin() as f32, centre.y - radius * a.cos() as f32);
                p.line_segment([centre, edge], Stroke::new(1.0, color.gamma_multiply(0.35)));
                p.circle_filled(edge, 3.5, color);
            }
            let mut run: Vec<Pos2> = Vec::new();
            for h in series.iter() {
                if h.alt >= 0.0 {
                    run.push(polar(h.az, h.alt));
                } else if run.len() > 1 {
                    p.add(Shape::line(std::mem::take(&mut run), Stroke::new(2.0, color)));
                } else {
                    run.clear();
                }
            }
            if run.len() > 1 {
                p.add(Shape::line(run, Stroke::new(2.0, color)));
            }
        }
        // where they are at the time shown
        for (i, (color, h)) in [(pal.sun, self.skies.sun), (pal.moon, self.skies.moon), (pal.core, self.skies.core)].into_iter().enumerate() {
            if !self.layers.get(i).copied().unwrap_or(true) {
                continue;
            }
            if h.alt >= 0.0 {
                let at = polar(h.az, h.alt);
                p.circle_filled(at, 6.0, color);
                p.circle_stroke(at, 6.0, Stroke::new(1.8, t.chrome));
            } else {
                // below the horizon: a hollow mark on the rim, in its direction
                let a = h.az.to_radians();
                let at = pos2(centre.x + radius * a.sin() as f32, centre.y - radius * a.cos() as f32);
                p.circle_filled(at, 5.0, t.inset);
                p.circle_stroke(at, 5.0, Stroke::new(1.6, color.gamma_multiply(0.7)));
            }
        }
        ui.add_space(4.0);
        note(ui, crate::i18n::tr("Looking down on the sky: the centre is straight up, the rim the horizon. A hollow mark is below the horizon."));
    }

    // -- sun, moon, core --------------------------------------------------------------------

    fn sun_card(&mut self, ui: &mut Ui) {
        let c = self.c;
        let s = &c.plan.sun;
        let pal = self.pal;
        let clk = |t: Option<f64>| t.map_or_else(|| "—".to_string(), |t| clock(t, c.offset));
        let with_az = |t: Option<f64>, az: Option<f64>| match (t, az) {
            (Some(t), Some(az)) => format!("{}  ·  {}", clock(t, c.offset), az_text(az)),
            _ => "—".to_string(),
        };
        let span = |sp: Option<light_studio_astro::Span>| {
            sp.map_or_else(|| "—".to_string(), |sp| format!("{} – {}", clock(sp.start, c.offset), clock(sp.end, c.offset)))
        };
        match s.state {
            SunState::MidnightSun => note(ui, crate::i18n::tr("The Sun does not set on this day.")),
            SunState::PolarNight => note(ui, crate::i18n::tr("The Sun does not rise on this day.")),
            SunState::Normal => {}
        }
        row(ui, "sun:rise", "Sunrise", &with_az(s.rise, s.rise_az), Some(pal.sun));
        row(ui, "sun:noon", "Solar Noon", &format!("{}  ·  {}", clock(s.noon, c.offset), alt_text(s.noon_alt)), Some(pal.sun));
        row(ui, "sun:set", "Sunset", &with_az(s.set, s.set_az), Some(pal.sun));
        if let (Some(r), Some(sn)) = (s.rise, s.set) {
            row(ui, "sun:length", "Daylight", &hm((sn - r) / 60.0), None);
        }
        subhead(ui, "Light");
        row(ui, "sun:goldenAm", "Golden Hour (morning)", &span(s.golden_am), Some(pal.golden));
        row(ui, "sun:goldenPm", "Golden Hour (evening)", &span(s.golden_pm), Some(pal.golden));
        row(ui, "sun:blueAm", "Blue Hour (morning)", &span(s.blue_am), Some(pal.blue));
        row(ui, "sun:bluePm", "Blue Hour (evening)", &span(s.blue_pm), Some(pal.blue));
        subhead(ui, "Twilight");
        row(ui, "sun:civil", "Civil", &format!("{} / {}", clk(s.civil.dawn), clk(s.civil.dusk)), None);
        row(ui, "sun:nautical", "Nautical", &format!("{} / {}", clk(s.nautical.dawn), clk(s.nautical.dusk)), None);
        row(ui, "sun:astro", "Astronomical", &format!("{} / {}", clk(s.astronomical.dawn), clk(s.astronomical.dusk)), None);
        note(
            ui,
            &format!(
                "{}: {:.0}°, {:.0}°, {:.0}°; {} {:.0}° … +{:.0}°.",
                crate::i18n::tr("Twilights at"),
                CIVIL,
                NAUTICAL,
                ASTRONOMICAL,
                crate::i18n::tr("golden hour"),
                GOLDEN_BOTTOM,
                GOLDEN_TOP
            ),
        );
    }

    fn moon_card(&mut self, ui: &mut Ui, st: &PlannerState) {
        let t = self.t;
        let c = self.c;
        let pal = self.pal;
        let m = &c.plan.moon;
        let at = c.start + self.minute * 60.0;
        let phase = moon_phase(at);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
            let (lit, shade) = moon_colors(ui.visuals().dark_mode, pal, t.inset);
            moon_disc(ui.painter(), r.center(), 24.0, phase.elongation, st.lat < 0.0, lit, shade, t.divider);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(crate::i18n::tr(phase.name.label())).font(t.semibold(14.0)).color(t.text));
                ui.label(
                    egui::RichText::new(format!("{:.0} % {}", phase.illumination * 100.0, crate::i18n::tr("lit")))
                        .font(t.font(12.5))
                        .color(t.text_dim),
                );
            });
        });
        ui.add_space(4.0);
        let with_az = |t: Option<f64>, az: Option<f64>| match (t, az) {
            (Some(t), Some(az)) => format!("{}  ·  {}", clock(t, c.offset), az_text(az)),
            _ => "—".to_string(),
        };
        row(ui, "moon:rise", "Moonrise", &with_az(m.rise, m.rise_az), Some(pal.moon));
        row(ui, "moon:set", "Moonset", &with_az(m.set, m.set_az), Some(pal.moon));
        row(ui, "moon:transit", "Highest", &format!("{}  ·  {}", clock(m.transit, c.offset), alt_text(m.transit_alt)), Some(pal.moon));
        subhead(ui, "Coming");
        let day = |t: Option<f64>| t.map_or_else(|| "—".to_string(), |t| short_date(Date::from_unix(t, c.offset)));
        row(ui, "moon:nextNew", "Next New Moon", &day(c.next_new), None);
        row(ui, "moon:nextFull", "Next Full Moon", &day(c.next_full), None);
        // a moonless dark sky is what the Milky Way wants
        let lit_day = moon_phase(c.start + DAY / 2.0).illumination;
        note(
            ui,
            if lit_day < 0.15 {
                crate::i18n::tr("A dark-moon day: good for the Milky Way.")
            } else if lit_day > 0.75 {
                crate::i18n::tr("A bright Moon: the faint sky will wash out.")
            } else {
                crate::i18n::tr("The Moon is up part of the night.")
            },
        );
    }

    fn core_card(&mut self, ui: &mut Ui) {
        let c = self.c;
        let pal = self.pal;
        let k = &c.plan.core;
        let with_az = |t: Option<f64>, az: Option<f64>| match (t, az) {
            (Some(t), Some(az)) => format!("{}  ·  {}", clock(t, c.offset), az_text(az)),
            _ => "—".to_string(),
        };
        row(ui, "core:rise", "Core Rises", &with_az(k.rise, k.rise_az), Some(pal.core));
        row(ui, "core:transit", "Core Highest", &format!("{}  ·  {}", clock(k.transit, c.offset), alt_text(k.transit_alt)), Some(pal.core));
        row(ui, "core:set", "Core Sets", &with_az(k.set, k.set_az), Some(pal.core));
        subhead(ui, "Night starting this evening");
        if c.night_core.is_empty() {
            note(
                ui,
                if k.transit_alt < 5.0 {
                    crate::i18n::tr("The core does not rise high enough here on this day.")
                } else {
                    crate::i18n::tr("The core is up only in daylight or twilight on this day.")
                },
            );
        } else {
            for (i, s) in c.night_core.iter().enumerate() {
                row(
                    ui,
                    &format!("core:dark:{i}"),
                    "Visible",
                    &format!("{} – {}  ({})", clock(s.start, c.offset), clock(s.end, c.offset), hm(s.minutes())),
                    Some(pal.core),
                );
            }
            // the Moon's share, judged in the middle of the first window
            if let Some(mid) = c.night_core.first().map(|s| (s.start + s.end) / 2.0) {
                let up = light_studio_astro::moon_horizontal(mid, self.obs).alt > light_studio_astro::plan::MOON_DOWN_ALT;
                let lit = moon_phase(mid).illumination * 100.0;
                note(
                    ui,
                    &if up {
                        format!("{} ({lit:.0} %).", crate::i18n::tr("The Moon is up during the window"))
                    } else {
                        crate::i18n::tr("The Moon is down during the window.").to_string()
                    },
                );
            }
        }
        note(ui, &format!("{}: {} · {}", crate::i18n::tr("Core at the time shown"), az_text(self.skies.core.az), alt_text(self.skies.core.alt)));
    }

    // -- coming nights and showers ----------------------------------------------------------

    fn showers_card(&mut self, ui: &mut Ui) {
        let t = self.t;
        let c = self.c;
        let mut list: Vec<(i32, &light_studio_astro::Shower)> =
            SHOWERS.iter().map(|s| (s.days_to_peak(c.date), s)).filter(|(d, s)| *d <= 60 || s.active_on(c.date)).collect();
        list.sort_by_key(|(d, _)| *d);
        list.truncate(6);
        if list.is_empty() {
            note(ui, crate::i18n::tr("No meteor shower peaks in the next two months."));
            return;
        }
        for (days, s) in list.into_iter() {
            let peak = {
                let this_year = s.peak_date(c.date.year);
                if this_year.days() >= c.date.days() { this_year } else { s.peak_date(c.date.year + 1) }
            };
            // the small hours after the peak's evening: the best time to watch
            let watch = peak.unix_midnight_utc() - f64::from(c.offset) * 60.0 + 2.0 * 3600.0;
            let illum = moon_phase(watch).illumination;
            let radiant = s.radiant_horizontal(watch, self.obs).alt;
            let verdict = if illum < 0.3 && radiant > 20.0 {
                t.pick
            } else if illum > 0.75 || radiant < 0.0 {
                t.reject
            } else {
                t.caution
            };
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(r, R_CONTROL, t.hover.gamma_multiply(0.5));
            }
            p.circle_filled(pos2(r.left() + 10.0, r.top() + 14.0), 4.5, verdict);
            p.text(pos2(r.left() + 24.0, r.top() + 14.0), Align2::LEFT_CENTER, crate::i18n::tr(s.name), t.semibold(12.5), t.text);
            let when = if days == 0 {
                crate::i18n::tr("peak today").to_string()
            } else if s.active_on(c.date) && days > 300 {
                crate::i18n::tr("just past its peak").to_string()
            } else {
                crate::i18n::tr("peak in {n} days").replace("{n}", &days.to_string())
            };
            p.text(pos2(r.right() - 4.0, r.top() + 14.0), Align2::RIGHT_CENTER, format!("{} · {}", short_date(peak), when), t.font(11.5), t.text_dim);
            let detail =
                format!("ZHR {}  ·  {} {:.0} %  ·  {} {:.0}°", s.zhr, crate::i18n::tr("Moon"), illum * 100.0, crate::i18n::tr("radiant"), radiant);
            p.text(pos2(r.left() + 24.0, r.top() + 31.0), Align2::LEFT_CENTER, detail, t.font(11.5), t.text_dim);
            if resp.clicked() {
                self.intents.push(Intent::JumpTo(peak));
            }
        }
        ui.add_space(4.0);
        note(
            ui,
            crate::i18n::tr(
                "Moon and radiant height are for 02:00 on the peak night. Green: dark and high; red: bright Moon or the radiant is below the horizon.",
            ),
        );
    }

    // -- exposure and focus -----------------------------------------------------------------

    /// The camera the calculations are for: sensor, lens, aperture, focus distance and resolution.
    pub(super) fn camera_inputs(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let t = self.t;
        let names: Vec<(&str, String)> = SENSORS.iter().enumerate().map(|(n, s)| (s.name, format!("{n}"))).collect();
        let items: Vec<(&str, &str)> = names.iter().map(|(label, key)| (*label, key.as_str())).collect();
        if let Some(n) = crate::widgets::segmented(ui, "plannerSensor", &items, Some(st.sensor), 3) {
            st.sensor = n;
        }
        ui.add_space(8.0);
        labelled_number(ui, rt, "focal", "Focal Length (mm)", &mut st.focal, (1.0, 2000.0), 0);
        // aperture walks the f-stops
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(vec2(150.0, 26.0), egui::Layout::left_to_right(Align::Center), |ui| {
                ui.label(egui::RichText::new(crate::i18n::tr("Aperture (f/)")).font(t.font(12.5)).color(t.text_dim));
            });
            if icon_button(ui, "plannerApertureLess", Icon::Minus, vec2(24.0, 26.0), false, true, "Wider Aperture").clicked() {
                st.aperture = F_STOPS.iter().rev().copied().find(|f| *f < st.aperture - 0.01).unwrap_or(F_STOPS[0]);
            }
            number(ui, rt, "aperture", &mut st.aperture, (0.7, 64.0), 1, 52.0);
            if icon_button(ui, "plannerApertureMore", Icon::Plus, vec2(24.0, 26.0), false, true, "Narrower Aperture").clicked() {
                st.aperture = F_STOPS.iter().copied().find(|f| *f > st.aperture + 0.01).unwrap_or(F_STOPS[F_STOPS.len() - 1]);
            }
        });
        labelled_number(ui, rt, "distance", "Focus Distance (m)", &mut st.distance, (0.05, 100_000.0), 2);
        labelled_number(ui, rt, "megapixels", "Resolution (megapixels)", &mut st.megapixels, (0.5, 400.0), 1);
    }

    /// What the camera gives: the sharp range, the stars and the field of view.
    pub(super) fn focus_results(&mut self, ui: &mut Ui, st: &mut PlannerState, _rt: &mut Runtime) {
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        let dof = optics::depth_of_field(st.focal, st.aperture, sensor.coc, st.distance * 1000.0);
        let pitch = sensor.pixel_pitch_um(st.megapixels);
        match dof {
            Some(d) => {
                row(ui, "exp:hyperfocal", "Hyperfocal Distance", &distance_text(d.hyperfocal), None);
                row(ui, "exp:near", "Sharp From", &distance_text(d.near), None);
                row(ui, "exp:far", "Sharp To", &d.far.map_or_else(|| "∞".to_string(), distance_text), None);
                row(ui, "exp:depth", "Depth of Field", &d.total().map_or_else(|| "∞".to_string(), distance_text), None);
                note(ui, &format!("{} {}: {} → ∞.", crate::i18n::tr("Focus at"), distance_text(d.hyperfocal), distance_text(d.hyperfocal / 2.0)));
            }
            None => note(ui, crate::i18n::tr("Focus must be farther than the focal length.")),
        }
        subhead(ui, "Stars");
        let npf = optics::npf_seconds(st.aperture, pitch, st.focal, 0.0);
        row(ui, "exp:npf", "Longest Exposure (NPF)", &npf.map_or_else(|| "—".to_string(), seconds_text), None);
        row(ui, "exp:500", "500 Rule", &optics::rule_500_seconds(st.focal * sensor.crop()).map_or_else(|| "—".to_string(), seconds_text), None);
        row(ui, "exp:pitch", "Pixel Pitch", &format!("{pitch:.2} µm"), None);
        subhead(ui, "Field of view");
        let (fw, fh) = (optics::field_of_view(sensor.width, st.focal), optics::field_of_view(sensor.height, st.focal));
        let diag = optics::field_of_view(sensor.width.hypot(sensor.height), st.focal);
        let deg = |v: Option<f64>| v.map_or_else(|| "—".to_string(), |v| format!("{v:.1}°"));
        row(ui, "exp:fovH", "Horizontal", &deg(fw), None);
        row(ui, "exp:fovV", "Vertical", &deg(fh), None);
        row(ui, "exp:fovD", "Diagonal", &deg(diag), None);
        note(ui, crate::i18n::tr("NPF is for stars on the celestial equator; stars nearer the pole trail more slowly."));
    }
}

// ---- drawing pieces ---------------------------------------------------------------------------

/// The sky colour behind a ten-minute slice of the chart, by the Sun's height; `None` in full daylight.
fn sky_tint(alt: f64, dark: bool, pal: Pal) -> Option<Color32> {
    let a = |c: Color32, alpha: u8| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha);
    let (night, deep, mid) = if dark {
        (Color32::from_rgb(4, 6, 24), Color32::from_rgb(18, 28, 78), Color32::from_rgb(44, 70, 150))
    } else {
        (Color32::from_rgb(40, 48, 100), Color32::from_rgb(70, 92, 170), Color32::from_rgb(110, 140, 215))
    };
    Some(if alt < ASTRONOMICAL {
        a(night, if dark { 200 } else { 120 })
    } else if alt < NAUTICAL {
        a(deep, if dark { 170 } else { 100 })
    } else if alt < CIVIL {
        a(mid, if dark { 130 } else { 80 })
    } else if alt < GOLDEN_BOTTOM {
        a(pal.blue, 70)
    } else if alt < GOLDEN_TOP {
        a(pal.golden, 60)
    } else {
        return None;
    })
}

/// The Moon's disc with its lit part, the way it looks from the northern hemisphere (mirrored south
/// of the equator). `elongation`: degrees from the new moon, 0–360.
/// The Moon's lit and shaded colours: the lit part is the bright one in either theme.
fn moon_colors(dark_mode: bool, pal: Pal, inset: Color32) -> (Color32, Color32) {
    if dark_mode { (pal.moon, inset) } else { (Color32::from_rgb(255, 252, 238), pal.moon) }
}

fn moon_disc(p: &egui::Painter, centre: Pos2, r: f32, elongation: f64, mirrored: bool, lit: Color32, dark: Color32, outline: Color32) {
    p.circle_filled(centre, r, dark);
    let phi = elongation.to_radians();
    let waxing = elongation < 180.0;
    let cosphi = phi.cos() as f32;
    let rows = (r * 2.0).ceil().max(8.0) as usize;
    for i in 0..rows {
        let y0 = -r + 2.0 * r * i as f32 / rows as f32;
        let y1 = -r + 2.0 * r * (i + 1) as f32 / rows as f32;
        let ym = (y0 + y1) / 2.0;
        let half = (r * r - ym * ym).max(0.0).sqrt();
        let (mut xl, mut xr) = if waxing { (half * cosphi, half) } else { (-half, -half * cosphi) };
        if mirrored {
            (xl, xr) = (-xr, -xl);
        }
        if xr > xl {
            p.rect_filled(Rect::from_min_max(pos2(centre.x + xl, centre.y + y0), pos2(centre.x + xr, centre.y + y1 + 0.4)), 0.0, lit);
        }
    }
    p.circle_stroke(centre, r, Stroke::new(1.0, outline));
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn new_app() -> App {
        App::new(egui::Context::default(), Default::default(), false)
    }

    /// The page drawn a few frames in a window of `size` (the first frames lay things out).
    fn draw(app: &mut App, size: egui::Vec2) {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        crate::theme::apply(&ctx);
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..Default::default() };
        for _ in 0..3 {
            let mut output = ctx.run_ui(input.clone(), |ui| show(app, ui));
            output.textures_delta.clear();
        }
    }

    #[test]
    fn the_page_draws_at_every_size_and_never_panics_on_hostile_state() {
        let hostile = PlannerState {
            lat: f64::NAN,
            lon: f64::INFINITY,
            utc_offset: Some(99_999),
            minute: Some(f64::NAN),
            sensor: 99,
            focal: -5.0,
            aperture: f64::NAN,
            distance: 0.0,
            tl_clip: f64::NEG_INFINITY,
            nd_stops: 1.0e9,
            map: crate::map::MapView { lat: f64::NAN, lon: 1.0e9, zoom: -50.0 },
            saved: vec![SavedPlace { name: "x".repeat(10_000), lat: f64::NAN, lon: 0.0, ..Default::default() }; 200],
            date: "not a date".into(),
            ..Default::default()
        };
        let places = [
            PlannerState::default(),
            hostile,
            // the polar day and night, the southern hemisphere, the date line
            PlannerState { lat: 80.0, lon: 10.0, date: "2024-06-21".into(), placed: true, ..Default::default() },
            PlannerState { lat: 80.0, lon: 10.0, date: "2024-12-21".into(), placed: true, ..Default::default() },
            PlannerState { lat: -33.9, lon: 151.2, date: "2024-12-21".into(), placed: true, ..Default::default() },
            PlannerState { lat: 0.0, lon: 180.0, date: "2024-03-20".into(), placed: true, ..Default::default() },
            PlannerState { lat: -90.0, lon: -180.0, date: "0001-01-01".into(), placed: true, ..Default::default() },
        ];
        for state in places {
            for size in [vec2(280.0, 500.0), vec2(700.0, 900.0), vec2(1100.0, 1200.0), vec2(1800.0, 1400.0)] {
                let mut app = new_app();
                app.ui.planner = state.clone();
                draw(&mut app, size);
                // what it read is in range afterwards
                let st = &app.ui.planner;
                assert!(st.lat.abs() <= 90.0 && st.lon.abs() <= 180.0 && st.sensor < SENSORS.len(), "{st:?}");
                assert!(st.saved.len() <= MAX_SAVED);
            }
        }
    }

    #[test]
    fn every_tab_draws() {
        let mut app = new_app();
        app.ui.planner = PlannerState { date: "2024-07-14".into(), utc_offset: Some(120), placed: true, ..Default::default() };
        for tab in [Tab::Sun, Tab::Night, Tab::Calendar, Tab::Moon] {
            app.ui.planner.tab = tab;
            draw(&mut app, vec2(1400.0, 1000.0));
        }
    }

    #[test]
    fn clock_text_follows_the_offset() {
        let noon_utc = 12.0 * 3600.0;
        assert_eq!(clock(noon_utc, 0), "12:00");
        assert_eq!(clock(noon_utc, 120), "14:00");
        assert_eq!(clock(noon_utc, -330), "06:30");
        assert_eq!(clock(f64::NAN, 0), "--:--");
        assert_eq!(clock(23.0 * 3600.0 + 59.0 * 60.0 + 40.0, 0), "00:00", "rounds to the next minute, wrapping");
    }

    #[test]
    fn clock_fields_read_back() {
        assert_eq!(parse_clock("22:30"), Some(1350.0));
        assert_eq!(parse_clock(" 6.05 "), Some(365.0));
        assert_eq!(parse_clock("24:00"), None);
        assert_eq!(parse_clock("12:60"), None);
        assert_eq!(parse_clock("noon"), None);
        assert_eq!(clock_minutes(1350.4), "22:30");
        assert_eq!(clock_minutes(-5.0), "00:00");
        assert_eq!(clock_minutes(9999.0), "23:59");
    }

    #[test]
    fn labels_read_well() {
        assert_eq!(utc_label(120), "UTC+02:00");
        assert_eq!(utc_label(-210), "UTC-03:30");
        assert_eq!(hm(65.0), "1 h 05 min");
        assert_eq!(hm(42.4), "42 min");
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(92.0), "E");
        assert_eq!(compass(225.0), "SW");
        assert_eq!(distance_text(450.0), "45 cm");
        assert_eq!(distance_text(2_507.0), "2.51 m");
        assert_eq!(distance_text(25_000.0), "25.0 m");
        assert_eq!(distance_text(f64::INFINITY), "∞");
        assert_eq!(seconds_text(8.55), "8.6 s");
        assert_eq!(seconds_text(250.0), "4 min 10 s");
    }

    #[test]
    fn state_read_from_a_bad_file_is_brought_into_range() {
        let mut st: PlannerState =
            serde_json::from_str(r#"{"lat": 400, "lon": "x", "focal": -3, "sensor": 99, "utcOffset": 9999, "minute": null}"#).unwrap_or_default();
        st.sane();
        assert!(st.lat.abs() <= 90.0 && st.lon.abs() <= 180.0);
        assert!(st.focal >= 1.0 && st.sensor < SENSORS.len() && st.utc_offset.is_none());
        let mut nan = PlannerState { lat: f64::NAN, aperture: f64::NAN, distance: f64::INFINITY, ..Default::default() };
        nan.sane();
        assert!(nan.lat.is_finite() && nan.aperture.is_finite() && nan.distance <= 100_000.0);
        // settings of an older version load, and a round trip keeps everything
        let plain: PlannerState = serde_json::from_str("{}").unwrap_or_default();
        assert_eq!(plain, PlannerState::default());
        let text = serde_json::to_string(&plain).unwrap_or_default();
        assert_eq!(serde_json::from_str::<PlannerState>(&text).ok(), Some(plain));
    }

    #[test]
    fn the_day_is_worked_out_for_the_place() {
        // Madrid, 21 June 2024, forced offset of two hours
        let st = PlannerState { date: "2024-06-21".into(), utc_offset: Some(120), placed: true, ..Default::default() };
        let c = compute(&st, 0.0);
        assert_eq!(c.date, Date::new(2024, 6, 21));
        assert_eq!(c.offset, 120);
        assert_eq!(c.sun.len(), SAMPLES);
        let rise = clock(c.plan.sun.rise.unwrap_or(0.0), c.offset);
        assert!(rise == "06:44" || rise == "06:45" || rise == "06:46", "{rise}");
        // the same place at the offset the program guesses: Madrid is on summer time
        let auto = compute(&PlannerState { utc_offset: None, ..st.clone() }, 0.0);
        assert_eq!(auto.offset, 120);
        let winter = compute(&PlannerState { date: "2024-01-15".into(), utc_offset: None, ..st }, 0.0);
        assert_eq!(winter.offset, 60);
    }

    #[test]
    fn the_cursor_defaults_to_sunset_on_other_days_and_to_now_today() {
        let st = PlannerState { date: "2024-06-21".into(), utc_offset: Some(120), placed: true, ..Default::default() };
        let c = compute(&st, 0.0);
        // a "now" on another day: sunset
        let m = cursor_minute(&st, &c, 0.0);
        let sunset = (c.plan.sun.set.unwrap_or(0.0) - c.start) / 60.0;
        assert!((m - sunset).abs() < 1e-6);
        // a "now" inside the day: now
        let inside = c.start + 10.0 * 3600.0;
        assert!((cursor_minute(&st, &c, inside) - 600.0).abs() < 1e-6);
        // a chosen minute wins, clamped
        let chosen = PlannerState { minute: Some(5000.0), ..st };
        assert_eq!(cursor_minute(&chosen, &c, inside), 1439.0);
    }

    #[test]
    fn sky_tints_follow_the_sun() {
        let pal = palette(true);
        assert!(sky_tint(40.0, true, pal).is_none(), "full daylight is untinted");
        let night = sky_tint(-30.0, true, pal).map(|c| c.a());
        let civil = sky_tint(-8.0, true, pal).map(|c| c.a());
        assert!(night > civil, "darker at night");
        assert!(sky_tint(2.0, true, pal).is_some(), "golden hour is tinted");
    }
}
