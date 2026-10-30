//! Astronomy for planning photographs: where the Sun, the Moon and the Milky Way's core are, when
//! they rise and set, the twilights and the golden and blue hours, moon phases, meteor showers,
//! and the lens arithmetic that goes with them (depth of field, star-trail exposure, field of view).
//! See README.md.
//!
//! Sources: Jean Meeus, *Astronomical Algorithms* (formulas, not code); the NPF rule (Michaud);
//! IMO meteor calendar (facts). Own code throughout.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod align;
pub mod eclipse;
pub mod horizon;
pub mod info;
pub mod moon;
pub mod optics;
pub mod plan;
pub mod search;
pub mod shade;
pub mod sky;
pub mod sun;
pub mod time;

pub use align::{Alignment, Body, Profile, Sight, alignments, destination, profile, shadow, sight};
pub use moon::{Phase, PhaseName, Principal, moon_coords, moon_coords_tt, moon_horizontal, moon_phase, next_phase, principal_phases};
pub use plan::{
    CoreDay, DayPlan, MoonDay, NightProfile, NightScore, Skies, Span, SunDay, SunState, Twilight, dark_core_windows, night_profile, plan_day,
    score_night, score_nights, skies_at,
};
pub use sky::{SHOWERS, Shower, core_horizontal};
pub use sun::{Horizontal, Observer, sun_horizontal};
pub use time::{DAY, Date};
