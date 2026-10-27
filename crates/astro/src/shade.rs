//! Terrain shadows on a height map: which ground the Sun does not reach at a given moment (a ridge in the way) or
//! reaches only at a glancing angle (a slope turned away), for the Planner's map.
//!
//! The ground is a grid of heights (metres) in north-up Web Mercator pixels, as the terrain tiles come. The Sun's rays
//! run across it in straight lines; walking along each line from the Sun's side, a "ceiling" (the height of the lowest
//! ray still in the air) drops by `tan(height of the Sun)` per metre travelled, and rises to the ground wherever the
//! ground is higher. Ground under the ceiling is in the shadow. One pass over the grid, whatever its size.

use crate::sun::Horizontal;

/// How far under the ceiling (metres) the ground has to be to be fully dark: the shadow's edge is a soft step this
/// deep, which also keeps it from stair-stepping.
const SOFT_M: f32 = 6.0;
/// The darkest a slope turned away from the Sun is (a share of full dark).
const SLOPE_DARK: f32 = 0.8;

/// How dark each point is (0 lit … 1 in a cast shadow, or a slope turned away), row by row from the north.
/// `heights` is `width × height` metres; `m_per_px` the ground's size of one pixel. None if the grid is not a grid of
/// numbers or the Sun is not a direction. With the Sun at or under the horizon everything is 1.
pub fn shade_dem(heights: &[f32], width: usize, height: usize, m_per_px: f64, sun: Horizontal) -> Option<Vec<f32>> {
    let n = width.checked_mul(height)?;
    if width < 3 || height < 3 || heights.len() != n || !(m_per_px.is_finite() && m_per_px > 0.0) || !(sun.alt.is_finite() && sun.az.is_finite()) {
        return None;
    }
    if heights.iter().any(|h| !h.is_finite()) {
        return None;
    }
    if sun.alt <= 0.0 {
        return Some(vec![1.0; n]);
    }
    let (az, alt) = (sun.az.to_radians(), sun.alt.to_radians());
    // the light's way across the grid (x east, y south): away from the Sun
    let (ux, uy) = (-az.sin(), az.cos());
    let transposed = uy.abs() > ux.abs();
    // the "major" axis is the one the light moves most along: one step of it per pixel; the other moves `slope` per step
    let (major, minor) = if transposed { (height, width) } else { (width, height) };
    let (dir, slope) = if transposed { (uy.signum(), ux / uy.abs()) } else { (ux.signum(), uy / ux.abs()) };
    let step_m = (m_per_px / if transposed { uy.abs() } else { ux.abs() }) as f32;
    let drop = step_m * alt.tan() as f32;
    // the ground at major position `a` and minor position `b` (between rows or columns: linear)
    let at = |a: usize, b0: usize| -> f32 {
        let idx = if transposed { a * width + b0 } else { b0 * width + a };
        heights.get(idx).copied().unwrap_or(0.0)
    };
    let sample = |a: usize, b: f64| -> f32 {
        let b0 = b.floor();
        let f = (b - b0) as f32;
        let (i0, i1) = (b0 as usize, (b0 as usize + 1).min(minor - 1));
        at(a, i0) * (1.0 - f) + at(a, i1) * f
    };
    let mut cast = vec![0.0_f32; n];
    // the lines start (integer minor positions) wherever they will cross the grid
    let span = (major as f64 * slope.abs()).ceil();
    let (first, last) = if slope >= 0.0 { (-span, minor as f64) } else { (0.0, minor as f64 + span) };
    let mut line = first.floor();
    while line <= last {
        let mut ceiling = f32::NEG_INFINITY;
        for k in 0..major {
            let b = line + k as f64 * slope;
            let a = if dir > 0.0 { k } else { major - 1 - k };
            if b < 0.0 || b > (minor - 1) as f64 {
                // off the grid: the ceiling there is not known
                ceiling = f32::NEG_INFINITY;
                continue;
            }
            let h = sample(a, b);
            ceiling = if ceiling.is_finite() { ceiling - drop } else { h };
            let depth = ceiling - h;
            let dark = if depth > 0.0 {
                let t = (depth / SOFT_M).min(1.0);
                t * t * (3.0 - 2.0 * t)
            } else {
                ceiling = h;
                0.0
            };
            // the pixel the line crosses at this step
            let row = (b + 0.5).floor() as usize;
            let idx = if transposed { a * width + row.min(minor - 1) } else { row.min(minor - 1) * width + a };
            if let Some(c) = cast.get_mut(idx) {
                *c = dark;
            }
        }
        line += 1.0;
    }
    // a slope turned away: darker than level ground at the same Sun height would be
    let sun_vec = (alt.cos() * az.sin(), alt.cos() * az.cos(), alt.sin());
    let level = sun_vec.2.max(0.05);
    let m = m_per_px;
    let g = |x: usize, y: usize| f64::from(heights.get(y.min(height - 1) * width + x.min(width - 1)).copied().unwrap_or(0.0));
    for y in 0..height {
        for x in 0..width {
            let dzdx = (g(x + 1, y) - g(x.saturating_sub(1), y)) / (2.0 * m);
            let dzdn = (g(x, y.saturating_sub(1)) - g(x, y + 1)) / (2.0 * m);
            let norm = (dzdx * dzdx + dzdn * dzdn + 1.0).sqrt();
            let lambert = ((-dzdx * sun_vec.0 - dzdn * sun_vec.1 + sun_vec.2) / norm).max(0.0);
            let slope_dark = ((1.0 - lambert / level).clamp(0.0, 1.0) as f32) * SLOPE_DARK;
            if let Some(c) = cast.get_mut(y * width + x) {
                *c += (1.0 - *c) * slope_dark;
            }
        }
    }
    Some(cast)
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 120;
    const H: usize = 90;

    fn sun(az: f64, alt: f64) -> Horizontal {
        Horizontal { az, alt }
    }

    /// Level ground at 100 m with a wall 600 m high along the column `col`.
    fn wall(col: usize) -> Vec<f32> {
        (0..W * H).map(|n| if n % W == col { 700.0 } else { 100.0 }).collect()
    }

    fn at(v: &[f32], x: usize, y: usize) -> f32 {
        v[y * W + x]
    }

    #[test]
    fn level_ground_is_lit_and_night_is_dark_and_bad_grids_are_refused() {
        let flat = vec![250.0; W * H];
        assert!(shade_dem(&flat, W, H, 50.0, sun(200.0, 30.0)).unwrap().iter().all(|d| *d == 0.0));
        assert!(shade_dem(&flat, W, H, 50.0, sun(200.0, -3.0)).unwrap().iter().all(|d| *d == 1.0));
        assert!(shade_dem(&flat[..10], W, H, 50.0, sun(200.0, 30.0)).is_none());
        assert!(shade_dem(&flat, W, H, 0.0, sun(200.0, 30.0)).is_none());
        assert!(shade_dem(&flat, W, H, 50.0, sun(f64::NAN, 30.0)).is_none());
        let mut bad = flat.clone();
        bad[5] = f32::NAN;
        assert!(shade_dem(&bad, W, H, 50.0, sun(200.0, 30.0)).is_none());
    }

    #[test]
    fn a_wall_throws_its_shadow_away_from_the_sun_as_far_as_its_height_says() {
        let col = 30;
        // 50 m pixels; the Sun in the west at 10°: 600 m of wall shades 600 / tan 10° = 3.4 km east of it = 68 pixels
        let s = shade_dem(&wall(col), W, H, 50.0, sun(270.0, 10.0)).unwrap();
        let y = 45;
        assert!(at(&s, 10, y) < 0.01, "in front of the wall (the Sun side) is lit");
        assert!(at(&s, col + 10, y) > 0.99 && at(&s, col + 60, y) > 0.99, "behind the wall, in its shadow");
        assert!(at(&s, col + 75, y) < 0.01, "the shadow ends about 3.4 km on: {}", at(&s, col + 75, y));
        // the Sun in the east at the same height: the shadow falls the other way
        let s = shade_dem(&wall(col), W, H, 50.0, sun(90.0, 10.0)).unwrap();
        assert!(at(&s, col - 10, y) > 0.99);
        assert!(at(&s, col + 10, y) < 0.01);
        // a higher Sun shortens it
        let s = shade_dem(&wall(col), W, H, 50.0, sun(270.0, 45.0)).unwrap();
        assert!(at(&s, col + 5, y) > 0.99 && at(&s, col + 20, y) < 0.01);
    }

    #[test]
    fn a_wall_seen_from_the_north_or_a_diagonal_casts_the_same_way() {
        // a wall along the row `row` (east-west), the Sun in the north (az 0) at 10°: the shadow falls south
        let row = 20;
        let h: Vec<f32> = (0..W * H).map(|n| if n / W == row { 700.0 } else { 100.0 }).collect();
        let s = shade_dem(&h, W, H, 50.0, sun(0.0, 10.0)).unwrap();
        let x = 60;
        assert!(at(&s, x, row - 5) < 0.01 && at(&s, x, row + 10) > 0.99, "{} {}", at(&s, x, row - 5), at(&s, x, row + 10));
        // the Sun in the south-west: the shadow of the column wall falls to the north-east
        let s = shade_dem(&wall(30), W, H, 50.0, sun(225.0, 15.0)).unwrap();
        assert!(at(&s, 22, 60) < 0.01 && at(&s, 36, 55) > 0.99, "{} {}", at(&s, 22, 60), at(&s, 36, 55));
        // every pixel got an answer between 0 and 1 for every direction
        for az in (0..360).step_by(15) {
            let s = shade_dem(&wall(30), W, H, 50.0, sun(f64::from(az), 20.0)).unwrap();
            assert_eq!(s.len(), W * H);
            assert!(s.iter().all(|d| (0.0..=1.0).contains(d)), "az {az}");
        }
    }

    #[test]
    fn the_shadows_edge_is_soft_not_a_step() {
        // a wall 150 m high, the Sun at 3° in the west: the shadow is 150 / tan 3° = 2.9 km = 57 pixels long
        let h: Vec<f32> = (0..W * H).map(|n| if n % W == 30 { 250.0 } else { 100.0 }).collect();
        let s = shade_dem(&h, W, H, 50.0, sun(270.0, 3.0)).unwrap();
        let y = 45;
        assert!(at(&s, 60, y) > 0.99, "{}", at(&s, 60, y));
        assert!(at(&s, 100, y) < 0.01, "{}", at(&s, 100, y));
        let ramp: Vec<f32> = (70..100).map(|x| at(&s, x, y)).collect();
        assert!(ramp.iter().any(|d| *d > 0.05 && *d < 0.95), "{ramp:?}");
    }

    #[test]
    fn a_slope_turned_from_the_sun_is_darker_without_being_in_a_cast_shadow() {
        // ground rising to the east at 20 %: its face looks west, away from a Sun in the east
        let ramp: Vec<f32> = (0..W * H).map(|n| (n % W) as f32 * 50.0 * 0.2).collect();
        let away = shade_dem(&ramp, W, H, 50.0, sun(90.0, 35.0)).unwrap();
        let towards = shade_dem(&ramp, W, H, 50.0, sun(270.0, 35.0)).unwrap();
        assert!(at(&away, 60, 45) > 0.2, "{}", at(&away, 60, 45));
        assert!(at(&towards, 60, 45) < 0.01);
    }
}
