//! The camera on the map: the field of view as a wedge from the place (the sensor's angle across the frame at the
//! focal length, the central line with a handle to turn it) or the depth of field as arcs across it (the nearest
//! and farthest sharp distance in grey, the focus in red, the hyperfocal distance in blue). The distances are
//! real ones, scaled to the map.

use light_studio_astro::optics;

use super::skylayers::dashed;
use super::*;
use crate::map::Cam;

const FOCUS_RED: Color32 = Color32::from_rgb(235, 64, 52);
const HYPER_BLUE: Color32 = Color32::from_rgb(70, 140, 255);

/// Screen points per metre at a latitude for a camera.
fn points_per_metre(cam: &Cam, lat: f64) -> f64 {
    cam.s / (40_075_016.7 * lat.to_radians().cos().abs().max(0.02))
}

fn dir(az: f64) -> egui::Vec2 {
    let a = az.to_radians();
    vec2(a.sin() as f32, -a.cos() as f32)
}

/// An arc round `origin` between two azimuths, as points.
fn arc(origin: Pos2, r: f32, from: f64, to: f64) -> Vec<Pos2> {
    let n = (((to - from).abs() / 2.0).ceil() as usize).clamp(2, 90);
    (0..=n).map(|i| origin + dir(from + (to - from) * i as f64 / n as f64) * r).collect()
}

impl Page<'_> {
    pub(super) fn camera_tool(&mut self, ui: &mut Ui, painter: &egui::Painter, st: &mut PlannerState, cam: &Cam, origin: Pos2, reach: f32) {
        let t = self.t;
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        let (across, other) = if st.portrait { (sensor.height, sensor.width) } else { (sensor.width, sensor.height) };
        let Some(fov) = optics::field_of_view(across, st.focal) else { return };
        let half = (fov / 2.0).min(89.0);
        let az = st.cam_az;
        let length = reach * 0.95;
        let (left, right) = (origin + dir(az - half) * length, origin + dir(az + half) * length);
        // the wedge
        painter.add(Shape::convex_polygon(
            std::iter::once(origin).chain(arc(origin, length, az - half, az + half)).collect(),
            Color32::from_white_alpha(18),
            Stroke::NONE,
        ));
        for end in [left, right] {
            painter.line_segment([origin, end], Stroke::new(2.4, Color32::from_black_alpha(90)));
            painter.line_segment([origin, end], Stroke::new(1.4, Color32::WHITE));
        }
        let tip = origin + dir(az) * length;
        dashed(painter, origin, tip, Stroke::new(1.6, Color32::from_white_alpha(200)));
        if st.cam_tool == 2 {
            let ppm = points_per_metre(cam, st.lat);
            if let Some(d) = optics::depth_of_field(st.focal, st.aperture, sensor.coc, st.distance * 1000.0) {
                let at = |mm: f64| (mm / 1000.0 * ppm) as f32;
                let ring = |mm: f64, color: Color32, width: f32| {
                    let r = at(mm);
                    if r > 3.0 && r < length * 6.0 {
                        let pts: Vec<Pos2> = arc(origin, r.min(length * 6.0), az - half, az + half);
                        painter.add(Shape::line(pts, Stroke::new(width + 2.0, Color32::from_black_alpha(90))));
                        painter.add(Shape::line(arc(origin, r, az - half, az + half), Stroke::new(width, color)));
                    }
                };
                ring(d.near, Color32::from_gray(205), 3.0);
                if let Some(far) = d.far {
                    ring(far, Color32::from_gray(205), 3.0);
                }
                ring(d.hyperfocal, HYPER_BLUE, 2.0);
                ring(st.distance * 1000.0, FOCUS_RED, 2.5);
            }
        }
        // the angle, over the wedge
        let label = format!("{fov:.1}° × {:.1}°", optics::field_of_view(other, st.focal).unwrap_or(0.0));
        let g = painter.layout_no_wrap(label, t.semibold(11.0), t.text);
        let chip = Rect::from_center_size(origin + dir(az) * (length + 26.0), g.size() + vec2(12.0, 6.0));
        painter.rect_filled(chip, 9.0, crate::theme::chip_fill(&t));
        painter.rect_stroke(chip, 9.0, Stroke::new(1.2, Color32::WHITE), StrokeKind::Inside);
        painter.galley(chip.center() - g.size() / 2.0, g, t.text);
        // the handle on the central line: drag it round
        let handle = Rect::from_center_size(tip, vec2(26.0, 26.0));
        let resp = ui.interact(handle, egui::Id::new("planner-cam-handle"), Sense::drag());
        painter.circle_filled(tip, 7.5, Color32::WHITE);
        painter.circle_stroke(tip, 7.5, Stroke::new(1.5, Color32::from_black_alpha(160)));
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let d = p - origin;
            if d.length() > 4.0 {
                st.cam_az = f64::from(d.x).atan2(-f64::from(d.y)).to_degrees().rem_euclid(360.0);
            }
        }
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arcs_run_between_their_azimuths() {
        let o = pos2(0.0, 0.0);
        let a = arc(o, 100.0, 0.0, 90.0);
        let (first, last) = (a[0], a[a.len() - 1]);
        assert!((first.x).abs() < 1e-3 && (first.y + 100.0).abs() < 1e-3, "{first:?}");
        assert!((last.x - 100.0).abs() < 1e-3 && last.y.abs() < 1e-3, "{last:?}");
        assert!(a.iter().all(|p| (p.to_vec2().length() - 100.0).abs() < 1e-3));
    }

    #[test]
    fn scale_follows_the_zoom_and_the_latitude() {
        let cam = Cam::new(crate::map::MapView { lat: 40.0, lon: 0.0, zoom: 10.0 }, Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)));
        let near = points_per_metre(&cam, 0.0);
        let far = points_per_metre(&cam, 60.0);
        assert!(near > 0.0 && (far / near - 2.0).abs() < 0.01, "{near} {far}");
        assert!(points_per_metre(&cam, 90.0).is_finite());
    }
}
