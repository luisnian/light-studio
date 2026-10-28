//! What a camera sees: a rectilinear projection of the sky and the skyline onto the sensor, for a direction (azimuth
//! and tilt), a focal length and a sensor in landscape or portrait. Exact for an ideal rectilinear lens; real lenses
//! distort (and wide ones vignette), which is not modelled.

/// A camera pointed somewhere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Degrees clockwise from north, and degrees above the horizon, of the centre of the picture.
    pub az: f64,
    pub alt: f64,
    pub focal: f64,
    /// The sensor across and up the picture, millimetres (swap them for portrait).
    pub width: f64,
    pub height: f64,
}

/// A point on the sensor, millimetres from the centre, `x` to the right and `y` up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SensorPoint {
    pub x: f64,
    pub y: f64,
    /// The angle between the direction and the axis of the lens, degrees.
    pub off_axis: f64,
}

fn unit(az: f64, alt: f64) -> [f64; 3] {
    let (a, h) = (az.to_radians(), alt.to_radians());
    [h.cos() * a.sin(), h.cos() * a.cos(), h.sin()]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl Frame {
    /// A frame, or `None` for numbers that make no picture.
    pub fn new(az: f64, alt: f64, focal: f64, width: f64, height: f64) -> Option<Frame> {
        let ok = [az, alt, focal, width, height].iter().all(|v| v.is_finite()) && focal > 0.0 && width > 0.0 && height > 0.0 && alt.abs() < 90.0;
        ok.then_some(Frame { az, alt, focal, width, height })
    }

    /// Where a direction (azimuth, altitude, degrees) lands on the sensor; `None` when it is behind the lens (more than
    /// 89° off the axis, where a rectilinear picture has nothing to show).
    pub fn project(&self, az: f64, alt: f64) -> Option<SensorPoint> {
        if !(az.is_finite() && alt.is_finite()) {
            return None;
        }
        let forward = unit(self.az, self.alt);
        let a0 = self.az.to_radians();
        let right = [a0.cos(), -a0.sin(), 0.0];
        let up = [
            right[1] * forward[2] - right[2] * forward[1],
            right[2] * forward[0] - right[0] * forward[2],
            right[0] * forward[1] - right[1] * forward[0],
        ];
        let v = unit(az, alt);
        let depth = dot(v, forward);
        if depth <= 0.017 {
            return None;
        }
        Some(SensorPoint {
            x: self.focal * dot(v, right) / depth,
            y: self.focal * dot(v, up) / depth,
            off_axis: depth.clamp(-1.0, 1.0).acos().to_degrees(),
        })
    }

    /// Whether a point on the sensor is inside the picture.
    pub fn inside(&self, p: SensorPoint) -> bool {
        p.x.abs() <= self.width / 2.0 && p.y.abs() <= self.height / 2.0
    }

    /// The width and height of the picture, degrees (across the middle).
    pub fn field(&self) -> Option<(f64, f64)> {
        Some((super::optics::field_of_view(self.width, self.focal)?, super::optics::field_of_view(self.height, self.focal)?))
    }

    /// The radius, on the sensor in millimetres, of a disc `diameter` degrees across at a point of the picture,
    /// measured across the direction away from the centre (the radial direction is stretched a little more at the edge).
    pub fn disc_radius(&self, diameter: f64, at: SensorPoint) -> f64 {
        let r = self.focal * (diameter.to_radians() / 2.0).tan();
        r / at.off_axis.to_radians().cos().max(0.2)
    }

    /// The same view with another focal length (the same direction and sensor).
    pub fn with_focal(&self, focal: f64) -> Option<Frame> {
        Frame::new(self.az, self.alt, focal, self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_frame(az: f64, alt: f64, focal: f64) -> Frame {
        Frame::new(az, alt, focal, 36.0, 24.0).expect("valid")
    }

    #[test]
    fn the_centre_of_the_picture_is_the_direction_aimed_at() {
        for (az, alt) in [(0.0, 0.0), (90.0, 10.0), (225.0, -20.0), (359.0, 45.0)] {
            let p = full_frame(az, alt, 35.0).project(az, alt).expect("in front");
            assert!(p.x.abs() < 1e-9 && p.y.abs() < 1e-9 && p.off_axis < 1e-6, "{p:?}");
        }
    }

    #[test]
    fn the_edge_of_the_picture_is_half_the_field_of_view_away() {
        let f = full_frame(180.0, 0.0, 24.0);
        let (w, h) = f.field().expect("valid");
        // 73.7 degrees across at 24 mm on 36 mm
        assert!((w - 73.74).abs() < 0.05, "{w}");
        let edge = f.project(180.0 + w / 2.0, 0.0).expect("in front");
        assert!((edge.x - 18.0).abs() < 1e-6 && edge.y.abs() < 1e-6, "{edge:?}");
        // up the middle: the vertical field
        let top = f.project(180.0, h / 2.0).expect("in front");
        assert!((top.y - 12.0).abs() < 1e-6 && top.x.abs() < 1e-9, "{top:?}");
    }

    #[test]
    fn east_is_to_the_right_when_facing_north_and_up_is_up() {
        let f = full_frame(0.0, 0.0, 50.0);
        assert!(f.project(5.0, 0.0).expect("in front").x > 0.0);
        assert!(f.project(355.0, 0.0).expect("in front").x < 0.0);
        assert!(f.project(0.0, 5.0).expect("in front").y > 0.0);
        // facing south, west is to the right
        let s = full_frame(180.0, 0.0, 50.0);
        assert!(s.project(185.0, 0.0).expect("in front").x > 0.0, "west is right when facing south");
        assert!(s.project(175.0, 0.0).expect("in front").x < 0.0);
    }

    #[test]
    fn the_horizon_is_straight_but_other_heights_bow() {
        // a great circle is a straight line in a rectilinear picture, tilted or not
        for tilt in [0.0, 30.0] {
            let f = full_frame(90.0, tilt, 24.0);
            let ys: Vec<f64> = [60.0, 90.0, 120.0].iter().map(|az| f.project(*az, 0.0).expect("in front").y).collect();
            assert!((ys[0] - ys[1]).abs() < 1e-9 && (ys[2] - ys[1]).abs() < 1e-9, "tilt {tilt}: {ys:?}");
        }
        // a line of constant height above the horizon is a curve: higher at the sides than in the middle
        let f = full_frame(90.0, 0.0, 24.0);
        let ys: Vec<f64> = [60.0, 90.0, 120.0].iter().map(|az| f.project(*az, 20.0).expect("in front").y).collect();
        assert!(ys[0] > ys[1] + 0.5 && ys[2] > ys[1] + 0.5, "{ys:?}");
    }

    #[test]
    fn what_is_behind_the_lens_is_not_projected() {
        let f = full_frame(0.0, 0.0, 24.0);
        assert_eq!(f.project(180.0, 0.0), None);
        assert_eq!(f.project(90.0, 0.0), None, "90 degrees off the axis has no rectilinear image");
        assert_eq!(f.project(f64::NAN, 0.0), None);
    }

    #[test]
    fn the_moon_is_small_on_a_wide_lens_and_big_on_a_long_one() {
        // 0.52 degrees at 200 mm: 200 x tan(0.26 deg) = 0.91 mm radius, 1.8 mm across
        let f = full_frame(150.0, 20.0, 200.0);
        let c = f.project(150.0, 20.0).expect("in front");
        assert!((f.disc_radius(0.52, c) - 0.907).abs() < 0.01);
        let wide = full_frame(150.0, 20.0, 24.0);
        assert!((wide.disc_radius(0.52, c) - 0.109).abs() < 0.005);
        // the sensor is 36 mm: the Moon at 200 mm fills 5 % of the width
        assert!((2.0 * f.disc_radius(0.52, c) / f.width - 0.05).abs() < 0.002);
    }

    #[test]
    fn nonsense_makes_no_frame() {
        assert!(Frame::new(0.0, 0.0, 0.0, 36.0, 24.0).is_none());
        assert!(Frame::new(f64::NAN, 0.0, 24.0, 36.0, 24.0).is_none());
        assert!(Frame::new(0.0, 90.0, 24.0, 36.0, 24.0).is_none());
        assert!(Frame::new(0.0, 0.0, 24.0, -1.0, 24.0).is_none());
    }

    #[test]
    fn the_sensor_can_be_turned_to_portrait() {
        let l = full_frame(0.0, 0.0, 35.0);
        let p = Frame::new(0.0, 0.0, 35.0, 24.0, 36.0).expect("valid");
        let (lw, lh) = l.field().expect("valid");
        let (pw, ph) = p.field().expect("valid");
        assert!((lw - ph).abs() < 1e-9 && (lh - pw).abs() < 1e-9);
    }
}
