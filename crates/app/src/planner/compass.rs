//! The map's compass: a dial in the map's bottom right corner. The map is always north-up, so the dial is fixed;
//! what it adds is where the Sun and the Moon stand (their azimuths, as small discs on its rim) at the time shown.

use super::*;

const R: f32 = 28.0;

impl Page<'_> {
    pub(super) fn compass(&mut self, ui: &mut Ui, free: Rect) {
        let t = self.t;
        let c = pos2(free.right() - R - 14.0, free.bottom() - R - 14.0);
        let r = Rect::from_center_size(c, vec2(2.0 * R, 2.0 * R));
        if free.width() < 4.0 * R || free.height() < 4.0 * R {
            return;
        }
        let resp = ui.interact(r, egui::Id::new("planner-compass"), Sense::hover());
        let p = ui.painter();
        p.circle_filled(c, R, t.inset.gamma_multiply(0.92));
        p.circle_stroke(c, R, Stroke::new(1.0, t.divider));
        let at = |az: f64, len: f32| -> Pos2 {
            let a = az.to_radians();
            c + vec2(a.sin() as f32, -(a.cos() as f32)) * len
        };
        for k in 0..12 {
            let az = f64::from(k) * 30.0;
            let major = k % 3 == 0;
            p.line_segment([at(az, R - if major { 7.0 } else { 4.0 }), at(az, R - 1.5)], Stroke::new(if major { 1.4 } else { 1.0 }, t.text_dim));
        }
        for (az, label, color) in
            [(0.0, "N", Color32::from_rgb(235, 80, 70)), (90.0, "E", t.text_dim), (180.0, "S", t.text_dim), (270.0, "W", t.text_dim)]
        {
            p.text(at(az, R - 15.0), Align2::CENTER_CENTER, crate::i18n::tr_ctx("compass", label), t.semibold(10.0), color);
        }
        // the north needle
        p.add(Shape::convex_polygon(vec![at(0.0, R - 21.0), c + vec2(-3.0, 0.0), c + vec2(3.0, 0.0)], Color32::from_rgb(235, 80, 70), Stroke::NONE));
        p.circle_filled(c, 2.0, t.text_dim);
        // the Sun and the Moon on the rim (hollow when below the horizon)
        for (h, color) in [(self.skies.sun, self.pal.sun), (self.skies.moon, self.pal.moon)] {
            if !h.az.is_finite() {
                continue;
            }
            let pt = at(h.az, R + 6.0);
            if h.alt > 0.0 {
                p.circle_filled(pt, 4.5, color);
            } else {
                p.circle_stroke(pt, 4.0, Stroke::new(1.4, color));
            }
        }
        resp.on_hover_text(crate::i18n::tr("North is up. The discs mark the Sun and the Moon."));
    }
}
