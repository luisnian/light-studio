//! The mountains around a place: the skyline seen from it all the way round, from ground heights sampled on rings
//! around the place (the Planner's «Mountains» strip), and the peaks that stand out on it.

use crate::align::{EARTH_R, EYE, REFRACTION_K, destination};

/// How many directions the skyline has (every 3°) and how many rings of ground heights each one reads.
pub const AZIMUTHS: usize = 120;
pub const RANGES: usize = 15;
/// The nearest and farthest ring (metres).
pub const NEAR: f64 = 500.0;
pub const FAR: f64 = 60_000.0;
/// Heights to ask for: the place itself, then the rings of each direction in turn.
pub const POINTS: usize = 1 + AZIMUTHS * RANGES;

/// How finely a skyline is sampled: directions all round and rings of distance. The coarse grid is what a web
/// elevation service can be asked for; the fine one needs the terrain tiles read on this computer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub azimuths: usize,
    pub rings: usize,
    pub near: f64,
    pub far: f64,
}

impl Grid {
    /// 3° between directions, 15 rings from 500 m to 60 km.
    pub const COARSE: Grid = Grid { azimuths: AZIMUTHS, rings: RANGES, near: NEAR, far: FAR };
    /// A quarter of a degree between directions (1 440), 120 rings from 150 m to 60 km (under 4 % apart).
    pub const FINE: Grid = Grid { azimuths: 1440, rings: 120, near: 150.0, far: FAR };

    /// Heights to ask for: the place itself, then the rings of each direction in turn.
    pub fn points(self) -> usize {
        1 + self.azimuths * self.rings
    }

    /// The distance of each ring from the place, closer together near it (metres).
    pub fn ranges(self) -> Vec<f64> {
        let n = self.rings.max(2);
        (0..self.rings).map(|j| self.near * (self.far / self.near).powf(j as f64 / (n - 1) as f64)).collect()
    }

    /// The degrees from north of direction `i`.
    pub fn azimuth_of(self, i: usize) -> f64 {
        360.0 * i as f64 / self.azimuths.max(1) as f64
    }

    /// Where the ground heights are wanted: the place `(lat, lon)` first, then, direction by direction, each ring.
    pub fn sample_points(self, lat: f64, lon: f64) -> Vec<(f64, f64)> {
        let rings = self.ranges();
        let mut out = Vec::with_capacity(self.points());
        out.push((lat, lon));
        for i in 0..self.azimuths {
            let az = self.azimuth_of(i);
            for d in &rings {
                out.push(destination(lat, lon, az, *d));
            }
        }
        out
    }
}

/// The distance of each ring from the place, closer together near it (metres).
pub fn ranges() -> [f64; RANGES] {
    let mut out = [0.0; RANGES];
    for (j, r) in out.iter_mut().enumerate() {
        *r = NEAR * (FAR / NEAR).powf(j as f64 / (RANGES - 1) as f64);
    }
    out
}

/// The degrees from north of direction `i` of the coarse grid.
pub fn azimuth_of(i: usize) -> f64 {
    Grid::COARSE.azimuth_of(i)
}

/// Where the ground heights are wanted: the place `(lat, lon)` first, then, direction by direction, each ring.
pub fn sample_points(lat: f64, lon: f64) -> Vec<(f64, f64)> {
    Grid::COARSE.sample_points(lat, lon)
}

/// The skyline all the way round.
#[derive(Clone, Debug, PartialEq)]
pub struct Skyline {
    /// The nearest and farthest ring (metres), for colouring by distance.
    pub near: f64,
    pub far: f64,
    /// The ground under the camera (metres).
    pub ground: f64,
    /// Per direction: how high the highest ground is seen (degrees above the camera's level, with the Earth's curve and
    /// the usual refraction), how far away it is and how high it is above the sea (metres).
    pub alt: Vec<f64>,
    pub dist: Vec<f64>,
    pub height: Vec<f64>,
}

/// A summit that stands out on the skyline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    pub az: f64,
    pub alt: f64,
    pub dist: f64,
    pub height: f64,
}

/// The skyline from `heights` as `sample_points` listed them (None if there are not as many, or one is not a number).
pub fn skyline(heights: &[f64]) -> Option<Skyline> {
    skyline_for(Grid::COARSE, heights)
}

/// As [`skyline`], for heights listed as `grid.sample_points` does.
pub fn skyline_for(grid: Grid, heights: &[f64]) -> Option<Skyline> {
    if grid.azimuths == 0 || grid.rings < 2 || heights.len() != grid.points() || heights.iter().any(|h| !h.is_finite()) {
        return None;
    }
    let ground = *heights.first()?;
    let eye = ground + EYE;
    let rings = grid.ranges();
    let (mut alt, mut dist, mut height) = (Vec::with_capacity(grid.azimuths), Vec::with_capacity(grid.azimuths), Vec::with_capacity(grid.azimuths));
    for i in 0..grid.azimuths {
        let mut best = (f64::MIN, 0.0, 0.0);
        for (j, d) in rings.iter().enumerate() {
            let h = *heights.get(1 + i * grid.rings + j)?;
            let drop = d * d * (1.0 - REFRACTION_K) / (2.0 * EARTH_R);
            let a = ((h - eye - drop) / d).atan().to_degrees();
            if a > best.0 {
                best = (a, *d, h);
            }
        }
        alt.push(best.0);
        dist.push(best.1);
        height.push(best.2);
    }
    Some(Skyline { near: grid.near, far: grid.far, ground, alt, dist, height })
}

impl Skyline {
    /// The skyline's height (degrees) in direction `az`, between the two nearest samples.
    pub fn altitude_at(&self, az: f64) -> f64 {
        let n = self.alt.len();
        if n == 0 || !az.is_finite() {
            return 0.0;
        }
        let x = az.rem_euclid(360.0) / 360.0 * n as f64;
        let i = x.floor() as usize % n;
        let f = x - x.floor();
        let (a, b) = (self.alt.get(i).copied().unwrap_or(0.0), self.alt.get((i + 1) % n).copied().unwrap_or(0.0));
        a + (b - a) * f
    }

    /// How far the skyline's ground is (metres) in direction `az`, between the two nearest samples (in the logarithm,
    /// as the rings are spaced).
    pub fn distance_at(&self, az: f64) -> f64 {
        let n = self.dist.len();
        if n == 0 || !az.is_finite() {
            return 0.0;
        }
        let x = az.rem_euclid(360.0) / 360.0 * n as f64;
        let i = x.floor() as usize % n;
        let f = x - x.floor();
        let (a, b) = (self.dist.get(i).copied().unwrap_or(NEAR).max(1.0), self.dist.get((i + 1) % n).copied().unwrap_or(NEAR).max(1.0));
        (a.ln() + (b.ln() - a.ln()) * f).exp()
    }

    /// The index of the sample nearest `az`.
    pub fn index_at(&self, az: f64) -> usize {
        let n = self.alt.len().max(1);
        ((az.rem_euclid(360.0) / 360.0 * n as f64).round() as usize) % n
    }

    /// The highest point of the skyline (degrees).
    pub fn highest(&self) -> f64 {
        self.alt.iter().copied().fold(f64::MIN, f64::max)
    }

    /// The summits that rise at least `rise` degrees above the lower ground on both sides within about 25°, highest first.
    pub fn peaks(&self, rise: f64) -> Vec<Peak> {
        let n = self.alt.len();
        if n == 0 {
            return Vec::new();
        }
        let at = |i: isize| self.alt.get(i.rem_euclid(n as isize) as usize).copied().unwrap_or(f64::MIN);
        // the neighbourhoods are in degrees (12° and 24°), whatever the sampling
        let per_degree = n as f64 / 360.0;
        let (near, wide) = ((12.0 * per_degree).round().max(1.0) as isize, (24.0 * per_degree).round().max(2.0) as isize);
        let mut out = Vec::new();
        for i in 0..n as isize {
            let a = at(i);
            if a <= at(i - 1) || a < at(i + 1) {
                continue;
            }
            // the highest point of its neighbourhood
            if (-near..=near).any(|k| k != 0 && at(i + k) > a) {
                continue;
            }
            let side = |dir: isize| (1..=wide).map(|k| at(i + dir * k)).fold(f64::MAX, f64::min);
            if a - side(-1).max(side(1)) < rise {
                continue;
            }
            let u = i as usize;
            out.push(Peak {
                az: 360.0 * u as f64 / n as f64,
                alt: a,
                dist: self.dist.get(u).copied().unwrap_or(0.0),
                height: self.height.get(u).copied().unwrap_or(0.0),
            });
        }
        out.sort_by(|a, b| b.alt.total_cmp(&a.alt));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(h: f64) -> Vec<f64> {
        vec![h; POINTS]
    }

    #[test]
    fn the_points_are_the_place_then_the_rings_of_each_direction() {
        let pts = sample_points(43.3, -1.98);
        assert_eq!(pts.len(), POINTS);
        assert_eq!(pts[0], (43.3, -1.98));
        // direction 0 is north: the rings go up the map
        assert!(pts[1].0 > 43.3 && (pts[1].1 + 1.98).abs() < 1e-6);
        // direction 30 (of 120) is east: they go along it
        let east = pts[1 + 30 * RANGES];
        assert!(east.1 > -1.98 && (east.0 - 43.3).abs() < 0.01, "{east:?}");
        let r = ranges();
        assert!((r[0] - NEAR).abs() < 1e-6 && (r[RANGES - 1] - FAR).abs() < 1e-3);
    }

    #[test]
    fn level_ground_seen_from_the_ground_is_a_hair_under_the_horizon() {
        let s = skyline(&flat(0.0)).unwrap();
        assert_eq!(s.alt.len(), AZIMUTHS);
        assert!(s.alt.iter().all(|a| (-0.2..0.0).contains(a)), "{:?}", &s.alt[..3]);
    }

    #[test]
    fn a_mountain_is_seen_at_its_angle_and_named_with_its_height_and_distance() {
        let mut h = flat(0.0);
        // east, ring 9 (about 11 km): 1 400 m
        h[1 + 30 * RANGES + 9] = 1400.0;
        let s = skyline(&h).unwrap();
        let d = ranges()[9];
        let drop = d * d * (1.0 - REFRACTION_K) / (2.0 * EARTH_R);
        let want = ((1400.0 - EYE - drop) / d).atan().to_degrees();
        assert!((s.alt[30] - want).abs() < 1e-9 && want > 6.0, "{} vs {want}", s.alt[30]);
        assert_eq!((s.dist[30], s.height[30]), (d, 1400.0));
        // between samples the skyline slopes down to its neighbours
        assert!(s.altitude_at(90.0) > s.altitude_at(91.5) && s.altitude_at(91.5) > s.altitude_at(93.0));
        let peaks = s.peaks(1.0);
        assert_eq!(peaks.len(), 1);
        assert_eq!((peaks[0].az, peaks[0].height), (90.0, 1400.0));
        assert!((s.highest() - want).abs() < 1e-9);
    }

    #[test]
    fn a_hill_near_hides_the_mountain_behind_when_it_is_taller_in_the_sky() {
        let mut h = flat(0.0);
        h[1 + 10 * RANGES + 2] = 300.0; // 100 m… near ring: steep angle
        h[1 + 10 * RANGES + 10] = 500.0; // far and low in the sky
        let s = skyline(&h).unwrap();
        assert_eq!(s.height[10], 300.0, "the near hill fills more of the sky");
    }

    #[test]
    fn a_broad_swell_is_not_a_peak_and_wrapping_works_across_north() {
        let mut h = flat(0.0);
        for i in 0..AZIMUTHS {
            // a gentle rise across the whole round: no summit stands out by a degree
            h[1 + i * RANGES + 9] = 100.0 + (i as f64 / 6.0).sin() * 20.0;
        }
        assert!(skyline(&h).unwrap().peaks(1.0).is_empty());
        let mut h = flat(0.0);
        h[1 + RANGES + 9] = 1500.0; // direction 1
        h[1 + (AZIMUTHS - 1) * RANGES + 9] = 900.0; // direction 119, across north
        let p = skyline(&h).unwrap().peaks(1.0);
        assert_eq!(p.first().map(|p| p.az), Some(3.0));
    }

    #[test]
    fn a_fine_grid_sees_the_same_mountain_with_a_quarter_of_a_degree_between_directions() {
        let g = Grid::FINE;
        assert_eq!(g.points(), 1 + 1440 * 120);
        let r = g.ranges();
        assert!((r[0] - 150.0).abs() < 1e-9 && (r[119] - FAR).abs() < 1e-3);
        let pts = g.sample_points(43.3, -1.98);
        assert_eq!(pts.len(), g.points());
        let mut h = vec![0.0; g.points()];
        // east is direction 360 of 1 440; ring 80 is about 11 km
        h[1 + 360 * g.rings + 80] = 1400.0;
        let s = skyline_for(g, &h).unwrap();
        assert_eq!(s.alt.len(), 1440);
        let peaks = s.peaks(1.0);
        assert_eq!(peaks.len(), 1);
        assert_eq!((peaks[0].az, peaks[0].height), (90.0, 1400.0));
        assert!(s.altitude_at(90.0) > s.altitude_at(90.125) && s.altitude_at(90.125) > s.altitude_at(90.25), "slopes down over one sample step");
        assert!(skyline_for(g, &h[..10]).is_none());
        assert!(skyline_for(Grid { azimuths: 0, ..g }, &[]).is_none());
    }

    #[test]
    fn bad_heights_are_refused() {
        assert!(skyline(&[0.0; 3]).is_none());
        let mut h = flat(0.0);
        h[5] = f64::NAN;
        assert!(skyline(&h).is_none());
    }
}
