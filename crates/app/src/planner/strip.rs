//! Jumping the day and the time bar to a moment (a click on a time or a date in the cards). The strip of information
//! cells that used to run along the top of the map was removed: it covered the map and added nothing the Information
//! card does not show.

use super::*;

/// Moves the day and the time bar to a moment (Unix seconds) on the clock of a zone `offset` minutes east of UTC.
pub(super) fn jump_to(st: &mut PlannerState, unix: f64, offset: i32) {
    if !unix.is_finite() {
        return;
    }
    let d = Date::from_unix(unix, offset);
    let midnight = d.unix_midnight_utc() - f64::from(offset) * 60.0;
    st.date = d.iso();
    st.minute = Some(((unix - midnight) / 60.0).clamp(0.0, 1439.0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use light_studio_astro::time::unix_from_civil;

    #[test]
    fn a_jump_lands_on_the_day_and_minute_of_the_places_clock() {
        let mut st = PlannerState::default();
        // 20:30 UTC in a zone two hours east is 22:30 the same day
        jump_to(&mut st, unix_from_civil(2024, 8, 12, 20, 30, 0), 120);
        assert_eq!(st.date, "2024-08-12");
        assert_eq!(st.minute, Some(22.0 * 60.0 + 30.0));
        // 23:00 UTC two hours east is already the next day at 01:00
        jump_to(&mut st, unix_from_civil(2024, 8, 12, 23, 0, 0), 120);
        assert_eq!(st.date, "2024-08-13");
        assert_eq!(st.minute, Some(60.0));
        // a time that is not a number changes nothing
        jump_to(&mut st, f64::NAN, 120);
        assert_eq!(st.date, "2024-08-13");
    }

    #[test]
    fn settings_read_from_a_file_are_brought_into_range() {
        use light_studio_astro::sky::SHOWERS;
        let mut st =
            PlannerState { find_az: f64::NAN, shower: 99, meteor: Some(99), find_days: 1.0e9, to_sensor: 77, iso: -5.0, ..Default::default() };
        st.sane();
        assert!(st.find_az.is_finite() && st.shower < SHOWERS.len());
        assert_eq!(st.meteor, None);
        assert!(st.find_days <= 400.0 && st.to_sensor < SENSORS.len() && st.iso >= 6.0);
    }
}
