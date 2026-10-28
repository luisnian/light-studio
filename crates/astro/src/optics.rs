//! Lens and sensor arithmetic for planning a frame: depth of field, hyperfocal distance, the
//! longest exposure before stars trail, and angle of view.
//!
//! Sources: depth of field and hyperfocal distance (textbook thin-lens formulas); the NPF rule
//! (Frederic Michaud, 2010: t = (35 N + 30 p) / f); the "500 rule" (folklore).

/// A sensor size, with the circle of confusion conventionally used for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sensor {
    pub name: &'static str,
    /// Width and height, millimetres.
    pub width: f64,
    pub height: f64,
    /// Circle of confusion, millimetres (the sensor diagonal / 1500).
    pub coc: f64,
}

impl Sensor {
    /// Focal length multiplier against 35 mm full frame.
    pub fn crop(&self) -> f64 {
        43.267 / self.width.hypot(self.height).max(1e-9)
    }

    /// Sensor width over the pixel columns, micrometres.
    pub fn pixel_pitch_um(&self, megapixels: f64) -> f64 {
        let pixels = megapixels.max(0.1) * 1e6;
        // the frame's aspect decides the columns
        let columns = (pixels * self.width / self.height).sqrt();
        self.width * 1000.0 / columns
    }
}

pub const SENSORS: [Sensor; 7] = [
    Sensor { name: "Full frame", width: 36.0, height: 24.0, coc: 0.029 },
    Sensor { name: "APS-C", width: 23.5, height: 15.6, coc: 0.019 },
    Sensor { name: "APS-C (Canon)", width: 22.3, height: 14.9, coc: 0.018 },
    Sensor { name: "Micro Four Thirds", width: 17.3, height: 13.0, coc: 0.015 },
    Sensor { name: "1 inch", width: 13.2, height: 8.8, coc: 0.0114 },
    Sensor { name: "Medium format 44x33", width: 43.8, height: 32.9, coc: 0.0365 },
    // appended last so that the indices saved by earlier versions keep their meaning
    Sensor { name: "Canon EOS 6D", width: 35.8, height: 23.9, coc: 0.029 },
];

/// The index of the Canon EOS 6D in [`SENSORS`] and its resolution (megapixels), the defaults of a new planner.
pub const CANON_6D: usize = 6;
pub const CANON_6D_MEGAPIXELS: f64 = 20.2;

/// Where the sharpness starts and ends when focused at a distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthOfField {
    /// Hyperfocal distance, millimetres.
    pub hyperfocal: f64,
    /// Nearest and farthest sharp distance, millimetres (`far` is `None` at infinity).
    pub near: f64,
    pub far: Option<f64>,
}

impl DepthOfField {
    /// Total depth, millimetres (`None` when it reaches infinity).
    pub fn total(&self) -> Option<f64> {
        self.far.map(|f| f - self.near)
    }
}

/// Depth of field for a focal length (mm), f-number, circle of confusion (mm) and focus distance (mm).
/// `None` when any number is not a positive finite one.
pub fn depth_of_field(focal: f64, fnumber: f64, coc: f64, distance: f64) -> Option<DepthOfField> {
    if ![focal, fnumber, coc, distance].iter().all(|v| v.is_finite() && *v > 0.0) || distance <= focal {
        return None;
    }
    let h = focal * focal / (fnumber * coc) + focal;
    let near = distance * (h - focal) / (h + distance - 2.0 * focal);
    let far = if distance >= h { None } else { Some(distance * (h - focal) / (h - distance)) };
    Some(DepthOfField { hyperfocal: h, near, far })
}

/// The longest exposure (seconds) before stars at the celestial equator trail, by the NPF rule, for
/// an f-number, pixel pitch (micrometres) and focal length (mm, the real one, not the equivalent).
/// A target at declination `dec` (degrees) moves slower by cos(dec), so it gets that much longer.
pub fn npf_seconds(fnumber: f64, pitch_um: f64, focal: f64, dec: f64) -> Option<f64> {
    if ![fnumber, pitch_um, focal].iter().all(|v| v.is_finite() && *v > 0.0) || !dec.is_finite() {
        return None;
    }
    let base = (35.0 * fnumber + 30.0 * pitch_um) / focal;
    let c = dec.to_radians().cos().abs().max(0.05);
    Some(base / c)
}

/// The "500 rule": seconds before stars trail, for an equivalent focal length (mm).
pub fn rule_500_seconds(focal_equivalent: f64) -> Option<f64> {
    (focal_equivalent.is_finite() && focal_equivalent > 0.0).then(|| 500.0 / focal_equivalent)
}

/// How far a star at declination `dec` (degrees) trails across the frame in `seconds`, in pixels:
/// the sky turns 15.041 arc seconds a second, and a pixel covers 206.265 x pitch / focal arc seconds.
pub fn star_trail_pixels(seconds: f64, pitch_um: f64, focal: f64, dec: f64) -> Option<f64> {
    if ![seconds, pitch_um, focal].iter().all(|v| v.is_finite() && *v > 0.0) || !dec.is_finite() {
        return None;
    }
    let pixel_scale = 206.265 * pitch_um / focal;
    Some(15.041 * seconds * dec.to_radians().cos().abs() / pixel_scale)
}

/// The shutter time (seconds) that keeps an exposure the same after putting on a neutral density filter
/// that cuts `stops` stops of light.
pub fn nd_shutter(base_seconds: f64, stops: f64) -> Option<f64> {
    (base_seconds.is_finite() && stops.is_finite() && base_seconds > 0.0 && (0.0..=30.0).contains(&stops)).then(|| base_seconds * 2f64.powf(stops))
}

/// A time-lapse: the photos to take and the interval between them (seconds), to turn `event_seconds`
/// of real time into a clip `clip_seconds` long at `fps` frames a second. `None` for numbers that
/// are not positive, or a clip of less than one frame.
pub fn timelapse(event_seconds: f64, clip_seconds: f64, fps: f64) -> Option<(u32, f64)> {
    if ![event_seconds, clip_seconds, fps].iter().all(|v| v.is_finite() && *v > 0.0) {
        return None;
    }
    let photos = (clip_seconds * fps).round();
    (1.0..=1.0e6).contains(&photos).then(|| (photos as u32, event_seconds / photos))
}

/// The longest shutter time (s) the camera sets by itself; longer needs Bulb (a Canon EOS 6D stops at 30 s).
pub const CAMERA_MAX_SHUTTER: f64 = 30.0;

/// An exposure bracket: `count` shutter times (an odd number, 3 to 15) `step` stops apart, the middle one the base
/// time, the shortest first. `None` for numbers out of range.
pub fn bracket(base_seconds: f64, step: f64, count: u32) -> Option<Vec<f64>> {
    if !(base_seconds.is_finite()
        && base_seconds > 0.0
        && step.is_finite()
        && (0.1..=4.0).contains(&step)
        && (3..=15).contains(&count)
        && count % 2 == 1)
    {
        return None;
    }
    let half = i64::from(count / 2);
    Some((-half..=half).map(|i| base_seconds * 2f64.powf(step * i as f64)).collect())
}

/// A time-lapse with exposures in it: what [`timelapse_plan`] works out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelapsePlan {
    pub photos: u32,
    /// Seconds from the start of one frame to the start of the next.
    pub interval: f64,
    /// Seconds left between the end of one exposure and the start of the next.
    pub gap: f64,
    /// Whether the gap is at least `MIN_GAP` (the camera needs a moment to save the frame).
    pub feasible: bool,
}

/// The least gap (s) a camera needs between two exposures to write the frame (a rough, conservative figure).
pub const MIN_GAP: f64 = 1.0;

/// As [`timelapse`], also checking that the exposure (seconds) fits in the interval.
pub fn timelapse_plan(event_seconds: f64, clip_seconds: f64, fps: f64, exposure: f64) -> Option<TimelapsePlan> {
    if !(exposure.is_finite() && exposure > 0.0) {
        return None;
    }
    let (photos, interval) = timelapse(event_seconds, clip_seconds, fps)?;
    let gap = interval - exposure;
    Some(TimelapsePlan { photos, interval, gap, feasible: gap >= MIN_GAP })
}

/// Angle of view (degrees) across `size` millimetres of sensor at a focal length (mm).
pub fn field_of_view(size: f64, focal: f64) -> Option<f64> {
    (size.is_finite() && focal.is_finite() && size > 0.0 && focal > 0.0).then(|| 2.0 * (size / (2.0 * focal)).atan().to_degrees())
}

/// A panorama shot as a grid of frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanoramaPlan {
    pub columns: u32,
    pub rows: u32,
    /// Angle covered by one frame across and up, degrees.
    pub fov_w: f64,
    pub fov_h: f64,
    /// Angle between the centres of two neighbouring frames, degrees.
    pub step_w: f64,
    pub step_h: f64,
    /// Angle the whole grid covers, degrees (at least what was asked for; the full circle when it closes on itself).
    pub covered_w: f64,
    pub covered_h: f64,
}

impl PanoramaPlan {
    pub fn shots(&self) -> u32 {
        self.columns.saturating_mul(self.rows)
    }
}

/// The frames needed to cover `total` degrees with frames of `fov` degrees overlapping by `overlap` (a fraction): the step
/// is the frame's angle less the overlap, measured in angle. Across a flat (rectilinear) picture the stretched edges hold
/// more pixels per degree than the middle, so the overlap in pixels is at least this, never less.
fn frames_for(total: f64, fov: f64, overlap: f64, closes: bool) -> (u32, f64, f64) {
    let step = fov * (1.0 - overlap);
    let n = if closes {
        (total / step - 1e-9).ceil()
    } else if total <= fov {
        1.0
    } else {
        ((total - fov) / step - 1e-9).ceil() + 1.0
    };
    let n = n.clamp(1.0, 1000.0);
    let covered = if closes { total } else { fov + (n - 1.0) * step };
    (n as u32, step, covered)
}

/// The grid of frames for a scene `width_deg` across and `height_deg` high, shot with frames of `frame_w` x `frame_h`
/// millimetres of sensor (turned for portrait) at `focal` millimetres, each overlapping the next by `overlap` (0.05 to 0.9).
/// A scene of 360 degrees closes on itself. It assumes the camera turns about the lens's entrance pupil.
pub fn panorama(frame_w: f64, frame_h: f64, focal: f64, width_deg: f64, height_deg: f64, overlap: f64) -> Option<PanoramaPlan> {
    if !(width_deg.is_finite() && height_deg.is_finite() && overlap.is_finite() && width_deg > 0.0 && height_deg > 0.0) {
        return None;
    }
    let (fov_w, fov_h) = (field_of_view(frame_w, focal)?, field_of_view(frame_h, focal)?);
    let overlap = overlap.clamp(0.05, 0.9);
    let width = width_deg.min(360.0);
    let height = height_deg.min(180.0);
    let (columns, step_w, covered_w) = frames_for(width, fov_w, overlap, width >= 360.0);
    let (rows, step_h, covered_h) = frames_for(height, fov_h, overlap, false);
    Some(PanoramaPlan { columns, rows, fov_w, fov_h, step_w, step_h, covered_w, covered_h })
}

/// Hyperfocal distance, millimetres.
pub fn hyperfocal(focal: f64, fnumber: f64, coc: f64) -> Option<f64> {
    depth_of_field(focal, fnumber, coc, focal * 2.0 + 1.0).map(|d| d.hyperfocal)
}

/// The circle of confusion (mm) that keeps a point looking sharp in a print: the eye resolves
/// `acuity_arcmin` (1 arc minute for good eyesight) at `viewing_mm`, and the print magnifies the
/// sensor by `print_diagonal_mm / sensor_diagonal_mm`.
pub fn coc_for_print(sensor_diagonal_mm: f64, print_diagonal_mm: f64, viewing_mm: f64, acuity_arcmin: f64) -> Option<f64> {
    if ![sensor_diagonal_mm, print_diagonal_mm, viewing_mm, acuity_arcmin].iter().all(|v| v.is_finite() && *v > 0.0) {
        return None;
    }
    let blur_on_print = viewing_mm * (acuity_arcmin / 60.0).to_radians().tan();
    Some(blur_on_print * sensor_diagonal_mm / print_diagonal_mm)
}

/// The inverse of depth of field: the f-number and focus distance (mm) that make everything between
/// `near` and `far` (mm) sharp, for a focal length and circle of confusion.
pub fn inverse_depth_of_field(focal: f64, coc: f64, near: f64, far: f64) -> Option<(f64, f64)> {
    if ![focal, coc, near, far].iter().all(|v| v.is_finite() && *v > 0.0) || far <= near || near <= focal {
        return None;
    }
    let h = 2.0 * near * far / (far - near);
    let distance = 2.0 / (1.0 / near + 1.0 / far);
    Some((focal * focal / (coc * (h - focal)), distance))
}

/// Exposure value at ISO 100: log2(N^2 / t) minus the ISO's stops over 100.
pub fn exposure_value(fnumber: f64, shutter: f64, iso: f64) -> Option<f64> {
    if ![fnumber, shutter, iso].iter().all(|v| v.is_finite() && *v > 0.0) {
        return None;
    }
    Some((fnumber * fnumber / shutter).log2() - (iso / 100.0).log2())
}

/// The shutter time (s) that gives the exposure value `ev` (at ISO 100) at an f-number and ISO.
pub fn shutter_for_ev(fnumber: f64, iso: f64, ev: f64) -> Option<f64> {
    if ![fnumber, iso, ev].iter().all(|v| v.is_finite()) || fnumber <= 0.0 || iso <= 0.0 {
        return None;
    }
    Some(fnumber * fnumber / 2f64.powf(ev + (iso / 100.0).log2()))
}

/// Typical exposure values (ISO 100) by light, for a first guess.
pub const LIGHTS: [(&str, f64); 12] = [
    ("Bright sun on sand or snow", 16.0),
    ("Clear sunny day", 15.0),
    ("Hazy sun", 14.0),
    ("Light cloud", 13.0),
    ("Overcast", 12.0),
    ("Open shade", 12.0),
    ("Sunset or sunrise", 11.0),
    ("Street lights at night", 6.0),
    ("Blue hour", 5.0),
    ("Full moon landscape", -2.0),
    ("Quarter moon landscape", -4.0),
    ("Milky Way", -7.0),
];

/// The longest exposure (s) that keeps stars as points: the sky turns 15.041 arc seconds a second,
/// and a star may smear `max_pixels` pixels.
pub fn stars_as_points_seconds(pitch_um: f64, focal: f64, dec: f64, max_pixels: f64) -> Option<f64> {
    if ![pitch_um, focal, max_pixels].iter().all(|v| v.is_finite() && *v > 0.0) || !dec.is_finite() {
        return None;
    }
    let pixel_scale = 206.265 * pitch_um / focal;
    Some(max_pixels * pixel_scale / (15.041 * dec.to_radians().cos().abs().max(0.05)))
}

/// How far (mm) to stand from a subject `subject_mm` long to fill `sensor_mm` of the frame with a
/// lens of `focal` mm (thin-lens: distance = focal x (1 + subject / sensor)).
pub fn subject_distance(focal: f64, subject_mm: f64, sensor_mm: f64) -> Option<f64> {
    ([focal, subject_mm, sensor_mm].iter().all(|v| v.is_finite() && *v > 0.0)).then(|| focal * (1.0 + subject_mm / sensor_mm))
}

/// The focal length on sensor `to` that frames what `focal` frames on sensor `from`.
pub fn equivalent_focal(focal: f64, from: &Sensor, to: &Sensor) -> Option<f64> {
    (focal.is_finite() && focal > 0.0).then(|| focal * from.crop() / to.crop())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyperfocal_of_a_wide_lens() {
        // 24 mm f/8 full frame: 24^2 / (8 x 0.029) + 24 = 2507 mm
        let d = depth_of_field(24.0, 8.0, 0.029, 5_000.0).expect("valid");
        assert!((d.hyperfocal - 2_506.8).abs() < 1.0, "{}", d.hyperfocal);
        assert_eq!(d.far, None, "focused beyond the hyperfocal: sharp to infinity");
        assert!(d.near > 1_600.0 && d.near < 1_700.0, "{}", d.near);
        // focused on the hyperfocal distance: sharp from half of it
        let at_h = depth_of_field(24.0, 8.0, 0.029, d.hyperfocal).expect("valid");
        assert!((at_h.near - d.hyperfocal / 2.0).abs() < 20.0, "{}", at_h.near);
        assert_eq!(at_h.far, None);
    }

    #[test]
    fn depth_of_field_of_a_portrait_lens() {
        // 50 mm f/2.8 on full frame at 3 m: about 2.74 m to 3.32 m
        let d = depth_of_field(50.0, 2.8, 0.029, 3_000.0).expect("valid");
        assert!((d.near - 2_737.0).abs() < 10.0, "{}", d.near);
        assert!((d.far.expect("finite") - 3_318.0).abs() < 10.0, "{:?}", d.far);
        assert!((d.total().expect("finite") - 581.0).abs() < 20.0);
    }

    #[test]
    fn depth_of_field_rejects_nonsense() {
        assert_eq!(depth_of_field(0.0, 2.8, 0.029, 3_000.0), None);
        assert_eq!(depth_of_field(50.0, -1.0, 0.029, 3_000.0), None);
        assert_eq!(depth_of_field(50.0, 2.8, 0.029, 40.0), None, "focused inside the focal length");
        assert_eq!(depth_of_field(f64::NAN, 2.8, 0.029, 3_000.0), None);
    }

    #[test]
    fn npf_matches_the_published_formula() {
        // 24 mm f/2 on a 4.51 um sensor: (70 + 135.3) / 24 = 8.55 s
        let t = npf_seconds(2.0, 4.51, 24.0, 0.0).expect("valid");
        assert!((t - 8.554).abs() < 0.01, "{t}");
        // a target near the pole trails more slowly: longer exposures are allowed
        assert!(npf_seconds(2.0, 4.51, 24.0, 60.0).expect("valid") > 1.9 * t);
        assert_eq!(npf_seconds(0.0, 4.51, 24.0, 0.0), None);
        assert!(npf_seconds(2.0, 4.51, 24.0, 90.0).expect("capped").is_finite());
    }

    #[test]
    fn star_trails_in_pixels() {
        // 24 mm, 4.51 um: a pixel is 38.8"; the sky moves 15.04"/s, so 30 s is 11.6 px at the equator
        let px = star_trail_pixels(30.0, 4.51, 24.0, 0.0).expect("valid");
        assert!((px - 11.64).abs() < 0.05, "{px}");
        // the NPF time is the one that keeps the trail near two pixels at the equator
        let t = npf_seconds(2.0, 4.51, 24.0, 0.0).expect("valid");
        assert!(star_trail_pixels(t, 4.51, 24.0, 0.0).expect("valid") < 3.5);
        assert!(star_trail_pixels(30.0, 4.51, 24.0, 89.0).expect("valid") < 0.3, "near the pole they hardly move");
        assert_eq!(star_trail_pixels(0.0, 4.51, 24.0, 0.0), None);
    }

    #[test]
    fn neutral_density_and_timelapse() {
        assert_eq!(nd_shutter(1.0 / 125.0, 10.0), Some(8.192));
        assert_eq!(nd_shutter(1.0, 0.0), Some(1.0));
        assert_eq!(nd_shutter(1.0, 40.0), None);
        assert_eq!(nd_shutter(-1.0, 3.0), None);
        // a 3 hour sunset in a 20 s clip at 25 fps: 500 photos, one every 21.6 s
        let (n, interval) = timelapse(3.0 * 3600.0, 20.0, 25.0).expect("valid");
        assert_eq!(n, 500);
        assert!((interval - 21.6).abs() < 1e-9);
        assert_eq!(timelapse(3600.0, 0.0, 25.0), None);
        assert_eq!(timelapse(3600.0, 0.01, 25.0), None, "less than a frame");
        assert_eq!(timelapse(f64::NAN, 20.0, 25.0), None);
    }

    #[test]
    fn the_500_rule() {
        assert_eq!(rule_500_seconds(20.0), Some(25.0));
        assert_eq!(rule_500_seconds(0.0), None);
    }

    #[test]
    fn angle_of_view() {
        // 35 mm across the 36 mm frame is 54.4 degrees
        assert!((field_of_view(36.0, 35.0).expect("valid") - 54.43).abs() < 0.05);
        assert_eq!(field_of_view(36.0, 0.0), None);
    }

    #[test]
    fn sensors_are_consistent() {
        assert!((SENSORS[0].crop() - 1.0).abs() < 0.01);
        assert!((SENSORS[1].crop() - 1.53).abs() < 0.03);
        assert!((SENSORS[3].crop() - 2.0).abs() < 0.05);
        // a 45 MP full-frame sensor has a pixel pitch of about 4.4 um
        let p = SENSORS[0].pixel_pitch_um(45.0);
        assert!((p - 4.4).abs() < 0.1, "{p}");
        for s in &SENSORS {
            assert!(s.coc > 0.0 && (s.coc - s.width.hypot(s.height) / 1500.0).abs() < 0.004, "{}", s.name);
        }
    }

    #[test]
    fn coc_for_a_print_and_its_inverse_dof() {
        // full frame (43.27 mm diagonal) printed 20 x 30 in (36 in diagonal = 914 mm) seen from 1 m at 1'
        let c = coc_for_print(43.27, 914.0, 1000.0, 1.0).unwrap_or(0.0);
        assert!((c - 0.0138).abs() < 0.002, "{c}");
        assert!(coc_for_print(0.0, 1.0, 1.0, 1.0).is_none());
        // the inverse recovers the setting
        let d = depth_of_field(24.0, 8.0, 0.029, 1500.0).unwrap();
        let (n, s) = inverse_depth_of_field(24.0, 0.029, d.near, d.far.unwrap()).unwrap();
        assert!((n - 8.0).abs() < 0.3 && (s - 1500.0).abs() < 30.0, "{n} {s}");
        assert!(inverse_depth_of_field(24.0, 0.029, 3000.0, 2000.0).is_none());
    }

    #[test]
    fn exposure_value_round_trips() {
        let ev = exposure_value(16.0, 1.0 / 100.0, 100.0).unwrap_or(0.0);
        assert!((ev - 14.64).abs() < 0.05, "{ev}");
        // sunny 16: f/16, 1/ISO
        let t = shutter_for_ev(16.0, 100.0, 15.0).unwrap_or(0.0);
        assert!((t - 1.0 / 128.0).abs() < 1e-4);
        let t2 = shutter_for_ev(2.8, 3200.0, -7.0).unwrap_or(0.0);
        assert!((exposure_value(2.8, t2, 3200.0).unwrap_or(0.0) + 7.0).abs() < 1e-9);
        assert!(exposure_value(0.0, 1.0, 100.0).is_none());
    }

    #[test]
    fn the_canon_6d_is_the_default_camera() {
        let s = SENSORS[CANON_6D];
        assert_eq!(s.name, "Canon EOS 6D");
        // 5472 x 3648 pixels over 35.8 mm: 6.54 µm
        let pitch = s.pixel_pitch_um(5472.0 * 3648.0 / 1e6);
        assert!((pitch - 6.54).abs() < 0.02, "{pitch}");
        // the megapixels the planner starts with give about the same pitch
        assert!((s.pixel_pitch_um(CANON_6D_MEGAPIXELS) - pitch).abs() < 0.1);
        // 35.8 mm across at 24 mm: 71.5°
        assert!((field_of_view(s.width, 24.0).expect("valid") - 73.9).abs() < 0.5);
    }

    #[test]
    fn brackets_are_centred_and_in_stops() {
        let b = bracket(1.0, 1.0, 5).expect("valid");
        assert_eq!(b, vec![0.25, 0.5, 1.0, 2.0, 4.0]);
        let third = bracket(1.0, 1.0 / 3.0, 3).expect("valid");
        assert!((third[0] - 2f64.powf(-1.0 / 3.0)).abs() < 1e-12 && third[1] == 1.0);
        assert_eq!(bracket(1.0, 1.0, 4), None, "an even count has no middle");
        assert_eq!(bracket(1.0, 0.0, 3), None);
        assert_eq!(bracket(0.0, 1.0, 3), None);
        assert_eq!(bracket(f64::NAN, 1.0, 3), None);
        assert_eq!(bracket(1.0, 1.0, 99), None);
    }

    #[test]
    fn a_timelapse_checks_that_the_exposure_fits() {
        // 3 h into 20 s at 25 fps: 500 photos, one every 21.6 s
        let p = timelapse_plan(3.0 * 3600.0, 20.0, 25.0, 15.0).expect("valid");
        assert_eq!(p.photos, 500);
        assert!((p.interval - 21.6).abs() < 1e-9 && (p.gap - 6.6).abs() < 1e-9 && p.feasible);
        // a 30 s exposure cannot repeat every 21.6 s
        let q = timelapse_plan(3.0 * 3600.0, 20.0, 25.0, 30.0).expect("valid");
        assert!(q.gap < 0.0 && !q.feasible);
        assert_eq!(timelapse_plan(3600.0, 20.0, 25.0, 0.0), None);
        assert_eq!(timelapse_plan(3600.0, 0.0, 25.0, 1.0), None);
    }

    #[test]
    fn points_distance_and_equivalence() {
        // a 14 mm lens, 4.3 um pixels: ~ NPF-like order of seconds
        let t = stars_as_points_seconds(4.3, 14.0, 0.0, 1.0).unwrap_or(0.0);
        assert!((t - 4.2).abs() < 0.3, "{t}");
        // 50 mm, 1.8 m subject, 24 mm frame height: 50 x (1 + 75) = 3.8 m
        assert!((subject_distance(50.0, 1800.0, 24.0).unwrap_or(0.0) - 3800.0).abs() < 1.0);
        let (ff, apsc) = (SENSORS[0], SENSORS[1]);
        let f = equivalent_focal(35.0, &apsc, &ff).unwrap_or(0.0);
        assert!((f - 53.7).abs() < 1.0, "{f}");
        assert!(hyperfocal(24.0, 8.0, 0.029).is_some());
    }

    #[test]
    fn panorama_grid_covers_what_was_asked() {
        // full frame, 24 mm: 73.7 x 53.1 degrees a frame; 30 % overlap → steps of 51.6 and 37.2
        let p = panorama(36.0, 24.0, 24.0, 180.0, 40.0, 0.3).unwrap();
        assert!((p.fov_w - 73.74).abs() < 0.05 && (p.fov_h - 53.13).abs() < 0.05);
        assert_eq!((p.columns, p.rows), (4, 1));
        assert!(p.covered_w >= 180.0 && p.covered_h >= 40.0);
        // one fewer column would not cover it
        assert!(p.fov_w + (f64::from(p.columns) - 2.0) * p.step_w < 180.0);
        // a scene that fits one frame is one shot
        assert_eq!(panorama(36.0, 24.0, 24.0, 60.0, 40.0, 0.3).unwrap().shots(), 1);
        // more overlap never needs fewer shots; a longer lens never needs fewer either
        let shots = |f: f64, o: f64| panorama(36.0, 24.0, f, 360.0, 90.0, o).unwrap().shots();
        assert!(shots(24.0, 0.5) >= shots(24.0, 0.2) && shots(50.0, 0.3) >= shots(24.0, 0.3));
        // the full circle closes: n frames of step s go round exactly
        let c = panorama(36.0, 24.0, 24.0, 360.0, 30.0, 0.3).unwrap();
        assert!(f64::from(c.columns) * c.step_w >= 360.0 - 1e-6 && (f64::from(c.columns) - 1.0) * c.step_w < 360.0);
    }

    #[test]
    fn panorama_refuses_nonsense() {
        for (w, h, f, a, b, o) in [
            (36.0, 24.0, 0.0, 90.0, 30.0, 0.3),
            (36.0, 24.0, -1.0, 90.0, 30.0, 0.3),
            (0.0, 24.0, 24.0, 90.0, 30.0, 0.3),
            (36.0, 24.0, 24.0, f64::NAN, 30.0, 0.3),
            (36.0, 24.0, 24.0, 90.0, 0.0, 0.3),
            (36.0, 24.0, 24.0, 90.0, 30.0, f64::NAN),
        ] {
            assert!(panorama(w, h, f, a, b, o).is_none());
        }
        // absurd inputs stay bounded
        let p = panorama(36.0, 24.0, 1e6, 1e9, 1e9, 5.0).unwrap();
        assert!(p.columns <= 1000 && p.rows <= 1000);
    }
}
