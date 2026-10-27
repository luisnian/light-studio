//! The icons, drawn with strokes and shapes in a unit square, so they stay sharp at any size and take any colour.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Bulb,
    Calendar,
    Check,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    Clock,
    Close,
    Cloud,
    Crop,
    Eye,
    EyeOff,
    Fit,
    Gear,
    Grab,
    Info,
    Linear,
    Locate,
    Map,
    Minus,
    Mountain,
    Pencil,
    Pin,
    Planner,
    Plus,
    ResizeHorizontal,
    Satellite,
    Search,
    Shade,
    Sidebar,
    Sliders,
    Sun,
    Target,
}

/// Draws `icon` inside `r` in `color`.
pub fn paint(p: &Painter, r: Rect, icon: Icon, color: Color32) {
    let side = r.width().min(r.height());
    let r = Rect::from_center_size(r.center(), vec2(side, side));
    let stroke = Stroke::new((side / 10.0).clamp(1.0, 2.4), color);
    let at = |x: f32, y: f32| -> Pos2 { pos2(r.left() + x * r.width(), r.top() + y * r.height()) };
    let len = |v: f32| v * side;
    let line = |pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| at(x, y)).collect();
        p.add(Shape::line(v, stroke));
    };
    let closed = |pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| at(x, y)).collect();
        p.add(Shape::closed_line(v, stroke));
    };
    let circle = |x: f32, y: f32, rad: f32| {
        p.circle_stroke(at(x, y), len(rad), stroke);
    };
    let dot = |x: f32, y: f32, rad: f32| {
        p.circle_filled(at(x, y), len(rad), color);
    };
    match icon {
        Icon::Plus => {
            line(&[(0.15, 0.5), (0.85, 0.5)]);
            line(&[(0.5, 0.15), (0.5, 0.85)]);
        }
        Icon::Minus => line(&[(0.15, 0.5), (0.85, 0.5)]),
        Icon::Close => {
            line(&[(0.22, 0.22), (0.78, 0.78)]);
            line(&[(0.78, 0.22), (0.22, 0.78)]);
        }
        Icon::Check => line(&[(0.18, 0.52), (0.42, 0.74), (0.84, 0.26)]),
        Icon::ChevronDown => line(&[(0.2, 0.36), (0.5, 0.66), (0.8, 0.36)]),
        Icon::ChevronLeft => line(&[(0.64, 0.2), (0.34, 0.5), (0.64, 0.8)]),
        Icon::ChevronRight => line(&[(0.36, 0.2), (0.66, 0.5), (0.36, 0.8)]),
        Icon::Clock => {
            circle(0.5, 0.5, 0.4);
            line(&[(0.5, 0.26), (0.5, 0.5), (0.68, 0.6)]);
        }
        Icon::Calendar => {
            closed(&[(0.14, 0.2), (0.86, 0.2), (0.86, 0.86), (0.14, 0.86)]);
            line(&[(0.14, 0.4), (0.86, 0.4)]);
            line(&[(0.34, 0.1), (0.34, 0.28)]);
            line(&[(0.66, 0.1), (0.66, 0.28)]);
        }
        Icon::Gear => {
            let mut pts = Vec::new();
            for k in 0..16 {
                let a = k as f32 * std::f32::consts::PI / 8.0;
                let (sn, cs) = a.sin_cos();
                let rad = if (k / 2) % 2 == 0 { 0.43 } else { 0.32 };
                pts.push((0.5 + cs * rad, 0.5 + sn * rad));
            }
            closed(&pts);
            circle(0.5, 0.5, 0.12);
        }
        Icon::Sun => {
            circle(0.5, 0.5, 0.18);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let (s, c) = a.sin_cos();
                line(&[(0.5 + c * 0.3, 0.5 + s * 0.3), (0.5 + c * 0.42, 0.5 + s * 0.42)]);
            }
        }
        Icon::Bulb => {
            circle(0.5, 0.4, 0.26);
            line(&[(0.4, 0.74), (0.6, 0.74)]);
            line(&[(0.43, 0.86), (0.57, 0.86)]);
        }
        Icon::Cloud => {
            line(&[(0.26, 0.74), (0.72, 0.74)]);
            line(&[
                (0.26, 0.74),
                (0.14, 0.62),
                (0.2, 0.46),
                (0.36, 0.44),
                (0.42, 0.3),
                (0.58, 0.26),
                (0.7, 0.38),
                (0.84, 0.48),
                (0.88, 0.62),
                (0.72, 0.74),
            ]);
        }
        Icon::Crop => {
            line(&[(0.3, 0.1), (0.3, 0.7), (0.9, 0.7)]);
            line(&[(0.1, 0.3), (0.7, 0.3), (0.7, 0.9)]);
        }
        Icon::Eye => {
            line(&[(0.08, 0.5), (0.25, 0.32), (0.5, 0.24), (0.75, 0.32), (0.92, 0.5), (0.75, 0.68), (0.5, 0.76), (0.25, 0.68), (0.08, 0.5)]);
            dot(0.5, 0.5, 0.11);
        }
        Icon::EyeOff => {
            line(&[(0.08, 0.5), (0.25, 0.32), (0.5, 0.24), (0.75, 0.32), (0.92, 0.5), (0.75, 0.68), (0.5, 0.76), (0.25, 0.68), (0.08, 0.5)]);
            line(&[(0.18, 0.86), (0.82, 0.14)]);
        }
        Icon::Fit => {
            line(&[(0.14, 0.38), (0.14, 0.14), (0.38, 0.14)]);
            line(&[(0.62, 0.14), (0.86, 0.14), (0.86, 0.38)]);
            line(&[(0.86, 0.62), (0.86, 0.86), (0.62, 0.86)]);
            line(&[(0.38, 0.86), (0.14, 0.86), (0.14, 0.62)]);
        }
        Icon::Grab => {
            for x in [0.34, 0.66] {
                for y in [0.24, 0.5, 0.76] {
                    dot(x, y, 0.065);
                }
            }
        }
        Icon::Info => {
            circle(0.5, 0.5, 0.4);
            dot(0.5, 0.31, 0.055);
            line(&[(0.5, 0.45), (0.5, 0.7)]);
        }
        Icon::Linear => {
            line(&[(0.14, 0.82), (0.86, 0.18)]);
            dot(0.14, 0.82, 0.08);
            dot(0.86, 0.18, 0.08);
        }
        Icon::Locate => {
            circle(0.5, 0.5, 0.26);
            dot(0.5, 0.5, 0.08);
            line(&[(0.5, 0.06), (0.5, 0.2)]);
            line(&[(0.5, 0.8), (0.5, 0.94)]);
            line(&[(0.06, 0.5), (0.2, 0.5)]);
            line(&[(0.8, 0.5), (0.94, 0.5)]);
        }
        Icon::Map => {
            closed(&[(0.1, 0.22), (0.37, 0.14), (0.63, 0.24), (0.9, 0.16), (0.9, 0.78), (0.63, 0.86), (0.37, 0.76), (0.1, 0.84)]);
            line(&[(0.37, 0.14), (0.37, 0.76)]);
            line(&[(0.63, 0.24), (0.63, 0.86)]);
        }
        Icon::Mountain => {
            line(&[(0.06, 0.8), (0.36, 0.26), (0.54, 0.56), (0.66, 0.4), (0.94, 0.8), (0.06, 0.8)]);
        }
        Icon::Pencil => {
            closed(&[(0.16, 0.84), (0.2, 0.62), (0.66, 0.16), (0.84, 0.34), (0.38, 0.8)]);
            line(&[(0.56, 0.26), (0.74, 0.44)]);
        }
        Icon::Pin => {
            circle(0.5, 0.36, 0.22);
            line(&[(0.34, 0.52), (0.5, 0.9), (0.66, 0.52)]);
            dot(0.5, 0.36, 0.07);
        }
        Icon::Planner => {
            circle(0.5, 0.5, 0.38);
            line(&[(0.5, 0.5), (0.5, 0.22)]);
            line(&[(0.5, 0.5), (0.7, 0.64)]);
            dot(0.5, 0.5, 0.05);
        }
        Icon::ResizeHorizontal => {
            line(&[(0.1, 0.5), (0.9, 0.5)]);
            line(&[(0.26, 0.34), (0.1, 0.5), (0.26, 0.66)]);
            line(&[(0.74, 0.34), (0.9, 0.5), (0.74, 0.66)]);
        }
        Icon::Satellite => {
            circle(0.5, 0.5, 0.18);
            closed(&[(0.1, 0.22), (0.26, 0.38), (0.18, 0.46), (0.02, 0.3)]);
            closed(&[(0.9, 0.78), (0.74, 0.62), (0.82, 0.54), (0.98, 0.7)]);
            line(&[(0.36, 0.36), (0.26, 0.26)]);
            line(&[(0.64, 0.64), (0.74, 0.74)]);
        }
        Icon::Search => {
            circle(0.44, 0.44, 0.28);
            line(&[(0.65, 0.65), (0.88, 0.88)]);
        }
        Icon::Shade => {
            line(&[(0.12, 0.8), (0.4, 0.3), (0.6, 0.6), (0.72, 0.44), (0.9, 0.8), (0.12, 0.8)]);
            line(&[(0.46, 0.8), (0.6, 0.62)]);
            line(&[(0.62, 0.8), (0.74, 0.64)]);
            line(&[(0.78, 0.8), (0.86, 0.7)]);
        }
        Icon::Sidebar => {
            closed(&[(0.12, 0.18), (0.88, 0.18), (0.88, 0.82), (0.12, 0.82)]);
            line(&[(0.62, 0.18), (0.62, 0.82)]);
        }
        Icon::Sliders => {
            line(&[(0.14, 0.3), (0.86, 0.3)]);
            line(&[(0.14, 0.7), (0.86, 0.7)]);
            dot(0.62, 0.3, 0.1);
            dot(0.36, 0.7, 0.1);
        }
        Icon::Target => {
            circle(0.5, 0.5, 0.38);
            circle(0.5, 0.5, 0.18);
            dot(0.5, 0.5, 0.05);
        }
    }
}
