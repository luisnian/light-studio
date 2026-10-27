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

pub const SENSORS: [Sensor; 6] = [
    Sensor { name: "Full frame", width: 36.0, height: 24.0, coc: 0.029 },
    Sensor { name: "APS-C", width: 23.5, height: 15.6, coc: 0.019 },
    Sensor { name: "APS-C (Canon)", width: 22.3, height: 14.9, coc: 0.018 },
    Sensor { name: "Micro Four Thirds", width: 17.3, height: 13.0, coc: 0.015 },
    Sensor { name: "1 inch", width: 13.2, height: 8.8, coc: 0.0114 },
    Sensor { name: "Medium format 44x33", width: 43.8, height: 32.9, coc: 0.0365 },
];

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

/// Angle of view (degrees) across `size` millimetres of sensor at a focal length (mm).
pub fn field_of_view(size: f64, focal: f64) -> Option<f64> {
    (size.is_finite() && focal.is_finite() && size > 0.0 && focal > 0.0).then(|| 2.0 * (size / (2.0 * focal)).atan().to_degrees())
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
}
