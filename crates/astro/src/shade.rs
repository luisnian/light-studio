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

/// How high the person stands above the ground at the point (metres), so that the grid's roughness does not shade it.
const EYE_M: f32 = 2.0;
/// Past this much height above the point the Sun's ray clears any ground on Earth.
const CLEAR_M: f32 = 9000.0;

fn ground_at(heights: &[f32], width: usize, height: usize, x: f64, y: f64) -> Option<f32> {
    let (x, y) = (x - 0.5, y - 0.5);
    if !(x.is_finite() && y.is_finite()) || x < 0.0 || y < 0.0 || x > (width - 1) as f64 || y > (height - 1) as f64 {
        return None;
    }
    let (i, j) = (x.floor() as usize, y.floor() as usize);
    let (fx, fy) = ((x - i as f64) as f32, (y - j as f64) as f32);
    let at = |i: usize, j: usize| heights.get(j.min(height - 1) * width + i.min(width - 1)).copied();
    let (a, b, c, d) = (at(i, j)?, at(i + 1, j)?, at(i, j + 1)?, at(i + 1, j + 1)?);
    Some(a * (1.0 - fx) * (1.0 - fy) + b * fx * (1.0 - fy) + c * (1.0 - fx) * fy + d * fx * fy)
}

/// Whether the Sun's direct light reaches the point (`x`, `y`, in pixels of the grid, `y` from the north) at this moment: the
/// ray towards the Sun is followed over the ground, and a ridge above it throws the point into shadow. `None` if the grid is
/// not a grid of numbers, the point is outside it or the Sun is not a direction. Beyond the grid's edge nothing is known to
/// block the ray. Only cast shadows count (a slope turned from the Sun is not looked at).
pub fn point_lit(heights: &[f32], width: usize, height: usize, m_per_px: f64, x: f64, y: f64, sun: Horizontal) -> Option<bool> {
    let n = width.checked_mul(height)?;
    if width < 3 || height < 3 || heights.len() != n || !(m_per_px.is_finite() && m_per_px > 0.0) || !(sun.alt.is_finite() && sun.az.is_finite()) {
        return None;
    }
    let h0 = ground_at(heights, width, height, x, y)? + EYE_M;
    if sun.alt <= 0.0 {
        return Some(false);
    }
    let rise_per_px = (sun.alt.to_radians().tan() * m_per_px) as f32;
    let (dx, dy) = (sun.az.to_radians().sin(), -sun.az.to_radians().cos());
    let mut k = 1.0_f64;
    loop {
        let ray = h0 + rise_per_px * k as f32;
        if ray > h0 + CLEAR_M {
            return Some(true);
        }
        match ground_at(heights, width, height, x + dx * k, y + dy * k) {
            None => return Some(true),
            Some(g) if g > ray => return Some(false),
            Some(_) => {}
        }
        k += 1.0;
    }
}

/// The stretches of `t0..t1` in which the Sun's direct light reaches the point (see [`point_lit`]); `sun_at` gives the Sun's
/// direction at a moment. `step` seconds apart are looked at, and each edge is found to the second.
#[allow(clippy::too_many_arguments)]
pub fn lit_spans(
    heights: &[f32],
    width: usize,
    height: usize,
    m_per_px: f64,
    x: f64,
    y: f64,
    sun_at: &dyn Fn(f64) -> Horizontal,
    t0: f64,
    t1: f64,
    step: f64,
) -> Vec<crate::plan::Span> {
    if point_lit(heights, width, height, m_per_px, x, y, Horizontal { az: 180.0, alt: 45.0 }).is_none() {
        return Vec::new();
    }
    let lit = |t: f64| point_lit(heights, width, height, m_per_px, x, y, sun_at(t)).unwrap_or(false);
    crate::plan::spans(&lit, t0, t1, step.max(1.0))
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

    fn ridge_grid(ridge_m: f32) -> (Vec<f32>, usize, usize) {
        // flat ground at 100 m, with a wall of `ridge_m` across columns 30..32 (to the west of the point)
        let (w, h) = (80, 40);
        let mut v = vec![100.0; w * h];
        for j in 0..h {
            for i in 30..32 {
                v[j * w + i] = ridge_m;
            }
        }
        (v, w, h)
    }

    #[test]
    fn a_wall_to_the_west_shades_the_point_in_the_evening_only() {
        let (v, w, h) = ridge_grid(400.0);
        // the point is 20 px (1 km at 50 m a pixel) east of the wall: the wall is 300 m above it
        let (x, y) = (52.0, 20.0);
        // sun in the east, low: lit. In the west at 10°: the wall (300 m at 1 km, 17°) hides it. In the west at 40°: clears it.
        assert_eq!(point_lit(&v, w, h, 50.0, x, y, Horizontal { az: 90.0, alt: 10.0 }), Some(true));
        assert_eq!(point_lit(&v, w, h, 50.0, x, y, Horizontal { az: 270.0, alt: 10.0 }), Some(false));
        assert_eq!(point_lit(&v, w, h, 50.0, x, y, Horizontal { az: 270.0, alt: 40.0 }), Some(true));
        assert_eq!(point_lit(&v, w, h, 50.0, x, y, Horizontal { az: 270.0, alt: -3.0 }), Some(false));
    }

    #[test]
    fn the_hours_of_sun_at_a_point_are_the_ones_the_ridge_leaves() {
        let (v, w, h) = ridge_grid(400.0);
        // a Sun that goes from east to west in 10 hours, 5° to 45° to 5°
        let sun = |t: f64| {
            let f = (t / 36000.0).clamp(0.0, 1.0);
            Horizontal { az: 90.0 + 180.0 * f, alt: 45.0 * (std::f64::consts::PI * f).sin() }
        };
        let spans = lit_spans(&v, w, h, 50.0, 52.0, 20.0, &sun, 0.0, 36000.0, 300.0);
        let lit: f64 = spans.iter().map(|s| s.end - s.start).sum();
        assert!(spans.len() == 1 && lit > 3.0 * 3600.0 && lit < 9.5 * 3600.0, "{spans:?} {lit}");
        assert!(spans[0].end < 36000.0 - 600.0, "the wall ends the day early");
    }

    #[test]
    fn a_point_off_the_grid_or_a_broken_grid_is_not_lit_nor_shaded() {
        let (v, w, h) = ridge_grid(400.0);
        let sun = Horizontal { az: 90.0, alt: 30.0 };
        assert_eq!(point_lit(&v, w, h, 50.0, -5.0, 20.0, sun), None);
        assert_eq!(point_lit(&v, w, h, 50.0, f64::NAN, 20.0, sun), None);
        assert_eq!(point_lit(&v[..10], w, h, 50.0, 52.0, 20.0, sun), None);
        assert_eq!(point_lit(&v, w, h, 0.0, 52.0, 20.0, sun), None);
        assert!(lit_spans(&v, w, h, 50.0, 500.0, 500.0, &|_| sun, 0.0, 1000.0, 60.0).is_empty());
    }
}
