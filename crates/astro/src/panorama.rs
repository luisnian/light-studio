//! The mountains as a picture: the ground all round a place drawn as it is seen from it (PeakFinder's view, in colour).
//!
//! The ground is the grid of heights the skyline is made from (directions × rings of distance, see
//! [`crate::horizon::Grid`]). Seen from the place, a point's height above the camera's level is an angle; walking out
//! along one direction, the ground is in view wherever that angle is higher than every nearer one. Each such stretch is a
//! band of the picture, as high as the angle rose since the last visible ring, and it takes that ring's colour. Done for
//! every column of the picture, that is the whole panorama, with the near ground below and the far ridges above.
//!
//! The colour of a sample is its own (from a table by height and steepness, or from an aerial photo draped over the
//! ground), lit by the Sun where it is at the time shown (the slope against the light, warm when low, dark at night),
//! and paled toward the colour of the sky at the horizon the farther it is.

use crate::align::{EARTH_R, EYE, REFRACTION_K};
use crate::horizon::Grid;
use crate::sun::Horizontal;

/// The ground round a place, ready to be coloured and drawn.
#[derive(Clone, Debug)]
pub struct Surface {
    grid: Grid,
    rings: Vec<f64>,
    /// Metres above the sea, direction by direction, ring by ring.
    heights: Vec<f32>,
    /// Degrees above the camera's level (with the Earth's curve and the usual refraction), same order.
    angle: Vec<f32>,
}

/// What part of the world a picture shows and how big it is: `width × height` pixels, the left edge looking at azimuth
/// `az_left`, `deg_x` and `deg_y` degrees to a pixel across and down, the top edge at `alt_top` degrees of height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub width: usize,
    pub height: usize,
    pub az_left: f64,
    pub deg_x: f64,
    pub alt_top: f64,
    pub deg_y: f64,
}

/// A sample's aerial photo colour: red, green, blue and 255, or alpha 0 where the photo has nothing.
pub type Photo = [u8; 4];

impl Surface {
    /// How the ground was sampled.
    pub fn grid(&self) -> Grid {
        self.grid
    }

    /// The ground from `heights` listed as `grid.sample_points` does (the place first). `None` when the list is not that
    /// long or holds something that is not a number.
    pub fn new(grid: Grid, heights: &[f64]) -> Option<Surface> {
        if grid.azimuths < 8 || grid.rings < 2 || heights.len() != grid.points() || heights.iter().any(|h| !h.is_finite()) {
            return None;
        }
        let eye = *heights.first()? + EYE;
        let rings = grid.ranges();
        let samples = heights.get(1..)?;
        let mut hs = Vec::with_capacity(samples.len());
        let mut angle = Vec::with_capacity(samples.len());
        for row in samples.chunks(grid.rings) {
            for (h, d) in row.iter().zip(&rings) {
                let drop = d * d * (1.0 - REFRACTION_K) / (2.0 * EARTH_R);
                angle.push(((h - eye - drop) / d).atan().to_degrees() as f32);
                hs.push(*h as f32);
            }
        }
        Some(Surface { grid, rings, heights: hs, angle })
    }

    /// How many samples there are (directions × rings): the length of [`Surface::colours`] and of a photo list.
    pub fn len(&self) -> usize {
        self.heights.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    fn height(&self, i: usize, j: usize) -> f64 {
        f64::from(self.heights.get(i * self.grid.rings + j).copied().unwrap_or(0.0))
    }

    /// The colour of every sample, direction by direction: its own, lit by the Sun at `sun`, paled toward `haze` (the
    /// sky's colour at the horizon) by distance. `photo` is the aerial photo colour of each sample, where there is one
    /// (a sample without it takes the table's colour).
    pub fn colours(&self, sun: Horizontal, photo: Option<&[Photo]>, haze: [u8; 3]) -> Vec<[u8; 3]> {
        let (n, m) = (self.grid.azimuths, self.grid.rings);
        let step = std::f64::consts::TAU / n as f64;
        let (alt, saz) = (sun.alt.to_radians(), sun.az.to_radians());
        let day = smooth(-6.0, 6.0, sun.alt);
        let low = 1.0 - (sun.alt / 25.0).clamp(0.0, 1.0);
        let sun_col = [1.0, 0.92 - 0.22 * low, 0.82 - 0.42 * low];
        let sky_tint = [0.86, 0.93, 1.0];
        let ambient = 0.04 + 0.28 * day;
        // the light on a lit slope in full day, to bring the photo's own light (and the table's) to a common level
        let reference = |c: usize| ambient_of(0.32, sky_tint, c) + 0.9 * 0.55;
        let mut out = Vec::with_capacity(n * m);
        for i in 0..n {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            // the Sun along the direction, across it and up
            let (sr, st, su) = (alt.cos() * (saz - a).cos(), alt.cos() * (saz - a).sin(), alt.sin());
            let (prev, next) = ((i + n - 1) % n, (i + 1) % n);
            for j in 0..m {
                let d = self.rings.get(j).copied().unwrap_or(1.0);
                let (j0, j1) = (j.saturating_sub(1), (j + 1).min(m - 1));
                let span = (self.rings.get(j1).copied().unwrap_or(d) - self.rings.get(j0).copied().unwrap_or(d)).max(1.0);
                let dzdr = (self.height(i, j1) - self.height(i, j0)) / span;
                let dzdt = (self.height(next, j) - self.height(prev, j)) / (2.0 * step * d);
                let steep = dzdr.hypot(dzdt);
                let lambert = ((-dzdr * sr - dzdt * st + su) / (1.0 + steep * steep).sqrt()).max(0.0);
                let h = self.height(i, j);
                let own = photo.and_then(|p| p.get(i * m + j)).filter(|p| p[3] > 0).map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
                let base = own.unwrap_or_else(|| ground_colour(h, steep.atan().to_degrees()));
                let mut c = [0.0; 3];
                for (k, v) in c.iter_mut().enumerate() {
                    let light = ambient_of(ambient, sky_tint, k) + 0.9 * day * lambert * sun_col.get(k).copied().unwrap_or(1.0);
                    *v = (base.get(k).copied().unwrap_or(0.0) * (light / reference(k)).min(1.4)).min(255.0);
                }
                let pale = (1.0 - (-d / 28_000.0).exp()) * 0.85;
                let px = [0, 1, 2].map(|k| {
                    let far = f64::from(haze.get(k).copied().unwrap_or(128));
                    (c.get(k).copied().unwrap_or(0.0) * (1.0 - pale) + far * pale).round().clamp(0.0, 255.0) as u8
                });
                out.push(px);
            }
        }
        out
    }

    /// The picture of `view` as RGBA (rows top to bottom): the ground from `colours` (as [`Surface::colours`] listed
    /// them), transparent where there is sky. Empty when `colours` is not one per sample or the view has no size.
    pub fn render(&self, colours: &[[u8; 3]], view: &View) -> Vec<u8> {
        let (w, h) = (view.width, view.height);
        let ok =
            w > 0 && h > 0 && w <= 8192 && h <= 8192 && view.deg_x > 0.0 && view.deg_y > 0.0 && view.az_left.is_finite() && view.alt_top.is_finite();
        if !ok || colours.len() != self.heights.len() {
            return Vec::new();
        }
        let (n, m) = (self.grid.azimuths, self.grid.rings);
        let mut out = vec![0u8; w * h * 4];
        let row_of = |alt: f64| (view.alt_top - alt) / view.deg_y;
        for x in 0..w {
            let az = (view.az_left + (x as f64 + 0.5) * view.deg_x).rem_euclid(360.0);
            let at = az / 360.0 * n as f64;
            let i0 = (at.floor() as usize) % n;
            let (i1, f) = ((i0 + 1) % n, (at - at.floor()) as f32);
            let mut top_seen = f64::NEG_INFINITY;
            let mut below: Option<[f32; 3]> = None;
            for j in 0..m {
                let angle = |i: usize| self.angle.get(i * m + j).copied().unwrap_or(f32::MIN);
                let a = f64::from(angle(i0) + (angle(i1) - angle(i0)) * f);
                let colour = |i: usize| colours.get(i * m + j).copied().unwrap_or([0, 0, 0]);
                let (c0, c1) = (colour(i0), colour(i1));
                let c = [0, 1, 2].map(|k| f32::from(c0[k]) + (f32::from(c1[k]) - f32::from(c0[k])) * f);
                if a > top_seen {
                    // the band that this ring brings into view: from the last ridge up to this angle
                    let (y_top, y_bottom) = (row_of(a), if top_seen.is_finite() { row_of(top_seen) } else { h as f64 });
                    let (first, last) = (y_top.ceil().max(0.0) as usize, (y_bottom.ceil().max(0.0) as usize).min(h));
                    let from = below.unwrap_or(c);
                    let height = (y_bottom - y_top).max(1.0) as f32;
                    for y in first..last {
                        // (a smooth change from the ring before, at the band's foot, to this one, at its top)
                        let t = (((y_bottom - y as f64) as f32) / height).clamp(0.0, 1.0);
                        let p = [0, 1, 2].map(|k| (from[k] + (c[k] - from[k]) * t).round().clamp(0.0, 255.0) as u8);
                        if let Some(px) = out.get_mut((y * w + x) * 4..(y * w + x) * 4 + 4) {
                            px.copy_from_slice(&[p[0], p[1], p[2], 255]);
                        }
                    }
                    top_seen = a;
                    below = Some(c);
                    if y_top <= 0.0 {
                        break;
                    }
                }
            }
        }
        out
    }
}

fn ambient_of(base: f64, tint: [f64; 3], k: usize) -> f64 {
    base * tint.get(k).copied().unwrap_or(1.0)
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The colour of bare ground by height above the sea (sea, fields, forest, scrub, rock, snow) and steepness (steep
/// slopes show rock).
fn ground_colour(h: f64, slope_deg: f64) -> [f64; 3] {
    if h <= 0.5 {
        return [40.0, 76.0, 108.0];
    }
    const STOPS: [(f64, [f64; 3]); 7] = [
        (0.5, [74.0, 98.0, 62.0]),
        (400.0, [62.0, 88.0, 54.0]),
        (1000.0, [92.0, 104.0, 70.0]),
        (1700.0, [128.0, 118.0, 98.0]),
        (2300.0, [142.0, 134.0, 124.0]),
        (2900.0, [226.0, 229.0, 236.0]),
        (3300.0, [246.0, 247.0, 251.0]),
    ];
    let mut c = STOPS[STOPS.len() - 1].1;
    for w in STOPS.windows(2) {
        let ((h0, c0), (h1, c1)) = (w[0], w[1]);
        if h <= h1 {
            let t = ((h - h0) / (h1 - h0)).clamp(0.0, 1.0);
            c = [0, 1, 2].map(|k| c0[k] + (c1[k] - c0[k]) * t);
            break;
        }
    }
    let rock = smooth(28.0, 48.0, slope_deg) * if h > 2800.0 { 0.35 } else { 1.0 };
    let stone = [118.0, 112.0, 106.0];
    [0, 1, 2].map(|k| c[k] + (stone[k] - c[k]) * rock)
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: Grid = Grid { azimuths: 72, rings: 40, near: 150.0, far: 60_000.0 };

    /// Heights as the grid lists them: the place at `ground`, then `f(azimuth°, distance m)` for every sample.
    fn surface(ground: f64, f: impl Fn(f64, f64) -> f64) -> Surface {
        let rings = G.ranges();
        let mut h = vec![ground];
        for i in 0..G.azimuths {
            for d in &rings {
                h.push(f(G.azimuth_of(i), *d));
            }
        }
        Surface::new(G, &h).unwrap()
    }

    fn view(az_left: f64) -> View {
        View { width: 200, height: 100, az_left, deg_x: 0.5, alt_top: 20.0, deg_y: 0.5 }
    }

    fn px(img: &[u8], v: &View, x: usize, alt: f64) -> [u8; 4] {
        let y = ((v.alt_top - alt) / v.deg_y) as usize;
        let k = (y * v.width + x) * 4;
        [img[k], img[k + 1], img[k + 2], img[k + 3]]
    }

    /// A 2 000 m ridge 10 km north (az −10°…10°) over flat ground at 100 m.
    fn ridge(az: f64, d: f64) -> f64 {
        let a = ((az + 180.0).rem_euclid(360.0)) - 180.0;
        if a.abs() < 10.0 && (9_000.0..11_000.0).contains(&d) { 2000.0 } else { 100.0 }
    }

    #[test]
    fn bad_grounds_are_refused() {
        assert!(Surface::new(G, &[0.0; 10]).is_none());
        let mut h = vec![0.0; G.points()];
        assert!(Surface::new(G, &h).is_some());
        h[7] = f64::NAN;
        assert!(Surface::new(G, &h).is_none());
        assert!(Surface::new(Grid { azimuths: 3, rings: 40, near: 1.0, far: 2.0 }, &[0.0; 121]).is_none());
    }

    #[test]
    fn a_ridge_stands_where_it_is_and_the_sky_is_clear_above_it() {
        let s = surface(100.0, ridge);
        let c = s.colours(Horizontal { az: 200.0, alt: 30.0 }, None, [170, 190, 215]);
        let v = view(-50.0); // looking from −50° to +50°: north (0°) is at x = 100
        let img = s.render(&c, &v);
        assert_eq!(img.len(), 200 * 100 * 4);
        // the ridge's top is atan(1900/9000) ≈ 11.9° (less the Earth's curve): solid at 5°, sky at 15°
        assert_eq!(px(&img, &v, 100, 5.0)[3], 255);
        assert_eq!(px(&img, &v, 100, 15.0)[3], 0);
        // beside the ridge (az 30° = x 160) the flat ground only reaches a little over the horizon's line
        assert_eq!(px(&img, &v, 160, 5.0)[3], 0);
        assert_eq!(px(&img, &v, 160, -5.0)[3], 255);
        // and the far ridge is the top of the band, not the near ground
        let top = (0..100).find(|y| img[(y * 200 + 100) * 4 + 3] == 255).unwrap();
        let alt = v.alt_top - top as f64 * v.deg_y;
        assert!((10.5..12.5).contains(&alt), "{alt}");
    }

    #[test]
    fn a_view_across_north_wraps_round() {
        let s = surface(100.0, ridge);
        let c = s.colours(Horizontal { az: 200.0, alt: 30.0 }, None, [170, 190, 215]);
        // left edge at 350°: the ridge (−10…10°) is in the middle of 350…10 and the picture wraps
        let v = View { width: 100, height: 100, az_left: 350.0, deg_x: 0.2, alt_top: 20.0, deg_y: 0.5 };
        let img = s.render(&c, &v);
        // (columns every 5°: well inside the ridge on both sides of north, and straight at it)
        for x in [20, 50, 80] {
            assert_eq!(px(&img, &v, x, 5.0)[3], 255, "x {x}");
        }
        assert_eq!(px(&img, &v, 50, 15.0)[3], 0);
    }

    #[test]
    fn a_slope_facing_the_sun_is_brighter_than_one_turned_away_and_night_is_dark() {
        // a hill rising to the east of the place: its west face looks west, a Sun in the west lights it
        let hill = |az: f64, d: f64| {
            let (x, y) = (d * az.to_radians().sin(), d * az.to_radians().cos());
            100.0 + 400.0 * (-(((x - 3000.0).powi(2) + y.powi(2)) / (1500.0f64).powi(2))).exp()
        };
        let s = surface(100.0, hill);
        let lum = |c: &[[u8; 3]], az: f64, d: f64| {
            let i = (az / 360.0 * 72.0).round() as usize % 72;
            let j = (0..40).min_by(|a, b| (G.ranges()[*a] - d).abs().total_cmp(&(G.ranges()[*b] - d).abs())).unwrap();
            let p = c[i * 40 + j];
            u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])
        };
        let west_sun = s.colours(Horizontal { az: 270.0, alt: 20.0 }, None, [0, 0, 0]);
        let east_sun = s.colours(Horizontal { az: 90.0, alt: 20.0 }, None, [0, 0, 0]);
        // the west face of the hill (east of the place at 80–90°, about 2 000 m out) is lit from the west, dark from the east
        assert!(lum(&west_sun, 88.0, 2000.0) > lum(&east_sun, 88.0, 2000.0), "{} {}", lum(&west_sun, 88.0, 2000.0), lum(&east_sun, 88.0, 2000.0));
        let night = s.colours(Horizontal { az: 270.0, alt: -30.0 }, None, [0, 0, 0]);
        assert!(lum(&night, 88.0, 2000.0) * 4 < lum(&west_sun, 88.0, 2000.0));
    }

    #[test]
    fn the_far_ground_is_paler_toward_the_sky_and_snow_is_white_and_the_sea_blue() {
        let h = |_az: f64, d: f64| if d < 20_000.0 { 100.0 } else { 3000.0 };
        let s = surface(100.0, h);
        let sky = [200, 210, 230];
        let c = s.colours(Horizontal { az: 180.0, alt: 40.0 }, None, sky);
        let rings = G.ranges();
        let j_near = 5;
        let j_far = (0..40).find(|j| rings[*j] > 40_000.0).unwrap();
        let dist = |p: [u8; 3]| (i32::from(p[0]) - 200).abs() + (i32::from(p[1]) - 210).abs() + (i32::from(p[2]) - 230).abs();
        assert!(dist(c[j_far]) < dist(c[j_near]), "far {:?} near {:?}", c[j_far], c[j_near]);
        // 3 000 m: snow, brighter than the fields
        assert!(u32::from(c[j_far][0]) > u32::from(c[j_near][0]));
        // the sea (heights of 0): blue wins over red
        let sea = surface(0.0, |_, _| 0.0).colours(Horizontal { az: 180.0, alt: 40.0 }, None, [0, 0, 0]);
        assert!(sea[3][2] > sea[3][0]);
    }

    #[test]
    fn a_photo_colours_the_ground_where_it_has_one_and_the_table_where_not() {
        let s = surface(100.0, |_, _| 100.0);
        let mut photo = vec![[200u8, 30, 30, 255]; s.len()];
        photo[3] = [0, 0, 0, 0];
        let sun = Horizontal { az: 180.0, alt: 40.0 };
        let c = s.colours(sun, Some(&photo), [0, 0, 0]);
        assert!(c[2][0] > c[2][1] * 3, "photo red: {:?}", c[2]);
        let table = s.colours(sun, None, [0, 0, 0]);
        assert_eq!(c[3], table[3], "no photo: the table's colour");
    }

    #[test]
    fn a_view_that_cannot_be_drawn_gives_nothing() {
        let s = surface(100.0, ridge);
        let c = s.colours(Horizontal { az: 200.0, alt: 30.0 }, None, [0, 0, 0]);
        let ok = view(0.0);
        assert!(s.render(&c[..10], &ok).is_empty());
        assert!(s.render(&c, &View { width: 0, ..ok }).is_empty());
        assert!(s.render(&c, &View { deg_x: 0.0, ..ok }).is_empty());
        assert!(s.render(&c, &View { az_left: f64::NAN, ..ok }).is_empty());
        assert!(s.render(&c, &View { width: 100_000, ..ok }).is_empty());
    }
}
