//! The outing index: one number, 0 to 100, for how good a night is for the Milky Way, from the things that decide it
//! (how long the core is up in a dark sky, how much of that the Moon spoils, how high the core gets, the sky's own
//! darkness where it is shot from, and the clouds).
//!
//! It is a way to compare nights, not a measurement: the weights are a photographer's rule of thumb. Clouds multiply
//! everything (an overcast night is a lost night whatever else is right); the rest are blended with exponents so that a
//! weak one pulls the score down without zeroing it. Without a forecast the clouds are left out and the score says so.

use crate::plan::NightScore;

/// Hours of the core in the dark that count as a full night (more adds nothing).
const FULL_DARK_MIN: f64 = 240.0;
/// The core is high enough for a full score at this altitude (degrees); the lowest that counts is [`crate::plan::CORE_MIN_ALT`].
const FULL_ALT: f64 = 35.0;

/// What held the score down most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// The core is up in the dark for too short a time (or never).
    Darkness,
    /// The Moon is up while the core is.
    Moon,
    /// The core stays low.
    Height,
    /// The sky is bright with artificial light at the place.
    Pollution,
    /// Clouds.
    Clouds,
    /// Nothing stands out: a very good night.
    None,
}

/// The index and its parts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outing {
    /// 0 (no night) to 100 (everything right).
    pub score: f64,
    /// Each factor, 0 to 1.
    pub darkness: f64,
    pub moon: f64,
    pub height: f64,
    /// `None` when the sky's brightness at the place is not known.
    pub sky: Option<f64>,
    /// `None` when there is no forecast for the night.
    pub clouds: Option<f64>,
    pub limit: Limit,
}

/// The index of a night. `moon_illumination` is the Moon's lit fraction (0..1) during it; `cloud_percent` the mean cloud cover
/// (0..100) over the hours the core is up in the dark, when a forecast covers them. `sky_mag` is the sky's brightness at
/// the place in magnitudes per square arcsecond (22 a pristine sky; higher is darker), when it is known.
pub fn outing_index(night: &NightScore, moon_illumination: f64, cloud_percent: Option<f64>, sky_mag: Option<f64>) -> Outing {
    let clamp = |v: f64| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let darkness = clamp(night.dark_core_minutes / FULL_DARK_MIN);
    let moonless = if night.dark_core_minutes > 0.0 { clamp(night.moonless_minutes / night.dark_core_minutes) } else { 0.0 };
    let moon = clamp(1.0 - (1.0 - moonless) * clamp(moon_illumination));
    let height = clamp((night.best_altitude - crate::plan::CORE_MIN_ALT) / (FULL_ALT - crate::plan::CORE_MIN_ALT));
    let clouds = cloud_percent.filter(|c| c.is_finite()).map(|c| clamp(1.0 - c / 100.0));
    let sky = sky_mag.filter(|m| m.is_finite()).map(sky_factor);
    let blended = darkness.powf(0.4) * moon.powf(0.4) * height.powf(0.2) * sky.unwrap_or(1.0).powf(0.6);
    let score = 100.0 * blended * clouds.unwrap_or(1.0);
    // the factor that costs most (each to its own weight)
    let costs = [
        (Limit::Darkness, 1.0 - darkness.powf(0.4)),
        (Limit::Moon, 1.0 - moon.powf(0.4)),
        (Limit::Height, 1.0 - height.powf(0.2)),
        (Limit::Pollution, 1.0 - sky.unwrap_or(1.0).powf(0.6)),
        (Limit::Clouds, 1.0 - clouds.unwrap_or(1.0)),
    ];
    let (limit, cost) = costs.into_iter().fold((Limit::None, 0.0), |best, c| if c.1 > best.1 { c } else { best });
    Outing { score, darkness, moon, height, sky, clouds, limit: if cost < 0.15 { Limit::None } else { limit } }
}

/// 1 for a pristine sky (22 mag/arcsec²) down to 0 at 18 (a city centre), bending so that the first magnitudes cost little.
pub fn sky_factor(mag: f64) -> f64 {
    (((mag - 18.0) / 4.0).clamp(0.0, 1.0)).powf(1.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn night(dark: f64, moonless: f64, alt: f64) -> NightScore {
        NightScore { day_start: 0.0, dark_core_minutes: dark, moonless_minutes: moonless, best_altitude: alt }
    }

    #[test]
    fn a_long_moonless_high_night_scores_full_and_clouds_take_it_away() {
        let best = outing_index(&night(300.0, 300.0, 40.0), 0.0, Some(0.0), None);
        assert!((best.score - 100.0).abs() < 1e-9 && best.limit == Limit::None);
        let overcast = outing_index(&night(300.0, 300.0, 40.0), 0.0, Some(100.0), None);
        assert_eq!(overcast.score, 0.0);
        assert_eq!(overcast.limit, Limit::Clouds);
        let half = outing_index(&night(300.0, 300.0, 40.0), 0.0, Some(50.0), None);
        assert!((half.score - 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_bright_moon_in_the_window_and_a_short_dark_spell_are_named() {
        let moony = outing_index(&night(300.0, 0.0, 40.0), 1.0, None, None);
        assert_eq!((moony.moon, moony.limit), (0.0, Limit::Moon));
        assert_eq!(moony.score, 0.0);
        let short = outing_index(&night(30.0, 30.0, 40.0), 0.0, None, None);
        assert_eq!(short.limit, Limit::Darkness);
        assert!(short.score > 0.0 && short.score < 70.0);
        let low = outing_index(&night(300.0, 300.0, 8.0), 0.0, None, None);
        assert_eq!(low.limit, Limit::Height);
    }

    #[test]
    fn no_dark_core_is_zero_and_hostile_numbers_do_not_panic() {
        assert_eq!(outing_index(&night(0.0, 0.0, -90.0), 0.0, Some(0.0), None).score, 0.0);
        let nan = outing_index(&night(f64::NAN, f64::INFINITY, f64::NAN), f64::NAN, Some(f64::NAN), None);
        assert!(nan.score.is_finite() && nan.score == 0.0);
        assert_eq!(outing_index(&night(300.0, 300.0, 40.0), 0.0, None, None).clouds, None);
    }

    #[test]
    fn a_bright_sky_costs_and_is_named() {
        let night = night(300.0, 300.0, 40.0);
        let dark = outing_index(&night, 0.0, Some(0.0), Some(22.0));
        assert!((dark.score - 100.0).abs() < 1e-9);
        let suburb = outing_index(&night, 0.0, Some(0.0), Some(19.5));
        assert!(suburb.score > 20.0 && suburb.score < 70.0 && suburb.limit == Limit::Pollution);
        assert_eq!(outing_index(&night, 0.0, Some(0.0), Some(17.0)).score, 0.0);
        assert!(sky_factor(21.0) > sky_factor(20.0) && sky_factor(25.0) == 1.0);
    }
}
