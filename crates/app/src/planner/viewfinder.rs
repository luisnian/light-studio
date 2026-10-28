//! The viewfinder: what the camera sees from the place, pointed where the wedge on the map points. The skyline (the
//! ground heights, else a flat horizon), the Sun, the Moon and the Milky Way are drawn by a rectilinear projection
//! for the sensor and focal length chosen, at the time shown, with the sizes the sky really has. Other focal lengths
//! show as frames over the same picture. A geometric diagram of the view, not a photograph: no lens distortion, no
//! trees or buildings, and the brightness of the sky is only a hint of the light.

use light_studio_astro::frame::{Frame, SensorPoint};
use light_studio_astro::info::{moon_size, sun_size};
use light_studio_astro::sky::milky_way_plane;
use light_studio_astro::{core_horizontal, moon_horizontal, moon_phase, sun_horizontal};

use super::skylayers::{SUNRISE, SUNSET};
use super::*;

/// The focal lengths that can be laid over the picture.
const COMPARE: [f64; 8] = [14.0, 24.0, 35.0, 50.0, 85.0, 135.0, 200.0, 400.0];
const OUTLINES: [Color32; 4] =
    [Color32::from_rgb(255, 200, 80), Color32::from_rgb(120, 220, 255), Color32::from_rgb(255, 130, 170), Color32::from_rgb(160, 255, 150)];
/// The widest span of azimuth drawn either side of the axis (a rectilinear picture has nothing past ~85° off the axis).
const MAX_SPAN: f64 = 88.0;

/// A framing kept with a plan: where the camera looked, with what.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FrameShot {
    pub az: f64,
    pub alt: f64,
    pub focal: f64,
    pub portrait: bool,
    pub sensor: usize,
}

impl Default for FrameShot {
    fn default() -> Self {
        FrameShot { az: 90.0, alt: 0.0, focal: 24.0, portrait: false, sensor: optics::CANON_6D }
    }
}

impl FrameShot {
    /// The framing the planner has now.
    pub fn of(st: &PlannerState) -> FrameShot {
        FrameShot { az: st.cam_az, alt: st.cam_alt, focal: st.focal, portrait: st.portrait, sensor: st.sensor }
    }

    /// Puts the framing back in the planner (numbers out of range are brought in).
    pub fn apply(&self, st: &mut PlannerState) {
        let fix = |v: f64, lo: f64, hi: f64, dflt: f64| if v.is_finite() { v.clamp(lo, hi) } else { dflt };
        st.cam_az = fix(self.az, 0.0, 360.0, 90.0);
        st.cam_alt = fix(self.alt, -60.0, 80.0, 0.0);
        st.focal = fix(self.focal, 1.0, 2000.0, 24.0);
        st.portrait = self.portrait;
        st.sensor = if self.sensor < SENSORS.len() { self.sensor } else { optics::CANON_6D };
        st.show_viewfinder = true;
        if st.cam_tool == 0 {
            st.cam_tool = 1;
        }
    }
}

/// The azimuths to sample, centred on `centre`, `span` either side, every `step` degrees.
fn azimuths(centre: f64, span: f64, step: f64) -> impl Iterator<Item = f64> {
    let n = (2.0 * span / step).ceil().max(1.0) as usize;
    (0..=n).map(move |i| centre - span + 2.0 * span * i as f64 / n as f64)
}

impl Page<'_> {
    /// The viewfinder card, at the foot of the map's free part.
    pub(super) fn viewfinder(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, free: Rect) {
        if !st.show_viewfinder {
            return;
        }
        let t = self.t;
        let room = free.height() - 42.0 - 36.0 - 6.0 - 68.0 - 16.0;
        let w = (free.width() - 16.0).clamp(0.0, 460.0);
        let h = room.min(w * 1.0 + 120.0).min(free.height() - 16.0);
        if w < 260.0 || h < 260.0 {
            return;
        }
        let r = Rect::from_min_size(pos2(free.left() + 4.0, free.bottom() - h - 4.0), vec2(w, h));
        super::layout::surface(ui.painter(), r, &t);
        let _ = ui.interact(r, egui::Id::new("planner-viewfinder-bg"), Sense::click_and_drag());
        ui.painter().text(pos2(r.left() + 14.0, r.top() + 18.0), Align2::LEFT_CENTER, crate::i18n::tr("Viewfinder"), t.semibold(14.0), t.text);
        let close = Rect::from_min_size(pos2(r.right() - 34.0, r.top() + 4.0), vec2(28.0, 28.0));
        let mut head = ui.new_child(egui::UiBuilder::new().max_rect(close));
        if icon_button(&mut head, "plannerViewfinderClose", Icon::Close, vec2(28.0, 28.0), false, true, "Close").clicked() {
            st.show_viewfinder = false;
            return;
        }

        // the controls at the foot
        let controls_h = 108.0;
        let controls = Rect::from_min_max(pos2(r.left() + 12.0, r.bottom() - controls_h), pos2(r.right() - 12.0, r.bottom() - 6.0));
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(controls).layout(egui::Layout::top_down(Align::Min)));
        cui.set_clip_rect(controls);
        self.viewfinder_controls(&mut cui, st, rt);

        let area = Rect::from_min_max(pos2(r.left() + 12.0, r.top() + 38.0), pos2(r.right() - 12.0, controls.top() - 4.0));
        let sensor = SENSORS.get(st.sensor).copied().unwrap_or(SENSORS[0]);
        let (sw, sh) = if st.portrait { (sensor.height, sensor.width) } else { (sensor.width, sensor.height) };
        // the picture is drawn at the widest focal length asked for; the others are frames over it
        let chosen: Vec<f64> = COMPARE.iter().enumerate().filter(|(i, _)| st.vf_compare & (1 << i) != 0).map(|(_, f)| *f).collect();
        let scene_focal = chosen.iter().copied().fold(st.focal, f64::min);
        let (Some(scene), Some(own)) = (Frame::new(st.cam_az, st.cam_alt, scene_focal, sw, sh), Frame::new(st.cam_az, st.cam_alt, st.focal, sw, sh))
        else {
            return;
        };
        let scale = f64::from((area.width() / sw as f32).min(area.height() / sh as f32));
        let size = vec2((sw * scale) as f32, (sh * scale) as f32);
        let pic = Rect::from_center_size(area.center(), size);
        let to_screen = |p: SensorPoint| pos2(pic.center().x + (p.x * scale) as f32, pic.center().y - (p.y * scale) as f32);

        let unix = self.c.start + self.minute * 60.0;
        let obs = self.obs;
        let sun = sun_horizontal(unix, obs);
        let painter = ui.painter_at(pic);
        // the sky
        let (top, bottom) = super::horizon::sky_colours(sun.alt, ui.visuals().dark_mode);
        let bands = 24;
        for i in 0..bands {
            let (f0, f1) = (i as f32 / bands as f32, (i + 1) as f32 / bands as f32);
            let band = Rect::from_min_max(pos2(pic.left(), pic.top() + pic.height() * f0), pos2(pic.right(), pic.top() + pic.height() * f1 + 0.5));
            painter.rect_filled(band, 0.0, top.lerp_to_gamma(bottom, (f0 + f1) / 2.0));
        }
        // the Milky Way's plane, when the sky is dark enough to see it
        if sun.alt < -12.0 {
            let plane = milky_way_plane(unix, obs, 2.0);
            let pts: Vec<Option<Pos2>> = plane.iter().map(|p| scene.project(p.at.az, p.at.alt).map(&to_screen)).collect();
            for (a, b) in pts.iter().zip(pts.iter().cycle().skip(1)) {
                if let (Some(a), Some(b)) = (a, b)
                    && (*a - *b).length() < 60.0
                {
                    painter.line_segment([*a, *b], Stroke::new(14.0, self.pal.core.gamma_multiply(0.18)));
                }
            }
        }
        // the Moon and the Sun, with their real angular sizes; the ground drawn after hides what is behind it
        let moon = moon_horizontal(unix, obs);
        let (lit, shade) = moon_colors(ui.visuals().dark_mode, self.pal, t.inset);
        let core = core_horizontal(unix, obs);
        let mut labels: Vec<(Pos2, String)> = Vec::new();
        let draw_body = |name: &str,
                         at: light_studio_astro::Horizontal,
                         diameter: f64,
                         min_r: f32,
                         disc: &dyn Fn(Pos2, f32),
                         labels: &mut Vec<(Pos2, String)>| {
            if let Some(p) = scene.project(at.az, at.alt) {
                let radius = ((scene.disc_radius(diameter, p) * scale) as f32).max(min_r);
                let c = to_screen(p);
                disc(c, radius);
                labels.push((c + vec2(radius + 4.0, -radius), format!("{} {:.2}°", crate::i18n::tr(name), diameter)));
            }
        };
        draw_body(
            "Sun",
            sun,
            sun_size(unix).0,
            3.0,
            &|c, r| {
                painter.circle_filled(c, r, if sun.alt < 6.0 { SUNSET } else { SUNRISE });
            },
            &mut labels,
        );
        let ph = moon_phase(unix);
        draw_body(
            "Moon",
            moon,
            moon_size(unix).0,
            3.0,
            &|c, r| moon_disc(&painter, c, r, ph.elongation, st.lat < 0.0, lit, shade, t.divider),
            &mut labels,
        );
        if let Some(p) = scene.project(core.az, core.alt) {
            let c = to_screen(p);
            painter.circle_stroke(c, 7.0, Stroke::new(1.5, self.pal.core));
            labels.push((c + vec2(10.0, -8.0), crate::i18n::tr("Milky Way core").to_string()));
        }
        // the ground: the skyline from the heights when they are read for this place, else a flat horizon
        let key = super::horizon::key_of(st);
        let skyline = rt.horizon.as_ref().filter(|h| h.key == key).and_then(|h| h.data.as_ref().ok().cloned());
        let span = MAX_SPAN.min(scene.field().map_or(MAX_SPAN, |(fw, fh)| fw.hypot(fh) / 2.0 + 6.0));
        let ground = Color32::from_rgb(22, 26, 24);
        let mut last: Option<Pos2> = None;
        for az in azimuths(st.cam_az, span, 0.5) {
            let alt = skyline.as_ref().map_or(0.0, |s| s.altitude_at(az));
            let Some(p) = scene.project(az, alt).map(&to_screen) else {
                last = None;
                continue;
            };
            if let Some(q) = last {
                let floor = pic.bottom() + 40.0;
                painter.add(Shape::convex_polygon(vec![q, p, pos2(p.x, floor), pos2(q.x, floor)], ground, Stroke::new(0.5, ground)));
                painter.line_segment([q, p], Stroke::new(1.2, Color32::from_gray(90)));
            }
            last = Some(p);
        }
        for (pos, text) in labels {
            painter.text(pos, Align2::LEFT_BOTTOM, text, t.font(10.5), Color32::WHITE);
        }
        // the frame of the focal length in use, and the others over it
        let frame_rect = |f: &Frame| {
            let k = f.focal.recip() * scene_focal;
            Rect::from_center_size(pic.center(), vec2(size.x * k as f32, size.y * k as f32))
        };
        for (i, f) in chosen.iter().enumerate() {
            if let Some(fr) = own.with_focal(*f) {
                let rr = frame_rect(&fr);
                let color = OUTLINES[i % OUTLINES.len()];
                painter.rect_stroke(rr, 0.0, Stroke::new(1.4, color), StrokeKind::Inside);
                painter.text(rr.left_top() + vec2(3.0, 2.0), Align2::LEFT_TOP, format!("{f:.0} mm"), t.font(10.0), color);
            }
        }
        painter.rect_stroke(frame_rect(&own), 0.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
        ui.painter().rect_stroke(pic, 0.0, Stroke::new(1.0, t.divider), StrokeKind::Outside);
        // what the picture says about itself
        let (fw, fh) = own.field().unwrap_or((0.0, 0.0));
        let source = if skyline.is_some() {
            crate::i18n::tr("Skyline from the ground heights")
        } else {
            crate::i18n::tr("Flat horizon: turn on Mountains for the real skyline")
        };
        ui.painter().text(
            pos2(r.left() + 14.0, pic.bottom() + 2.0 + 0.0),
            Align2::LEFT_TOP,
            format!("{fw:.1}° × {fh:.1}° · {source}"),
            t.font(10.5),
            t.text_dim,
        );
    }

    fn viewfinder_controls(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime) {
        let unix = self.c.start + self.minute * 60.0;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
            ui.label(egui::RichText::new(crate::i18n::tr("Azimuth (°)")).font(self.t.font(11.5)).color(self.t.text_dim));
            number(ui, rt, "vfAz", &mut st.cam_az, (0.0, 360.0), 0, 54.0);
            ui.label(egui::RichText::new(crate::i18n::tr("Tilt (°)")).font(self.t.font(11.5)).color(self.t.text_dim));
            number(ui, rt, "vfAlt", &mut st.cam_alt, (-60.0, 80.0), 0, 54.0);
            if text_button(ui, "plannerVfPortrait", "Portrait", st.portrait).clicked() {
                st.portrait = !st.portrait;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
            ui.label(egui::RichText::new(crate::i18n::tr("Aim at")).font(self.t.font(11.5)).color(self.t.text_dim));
            let targets = [
                ("plannerVfSun", "Sun", sun_horizontal(unix, self.obs)),
                ("plannerVfMoon", "Moon", moon_horizontal(unix, self.obs)),
                ("plannerVfCore", "Milky Way", core_horizontal(unix, self.obs)),
            ];
            for (id, label, at) in targets {
                if text_button(ui, id, label, false).clicked() && at.az.is_finite() && at.alt.is_finite() {
                    st.cam_az = at.az.rem_euclid(360.0);
                    st.cam_alt = at.alt.clamp(-60.0, 80.0);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(5.0, 4.0);
            ui.label(egui::RichText::new(crate::i18n::tr("Compare")).font(self.t.font(11.5)).color(self.t.text_dim));
            for (i, f) in COMPARE.iter().enumerate() {
                let on = st.vf_compare & (1 << i) != 0;
                if text_button(ui, &format!("plannerVfCompare{i}"), &format!("{f:.0}"), on).clicked() {
                    st.vf_compare ^= 1 << i;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_framing_survives_hostile_numbers() {
        let mut st = PlannerState::default();
        FrameShot { az: f64::NAN, alt: 500.0, focal: -3.0, portrait: true, sensor: 99 }.apply(&mut st);
        assert!(st.cam_az.is_finite() && (-60.0..=80.0).contains(&st.cam_alt) && st.focal >= 1.0 && st.sensor < SENSORS.len());
        assert!(st.show_viewfinder && st.cam_tool > 0 && st.portrait);
    }

    #[test]
    fn the_framing_of_the_planner_round_trips() {
        let st = PlannerState { cam_az: 123.0, cam_alt: 12.0, focal: 70.0, portrait: true, sensor: 2, ..Default::default() };
        let mut other = PlannerState::default();
        FrameShot::of(&st).apply(&mut other);
        assert_eq!((other.cam_az, other.cam_alt, other.focal, other.portrait, other.sensor), (123.0, 12.0, 70.0, true, 2));
    }

    #[test]
    fn azimuths_cover_the_span_with_both_ends() {
        let v: Vec<f64> = azimuths(90.0, 30.0, 0.5).collect();
        assert_eq!((v.first().copied(), v.last().copied()), (Some(60.0), Some(120.0)));
        assert!(v.windows(2).all(|w| w[1] > w[0] && w[1] - w[0] <= 0.5 + 1e-9));
    }
}
