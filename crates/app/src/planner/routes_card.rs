//! The routes card: a big card in the middle of the map (Routes mode) with the list of routes on the left and, on the
//! right, what the selected one is — its numbers, its profile, what can be done with it.
//! Closed, it leaves a small «Routes» button; the map behind it is for looking at the route.

use super::routes::{ROUTE, elide, km, sample};
use super::*;

/// The card: centred in the free part of the map, as big as the map allows.
pub(super) fn card_rect(free: Rect) -> Rect {
    let w = (free.width() - 80.0).clamp(320.0, 1040.0).min(free.width());
    let h = (free.height() - 90.0).clamp(240.0, 660.0).min(free.height());
    Rect::from_center_size(free.center(), vec2(w, h))
}

/// Shows (`Some(true)`) or hides (`Some(false)`) every route on the map at once; `None` flips them: all hidden when
/// any is shown, else all shown. Returns whether they are shown now.
pub(super) fn set_all_visible(st: &mut PlannerState, visible: Option<bool>) -> bool {
    let show = visible.unwrap_or_else(|| !st.routes.iter().any(|r| r.visible));
    for r in &mut st.routes {
        r.visible = show;
    }
    show
}

/// «2 h 05 min» for a number of seconds.
fn duration(secs: f64) -> String {
    let m = (secs.max(0.0) / 60.0).round() as u64;
    if m >= 60 { format!("{} h {:02} min", m / 60, m % 60) } else { format!("{m} min") }
}

impl Page<'_> {
    /// The small button that opens the card again.
    pub(super) fn routes_chip(&mut self, ui: &mut Ui, rt: &mut Runtime, r: Rect) {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(Align::Min)));
        if text_button(&mut child, "labelSet-routesOpen", crate::i18n::tr("Routes"), false).clicked() {
            rt.routes_card_closed = false;
        }
    }

    pub(super) fn routes_card(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, r: Rect) {
        let t = self.t;
        let painter = ui.painter().clone();
        painter.rect_filled(Rect::EVERYTHING.intersect(ui.clip_rect()), 0.0, Color32::from_black_alpha(70));
        painter.rect_filled(r.translate(vec2(0.0, 4.0)).expand(2.0), 16.0, Color32::from_black_alpha(80));
        crate::theme::paint_chip(&painter, r, &t);
        let _ = ui.interact(ui.clip_rect(), egui::Id::new("planner-routes-scrim"), Sense::click_and_drag());
        let _ = ui.interact(r, egui::Id::new("planner-routes-bg"), Sense::click_and_drag());
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            rt.routes_card_closed = true;
        }

        let inner = r.shrink(16.0);
        let left_w = 280.0_f32.min(inner.width() * 0.45);
        let left = Rect::from_min_size(inner.min, vec2(left_w, inner.height()));
        let right = Rect::from_min_max(pos2(left.right() + 16.0, inner.top()), inner.max);
        painter.line_segment([pos2(left.right() + 8.0, inner.top()), pos2(left.right() + 8.0, inner.bottom())], Stroke::new(1.0, t.card_stroke));

        // the close button
        let close = Rect::from_center_size(pos2(r.right() - 22.0, r.top() + 22.0), vec2(26.0, 26.0));
        let on_close = ui.interact(close, egui::Id::new("planner-routes-close"), Sense::click());
        if on_close.hovered() {
            painter.rect_filled(close, 6.0, t.hover);
        }
        crate::icons::paint(&painter, close.shrink(6.0), Icon::Close, t.text_dim);
        if on_close.on_hover_text(crate::i18n::tr("Close")).clicked() {
            rt.routes_card_closed = true;
        }

        self.routes_card_list(ui, st, rt, left);
        let right = Rect::from_min_max(right.min, pos2(right.max.x - 30.0, right.max.y));
        self.routes_card_detail(ui, st, rt, right);
    }

    /// Left: the title, import, show/hide all, and the routes.
    fn routes_card_list(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, r: Rect) {
        let t = self.t;
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(Align::Min)));
        ui.set_clip_rect(r.intersect(ui.clip_rect()));
        ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
        let ui = &mut ui;
        let title =
            if st.routes.is_empty() { crate::i18n::tr("Routes").to_string() } else { format!("{} ({})", crate::i18n::tr("Routes"), st.routes.len()) };
        ui.label(egui::RichText::new(title).font(t.semibold(15.0)).color(t.text));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if text_button(ui, "plannerRoutesImport", "Import GPX…", false).clicked() {
                self.intents.push(Intent::ImportRoute);
            }
            if !st.routes.is_empty() {
                let any = st.routes.iter().any(|r| r.visible);
                let label = crate::i18n::tr(if any { "Hide all" } else { "Show all" });
                if text_button(ui, "labelSet-routesAll", label, false).clicked() {
                    set_all_visible(st, None);
                }
            }
        });
        ui.add_space(8.0);
        if st.routes.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(crate::i18n::tr("No routes yet. Import a GPX file, for example one downloaded from Wikiloc."))
                        .font(t.font(12.0))
                        .color(t.text_dim),
                )
                .wrap(),
            );
            return;
        }
        let (mut select, mut remove): (Option<u64>, Option<u64>) = (None, None);
        egui::ScrollArea::vertical().id_salt("routes-card-list").auto_shrink([false, false]).show(ui, |ui| {
            for route in &mut st.routes {
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
                let eye = Rect::from_center_size(pos2(row.left() + 15.0, row.center().y), vec2(26.0, 32.0));
                let bin = Rect::from_center_size(pos2(row.right() - 15.0, row.center().y), vec2(26.0, 32.0));
                let body = Rect::from_min_max(pos2(row.left() + 32.0, row.top()), pos2(row.right() - 32.0, row.bottom()));
                let on_body = ui.interact(body, egui::Id::new(("planner-route", route.id)), Sense::click());
                let on_eye = ui.interact(eye, egui::Id::new(("planner-route-eye", route.id)), Sense::click());
                let on_bin = ui.interact(bin, egui::Id::new(("planner-route-del", route.id)), Sense::click());
                let selected = rt.route_sel == Some(route.id);
                let p = ui.painter();
                if selected {
                    crate::theme::paint_chosen(p, row, 8.0, &t);
                } else if on_body.hovered() || on_eye.hovered() || on_bin.hovered() {
                    p.rect_filled(row, 7.0, t.hover.gamma_multiply(0.5));
                }
                let icon = if route.visible { Icon::Eye } else { Icon::EyeOff };
                crate::icons::paint(
                    p,
                    Rect::from_center_size(eye.center(), vec2(16.0, 16.0)),
                    icon,
                    if route.visible { ROUTE } else { t.text_disabled },
                );
                crate::icons::paint(p, Rect::from_center_size(bin.center(), vec2(15.0, 15.0)), Icon::Trash, t.text_dim);
                let dim = if route.visible { t.text } else { t.text_dim };
                p.text(pos2(body.left() + 2.0, body.center().y - 9.0), Align2::LEFT_CENTER, elide(&route.name, 28), t.semibold(13.0), dim);
                let detail = format!("{} · +{:.0} m · -{:.0} m", km(route.distance_m), route.ascent_m, route.descent_m);
                p.text(pos2(body.left() + 2.0, body.center().y + 9.0), Align2::LEFT_CENTER, detail, t.font(11.5), t.text_dim);
                if on_eye.clicked() {
                    route.visible = !route.visible;
                }
                if on_bin.clicked() {
                    remove = Some(route.id);
                } else if on_body.clicked() {
                    select = Some(route.id);
                }
                on_eye.on_hover_text(crate::i18n::tr(if route.visible { "Hide the Route" } else { "Show the Route" }));
                on_bin.on_hover_text(crate::i18n::tr("Remove the Route"));
            }
        });
        if let Some(id) = remove {
            st.routes.retain(|r| r.id != id);
            if rt.route_sel == Some(id) {
                rt.route_sel = None;
            }
        }
        if let Some(id) = select {
            if let Some(route) = st.routes.iter_mut().find(|r| r.id == id) {
                route.visible = true;
            }
            rt.route_sel = Some(id);
            rt.route_fit = Some(id);
        }
    }

    /// The route on a small map: the tiles of the main map, the line, and where it starts and ends.
    fn route_minimap(&mut self, ui: &Ui, r: Rect, route: &SavedRoute) {
        let t = self.t;
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let painter = ui.painter_at(r);
        painter.rect_filled(r, 8.0, t.inset);
        let [min_la, min_lo, max_la, max_lo] = route.bounds;
        let (x0, y0) = crate::services::map_tiles::project(max_la, min_lo);
        let (x1, y1) = crate::services::map_tiles::project(min_la, max_lo);
        let Some(view) = crate::map::fit(&[(x0, y0), (x1, y1)], r) else { return };
        let cam = crate::map::Cam::new(view, r);
        crate::map::draw_tiles(&mut *self.tiles, &self.carto, self.satellite, view.zoom, &ctx, &painter, r, cam, now, false);
        let line: Vec<Pos2> = route
            .points
            .iter()
            .map(|p| {
                let (wx, wy) = crate::services::map_tiles::project(p.0, p.1);
                cam.screen(wx, wy)
            })
            .collect();
        painter.add(egui::Shape::line(line.clone(), Stroke::new(6.5, Color32::from_black_alpha(150))));
        painter.add(egui::Shape::line(line.clone(), Stroke::new(3.5, ROUTE)));
        for (at, colour) in [(line.first(), Color32::from_rgb(52, 199, 89)), (line.last(), Color32::from_rgb(255, 69, 58))] {
            if let Some(&at) = at {
                painter.circle_filled(at, 5.5, colour);
                painter.circle_stroke(at, 5.5, Stroke::new(1.5, Color32::WHITE));
            }
        }
        painter.rect_stroke(r, 8.0, Stroke::new(1.0, t.card_stroke), egui::StrokeKind::Inside);
        let credit = self.tiles.credit(self.satellite).to_string();
        if !credit.is_empty() {
            painter.text(r.right_bottom() - vec2(6.0, 4.0), Align2::RIGHT_BOTTOM, credit, t.font(9.5), Color32::from_white_alpha(200));
        }
    }

    /// Right: the selected route.
    fn routes_card_detail(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, r: Rect) {
        let t = self.t;
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(Align::Min)));
        ui.set_clip_rect(r.intersect(ui.clip_rect()));
        let ui = &mut ui;
        let Some(route) = st.routes.iter().find(|x| Some(x.id) == rt.route_sel) else {
            ui.add_space(r.height() * 0.4);
            let msg = if st.routes.is_empty() { "" } else { "Pick a route in the list to see it here." };
            ui.vertical_centered(|ui| ui.label(egui::RichText::new(crate::i18n::tr(msg)).font(t.font(13.0)).color(t.text_dim)));
            return;
        };
        let (id, on_phone, waypoints) = (route.id, !route.phone_id.is_empty(), route.waypoints.len());
        // the route on a small map, above everything else
        let map_h = (r.height() * 0.36).clamp(110.0, 260.0);
        let mini = Rect::from_min_size(r.min, vec2(r.width(), map_h));
        self.route_minimap(ui, mini, route);
        ui.add_space(map_h + 10.0);
        egui::ScrollArea::vertical().id_salt("routes-card-detail").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.label(egui::RichText::new(&route.name).font(t.semibold(19.0)).color(t.text));
            if !route.file.is_empty() {
                ui.label(egui::RichText::new(&route.file).font(t.font(11.5)).color(t.text_dim));
            }
            ui.add_space(8.0);

            // the numbers, three to a row
            let (lo, hi) = if route.profile.len() >= 2 {
                let (a, b) = route.profile.iter().fold((f32::MAX, f32::MIN), |(a, b), &h| (a.min(h), b.max(h)));
                (Some(a), Some(b))
            } else {
                (None, None)
            };
            let m = |v: Option<f32>| v.map_or("–".to_string(), |v| format!("{v:.0} m"));
            let time = match (route.times.first(), route.times.last()) {
                (Some(a), Some(b)) if route.times.len() >= 2 && b > a => duration(b - a),
                _ => "–".to_string(),
            };
            let cells: [(&str, String); 6] = [
                ("Distance", km(route.distance_m)),
                ("Ascent", format!("+{:.0} m", route.ascent_m)),
                ("Descent", format!("-{:.0} m", route.descent_m)),
                ("Highest point", m(hi)),
                ("Lowest point", m(lo)),
                ("Duration", time),
            ];
            let w = ui.available_width() / 3.0;
            for row in cells.chunks(3) {
                let (cell, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
                for (i, (label, value)) in row.iter().enumerate() {
                    let x = cell.left() + w * i as f32;
                    ui.painter().text(pos2(x, cell.top() + 8.0), Align2::LEFT_CENTER, crate::i18n::tr(label), t.font(11.5), t.text_dim);
                    ui.painter().text(pos2(x, cell.top() + 28.0), Align2::LEFT_CENTER, value, t.semibold(17.0), t.text);
                }
            }

            // the profile
            if route.profile.len() >= 2 {
                ui.add_space(4.0);
                let (pr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(pr, 8.0, t.inset);
                let inner = pr.shrink2(vec2(8.0, 8.0));
                let (lo, hi) = (lo.unwrap_or(0.0), hi.unwrap_or(0.0).max(lo.unwrap_or(0.0) + 10.0));
                let y_of = |h: f32| inner.bottom() - (h - lo) / (hi - lo) * inner.height();
                let steps = (inner.width() / 2.0).max(2.0) as usize;
                let mut line = Vec::with_capacity(steps + 1);
                for i in 0..=steps {
                    let f = i as f32 / steps as f32;
                    let x = inner.left() + f * inner.width();
                    let y = y_of(sample(&route.profile, f));
                    p.line_segment([pos2(x, y), pos2(x, inner.bottom())], Stroke::new(2.0, ROUTE.gamma_multiply(0.25)));
                    line.push(pos2(x, y));
                }
                p.add(egui::Shape::line(line, Stroke::new(2.0, ROUTE)));
            }

            // what can be done with it
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if text_button(ui, "labelSet-routeView", crate::i18n::tr("View on the Map"), true).clicked() {
                    rt.route_fit = Some(id);
                    rt.routes_card_closed = true;
                }
                if waypoints > 0 {
                    let label = format!("{} ({waypoints})", crate::i18n::tr("Waypoints to Points of Interest"));
                    if text_button(ui, "labelSet-routeWaypoints", &label, false).clicked() {
                        self.intents.push(Intent::RouteWaypoints(id));
                    }
                }
                let send = crate::i18n::tr(if on_phone { "Send again to the phone" } else { "Send to the phone" });
                if text_button(ui, "labelSet-routeToPhone", send, false).clicked() {
                    self.intents.push(Intent::RouteToPhone(id, false));
                }
                if on_phone && text_button(ui, "labelSet-routeOffPhone", crate::i18n::tr("Remove from the phone"), false).clicked() {
                    self.intents.push(Intent::RouteToPhone(id, true));
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_fits_in_what_is_free_and_stays_centred() {
        let free = Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 1000.0));
        let c = card_rect(free);
        assert!(free.contains_rect(c) && (c.center() - free.center()).length() < 1.0);
        assert!(c.width() <= 1040.0 && c.height() <= 660.0);
        let small = Rect::from_min_size(pos2(10.0, 20.0), vec2(500.0, 400.0));
        assert!(small.contains_rect(card_rect(small)));
    }

    #[test]
    fn all_routes_are_shown_or_hidden_at_once() {
        let mut st = PlannerState {
            routes: vec![SavedRoute { id: 1, visible: true, ..Default::default() }, SavedRoute { id: 2, visible: false, ..Default::default() }],
            ..Default::default()
        };
        assert!(!set_all_visible(&mut st, None), "some are shown: flipping hides them all");
        assert!(st.routes.iter().all(|r| !r.visible));
        assert!(set_all_visible(&mut st, None), "none shown: flipping shows them all");
        assert!(st.routes.iter().all(|r| r.visible));
        assert!(!set_all_visible(&mut st, Some(false)) && st.routes.iter().all(|r| !r.visible));
        assert_eq!(duration(18300.0), "5 h 05 min");
        assert_eq!(duration(2400.0), "40 min");
    }
}
