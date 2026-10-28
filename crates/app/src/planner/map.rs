//! The planner's map: the place on the same quiet tiles as the Map view, with a line along the
//! direction of each sunrise, sunset, moonrise, moonset and rise and set of the Milky Way's core
//! (with its time), and an arrow to where each body is at the time shown. Click the map to move
//! the place there; drag to pan; scroll or pinch to zoom.
//!
//! The tiles, their cache and their credit are the Map view's own ([`crate::map`]), so a place
//! seen in one shows at once in the other, and the CARTO key set for one serves both.

use crate::services::map_tiles;

use super::skylayers::{LineStyle, MOON_NOW, MOONRISE, MOONSET, SUN_NOW, SUNRISE, SUNSET, band, geodesic, wedge};
use super::*;
use crate::map::{Cam, MAX_ZOOM, MIN_ZOOM, clamp_view, draw_tiles, view_at, zoom_about};

impl Page<'_> {
    /// The map over all of `outer`; `free` is the part not under the floating panels (where its own buttons, hint and
    /// credit go) and `blocked` the panels themselves (the map takes no clicks, drags or scrolling there).
    pub(super) fn map_view(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, outer: Rect, free: Rect, blocked: &[Rect]) {
        let t = self.t;
        let pal = self.pal;
        let c = self.c;
        let ctx = ui.ctx().clone();
        let resp = ui.interact(outer, egui::Id::new("planner-map"), Sense::click_and_drag());
        let now = ctx.input(|i| i.time);
        let mode = st.map_mode;
        let planner = mode == MapMode::Planner;
        self.tiles.next_frame();
        st.map = clamp_view(st.map.sane(), outer);
        let under_panel = |p: Option<Pos2>| p.is_some_and(|p| blocked.iter().any(|r| r.contains(p)));
        let (hover, origin) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.press_origin()));
        let free_pointer = !under_panel(hover);
        // (the map is centred in the part not under the panels)
        let input_ok = !(under_panel(origin) || under_panel(resp.interact_pointer_pos()));

        // pan, zoom
        if input_ok && resp.dragged() {
            let d = resp.drag_delta();
            if d != egui::Vec2::ZERO {
                let cam = Cam::new(st.map, free);
                st.map = clamp_view(view_at(cam.cx - f64::from(d.x) / cam.s, cam.cy - f64::from(d.y) / cam.s, st.map.zoom), outer);
            }
        }
        if resp.contains_pointer() && free_pointer {
            let (scroll, pinch, at) = ctx.input(|i| (i.smooth_scroll_delta(), i.zoom_delta(), i.pointer.hover_pos()));
            let dz = f64::from(scroll.y) / 180.0 + f64::from(pinch).log2();
            if dz.abs() > 1e-4
                && let Some(at) = at
            {
                st.map = zoom_about(st.map, free, at, st.map.zoom + dz);
            }
        }
        if input_ok
            && resp.double_clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            st.map = zoom_about(st.map, free, at, (st.map.zoom + 1.0).round());
        }
        st.map = clamp_view(st.map, outer);
        let cam = Cam::new(st.map, free);

        // a click moves the place there (the map stays where it is); a right-click, or a click with the target
        // button on, puts the target there
        if input_ok
            && (resp.clicked() || resp.secondary_clicked())
            && let Some(pos) = resp.interact_pointer_pos()
        {
            let (wx, wy) = cam.world(pos);
            let (lat, lon) = map_tiles::unproject(wx.rem_euclid(1.0), wy.clamp(0.0, 1.0));
            if resp.secondary_clicked() {
                // the menu (below) offers what to do at this spot
                rt.menu_at = Some((lat, lon));
            } else if st.target_mode {
                st.target = Some([lat, lon]);
                st.target_mode = false;
            } else {
                let keep = st.map;
                set_place(st, lat, lon, String::new());
                st.map = keep;
            }
        }

        if resp.secondary_clicked() && !input_ok {
            rt.menu_at = None;
        }
        // right-click: new point of interest or plan right here, each opening its record to fill in
        {
            let mut chosen: Option<(u8, f64, f64)> = None;
            resp.context_menu(|ui| {
                let Some((lat, lon)) = rt.menu_at else {
                    ui.close();
                    return;
                };
                if ui.button(crate::i18n::tr("New POI")).clicked() {
                    chosen = Some((0, lat, lon));
                }
                if ui.button(crate::i18n::tr("New Plan")).clicked() {
                    chosen = Some((1, lat, lon));
                }
                if planner {
                    ui.separator();
                    if ui.button(crate::i18n::tr("Set the Target Here")).clicked() {
                        chosen = Some((2, lat, lon));
                    }
                }
                if chosen.is_some() {
                    ui.close();
                }
            });
            if let Some((what, lat, lon)) = chosen {
                match what {
                    0 => {
                        if let Some(i) = super::places::keep(&mut st.saved, "", lat, lon) {
                            st.map_mode = MapMode::Pois;
                            rt.site_open = Some(i);
                            rt.plan_popup = None;
                            rt.edit_of = Some(super::popup::Shown::Site(i));
                        } else {
                            self.intents.push(Intent::Full);
                        }
                    }
                    1 => {
                        self.new_plan_at(st, rt, lat, lon, None);
                        st.map_mode = MapMode::Plans;
                    }
                    _ => st.target = Some([lat, lon]),
                }
                if what < 2 {
                    st.show_horizon = false;
                    st.target_mode = false;
                }
            }
        }

        let painter = ui.painter_at(outer);
        painter.rect_filled(outer, 0.0, t.canvas);
        draw_tiles(&mut *self.tiles, &self.carto, self.satellite, st.map.zoom, &ctx, &painter, outer, cam, now, planner && st.show_light);

        // the terrain's shadows at the time shown, on the tiles and under everything else
        if planner && st.show_shadows {
            self.shade_overlay(&painter, st, rt, cam, free);
        }
        // from the place: where each body rises and sets, and where it is now
        let (wx, wy) = map_tiles::project(st.lat, st.lon);
        let origin = cam.screen(wx, wy);
        let reach = (free.width().min(free.height()) * 0.42).max(90.0);
        let at_time = c.start + self.minute * 60.0;
        if planner && st.show_twilight {
            self.twilight_tint(&painter, cam, outer, at_time);
        }
        if planner
            && st.show_eclipse
            && let Some(path) = self.eclipse_path(rt, at_time)
        {
            self.eclipse_band(&painter, cam, &path);
        }
        let curved =
            planner && (st.show_arc || st.meteor.is_some() || st.sun_style == LineStyle::Trajectory || st.moon_style == LineStyle::Trajectory);
        if curved {
            self.elevation_circles(&painter, origin, reach);
        }
        if planner && st.show_arc {
            self.milky_way_arc(&painter, origin, reach, at_time);
        }
        if let Some(shower) = st.meteor.filter(|_| planner) {
            self.meteor_layer(&painter, origin, reach, shower);
        }
        let p = &c.plan;
        // the length of a line: a fixed reach, or out to the edge of the map when the discs are drawn
        let long = if st.show_size { outer.size().max_elem() * 1.5 } else { reach };
        let sun_half = 0.2666;
        let moon_half = light_studio_astro::info::moon_size(at_time).0 / 2.0;
        type Event = (Option<f64>, Option<f64>, Color32);
        let sets: [(usize, LineStyle, f64, [Event; 2]); 3] = [
            (0, st.sun_style, sun_half, [(p.sun.rise_az, p.sun.rise, SUNRISE), (p.sun.set_az, p.sun.set, SUNSET)]),
            (1, st.moon_style, moon_half, [(p.moon.rise_az, p.moon.rise, MOONRISE), (p.moon.set_az, p.moon.set, MOONSET)]),
            (2, LineStyle::Standard, 0.4, [(p.core.rise_az, p.core.rise, pal.core), (p.core.set_az, p.core.set, pal.core)]),
        ];
        for (layer, style, half, evs) in sets.into_iter().filter(|_| planner) {
            if !st.layers[layer] {
                continue;
            }
            if style == LineStyle::Trajectory {
                let (path, color) = if layer == 0 { (&c.sun, SUN_NOW) } else { (&c.moon, MOON_NOW) };
                self.trajectory(&painter, origin, reach, path, color);
            }
            if style == LineStyle::Light {
                let (path, color) = if layer == 0 { (&c.sun, SUN_NOW) } else { (&c.moon, MOON_NOW) };
                self.light_arrows(&painter, origin, reach, path, color);
                continue;
            }
            for (az, time, color) in evs {
                if let (Some(az), Some(time)) = (az, time) {
                    if st.show_size {
                        wedge(&painter, origin, az, half, long, color);
                    }
                    if st.long_lines {
                        let line = geodesic(&cam, wx, st.lat, st.lon, az, 3000.0);
                        painter.add(Shape::line(line, Stroke::new(5.0, color.gamma_multiply(0.85))));
                    }
                    band(&painter, origin, az, reach, color, 6.0);
                    label_chip(&painter, &t, origin, az, reach, color, &clock(time, c.offset));
                }
            }
        }
        // the black pin's direction, to say when something stands in the way of a body
        let blocker = st.target.map(|[la, lo]| light_studio_astro::align::sight(st.lat, st.lon, la, lo, st.target_height));
        for (layer, color, h) in [(0, SUN_NOW, self.skies.sun), (1, MOON_NOW, self.skies.moon), (2, pal.core, self.skies.core)] {
            if !(planner && st.layers[layer] && h.alt >= 0.0) {
                continue;
            }
            // as long as the rise and set lines: along the ground to 3000 km when those are, else their reach
            let end = origin + ray_dir(h.az) * reach;
            let path = if st.long_lines { geodesic(&cam, wx, st.lat, st.lon, h.az, 3000.0) } else { vec![origin, end] };
            let hidden = blocker.is_some_and(|b| {
                let mut d = b.azimuth - h.az;
                d -= 360.0 * (d / 360.0).round();
                d.abs() < 4.0 && b.altitude > h.alt
            });
            if hidden {
                painter.extend(Shape::dashed_line(&path, Stroke::new(3.5, color), 9.0, 6.0));
            } else {
                painter.add(Shape::line(path.clone(), Stroke::new(5.0, Color32::from_black_alpha(70))));
                painter.add(Shape::line(path, Stroke::new(2.6, color)));
            }
            arrow_head(&painter, origin, h.az, reach, color);
            label_chip(&painter, &t, origin, h.az, reach * 0.8, color, &alt_text(h.alt));
        }
        if planner && st.cam_tool > 0 {
            self.camera_tool(ui, &painter, st, &cam, origin, reach);
        }
        // the place's pin: where a point of interest or a plan would be made
        painter.circle_filled(origin, 4.5, t.accent);
        painter.circle_stroke(origin, 6.5, Stroke::new(1.5, t.chrome));
        crate::icons::paint(&painter, Rect::from_center_size(pos2(origin.x, origin.y - 16.0), vec2(26.0, 26.0)), Icon::Pin, t.text);
        // the black pin and the dotted line to it (over the bodies' lines)
        if let Some([la, lo]) = st.target.filter(|_| planner) {
            let (tx, ty) = map_tiles::project(la, lo);
            let mut dx = tx - wx;
            dx -= dx.round();
            let at = cam.screen(wx + dx, ty);
            painter.add(Shape::dotted_line(&[origin, at], Color32::WHITE, 8.0, 1.8));
            black_pin(&painter, at);
        }

        // the target's shadow at the time shown (its object's height, the Sun's height and direction)
        if let Some([la, lo]) = st.target.filter(|_| planner)
            && let Some((len, az)) = light_studio_astro::align::shadow(st.target_height, self.skies.sun)
        {
            let (ela, elo) = light_studio_astro::align::destination(la, lo, az, len.min(20_000.0));
            let screen = |la: f64, lo: f64| {
                let (x, y) = map_tiles::project(la, lo);
                let mut dx = x - wx;
                dx -= dx.round();
                cam.screen(wx + dx, y)
            };
            let (a, b) = (screen(la, lo), screen(ela, elo));
            painter.line_segment([a, b], Stroke::new(6.0, Color32::from_black_alpha(150)));
        }
        // the plans (blue pins, a click opens the plan) and the points of interest (red pins, a click opens the
        // record): each its own list, shown or hidden on its own
        // (a plan at the very spot of a point of interest steps aside so both pins show)
        let poi_at: Vec<Pos2> = if mode == MapMode::Pois {
            st.saved.iter().filter_map(|p| visible(&cam, wx, p.lat, p.lon, outer, blocked)).collect()
        } else {
            Vec::new()
        };
        if mode == MapMode::Plans {
            for p in &st.plans {
                let Some(mut at) = visible(&cam, wx, p.lat, p.lon, outer, blocked) else { continue };
                if poi_at.iter().any(|q| q.distance(at) < 18.0) {
                    at.x += 22.0;
                }
                let when = format!("{} · {:02}:{:02}", p.date, (p.minute as i64) / 60, (p.minute as i64) % 60);
                let resp =
                    pin(ui, &painter, at, t.accent, Icon::Calendar, egui::Id::new(("planner-plan-pin", p.id)), format!("planner:mapPlan:{}", p.id));
                if resp.clicked() {
                    self.intents.push(Intent::OpenPlan(p.id));
                }
                if resp.hovered() {
                    resp.on_hover_ui_at_pointer(|ui| {
                        ui.label(egui::RichText::new(&p.name).font(t.semibold(12.5)).color(t.text));
                        ui.label(egui::RichText::new(when).font(t.font(11.5)).color(t.text_dim));
                        ui.label(egui::RichText::new(crate::i18n::tr("Click to open the plan")).font(t.font(11.0)).color(t.text_dim));
                    });
                }
            }
        }
        if mode == MapMode::Pois {
            for (i, p) in st.saved.iter().enumerate() {
                let Some(at) = visible(&cam, wx, p.lat, p.lon, outer, blocked) else { continue };
                let resp =
                    pin(ui, &painter, at, super::places::POI_RED, Icon::Pin, egui::Id::new(("planner-site", i)), format!("planner:mapSite:{i}"));
                if resp.clicked() {
                    self.intents.push(Intent::OpenSite(i));
                }
                if resp.hovered() {
                    resp.on_hover_ui_at_pointer(|ui| {
                        ui.label(egui::RichText::new(&p.name).font(t.semibold(12.5)).color(t.text));
                        if !p.description.trim().is_empty() {
                            let d: String = p.description.trim().chars().take(140).collect();
                            ui.label(egui::RichText::new(d).font(t.font(11.5)).color(t.text_dim));
                        }
                        ui.label(egui::RichText::new(crate::i18n::tr("Click to open the record")).font(t.font(11.0)).color(t.text_dim));
                    });
                }
            }
        }

        // the mode menu, zoom, recentre and then the icons of the mode shown, top right
        let count = match mode {
            MapMode::Pois => 10,
            MapMode::Plans => 9,
            MapMode::Planner => 15,
        };
        let bar = Rect::from_min_size(pos2(self.rail_right - layout::RAIL_W, free.top()), vec2(layout::RAIL_W, count as f32 * 34.0 + 6.0));
        let k = super::layout::card_opacity(ui.ctx(), "rail", bar, false);
        painter.rect_filled(bar.translate(vec2(0.0, 1.5)), 9.0, Color32::from_black_alpha(50).gamma_multiply(k));
        crate::theme::paint_chip_at(&painter, bar, &t, k);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(2.0, 3.0))).layout(egui::Layout::top_down(Align::Center)));
        child.spacing_mut().item_spacing.y = 0.0;
        let sz = vec2(32.0, 34.0);
        let z = st.map.zoom;
        let mode_label = MAP_MODES.iter().find(|(m, _, _)| *m == mode).map_or("", |(_, _, l)| *l);
        let mode_tip = format!("{}: {}", crate::i18n::tr("Map Mode"), crate::i18n::tr(mode_label));
        if crate::widgets::icon_button_grow(&mut child, "plannerMapMode", mode.icon(), sz, rt.mode_menu, true, &mode_tip).clicked() {
            rt.mode_menu = !rt.mode_menu;
        }
        let mode_button = Rect::from_min_size(bar.min + vec2(2.0, 3.0), sz);
        if crate::widgets::icon_button_grow(&mut child, "plannerMapIn", Icon::Plus, sz, false, z < MAX_ZOOM, "Zoom In").clicked() {
            st.map.zoom = (z + 1.0).round().min(MAX_ZOOM);
        }
        if crate::widgets::icon_button_grow(&mut child, "plannerMapOut", Icon::Minus, sz, false, z > MIN_ZOOM, "Zoom Out").clicked() {
            st.map.zoom = (z - 1.0).round().max(MIN_ZOOM);
        }
        if crate::widgets::icon_button_grow(&mut child, "plannerMapHome", Icon::Fit, sz, false, true, "Centre on the Place").clicked() {
            st.map.lat = st.lat;
            st.map.lon = st.lon;
        }
        let tip = if self.locating { "Finding your location…" } else { "My Location (approximate, from the IP address)" };
        if crate::widgets::icon_button_grow(&mut child, "plannerMapLocate", Icon::Locate, sz, self.locating, !self.locating, tip).clicked() {
            self.intents.push(Intent::LocateMe);
        }
        match mode {
            MapMode::Plans => {}
            MapMode::Pois => {
                if crate::widgets::icon_button_grow(
                    &mut child,
                    "plannerMapSavePoi",
                    Icon::Plus,
                    sz,
                    false,
                    true,
                    "Keep this Place as a Point of Interest",
                )
                .clicked()
                {
                    self.intents.push(Intent::SavePoi);
                }
            }
            MapMode::Planner => {
                if crate::widgets::icon_button_grow(
                    &mut child,
                    "plannerMapTarget",
                    Icon::Target,
                    sz,
                    st.target_mode,
                    true,
                    "Set the Target (or right-click the map)",
                )
                .clicked()
                {
                    st.target_mode = !st.target_mode;
                }
                let tip = if st.show_light { "Hide the Light Pollution" } else { "Show the Light Pollution" };
                if crate::widgets::icon_button_grow(&mut child, "plannerMapLight", Icon::Bulb, sz, st.show_light, true, tip).clicked() {
                    st.show_light = !st.show_light;
                }
                let tip = if st.show_shadows { "Hide the Shadows" } else { "Show the Shadows" };
                if crate::widgets::icon_button_grow(&mut child, "plannerMapShadows", Icon::Shade, sz, st.show_shadows, true, tip).clicked() {
                    st.show_shadows = !st.show_shadows;
                }
                let tip = if st.show_horizon { "Hide the Mountains" } else { "Show the Mountains" };
                if crate::widgets::icon_button_grow(&mut child, "plannerMapHorizon", Icon::Mountain, sz, st.show_horizon, true, tip).clicked() {
                    st.show_horizon = !st.show_horizon;
                }
                let tip = match st.cam_tool {
                    0 => "Camera Tool: off",
                    1 => "Camera Tool: field of view",
                    _ => "Camera Tool: depth of field",
                };
                if crate::widgets::icon_button_grow(&mut child, "plannerMapCamera", Icon::Crop, sz, st.cam_tool > 0, true, tip).clicked() {
                    st.cam_tool = (st.cam_tool + 1) % 3;
                }
                if crate::widgets::icon_button_grow(&mut child, "plannerMapLayers", Icon::Sliders, sz, rt.layers_open, true, "Map Layers").clicked() {
                    rt.layers_open = !rt.layers_open;
                }
            }
        }
        let tip = if self.satellite { "Plain Map" } else { "Satellite" };
        if crate::widgets::icon_button_grow(&mut child, "plannerMapSatellite", Icon::Satellite, sz, self.satellite, true, tip).clicked() {
            self.intents.push(Intent::ToggleSatellite);
        }
        let tip = if self.syncing {
            "Looking for points of interest…"
        } else if self.sync_state == Some(true) {
            "Sync Points of Interest: in step with the phone"
        } else if self.sync_state == Some(false) {
            "Sync Points of Interest: waiting to be sent"
        } else {
            "Sync Points of Interest"
        };
        let tint = match self.sync_state {
            Some(true) => Some(Color32::from_rgb(52, 199, 89)),
            Some(false) => Some(Color32::from_rgb(255, 69, 58)),
            None => None,
        };
        if crate::widgets::icon_button_grow_tint(&mut child, "plannerMapSync", Icon::Cloud, sz, self.syncing, !self.syncing, tip, tint).clicked() {
            self.intents.push(Intent::SyncPois);
        }
        if crate::widgets::icon_button_grow(&mut child, "plannerSettings", Icon::Gear, sz, false, true, "Settings").clicked() {
            self.intents.push(Intent::OpenSettings);
        }
        let tip = if st.panel_hidden { "Show the Panel" } else { "Hide the Panel" };
        if crate::widgets::icon_button_grow(&mut child, "plannerPanel", Icon::Sidebar, sz, !st.panel_hidden, true, tip).clicked() {
            st.panel_hidden = !st.panel_hidden;
        }
        if rt.layers_open && planner {
            self.layers_panel(ui, st, rt, bar, free);
        }
        if st.strip && planner {
            self.info_strip(ui, st, rt, free);
        }
        // last, so the list lies over the info strip and the other cards (its first row was hidden under the strip)
        if rt.mode_menu {
            self.mode_menu(ui, st, rt, mode_button);
        }

        // while placing the target, a line says what the next click does (otherwise the map has no legend)
        if planner && st.target_mode {
            let hint = painter.layout_no_wrap(crate::i18n::tr("Click the map to put the target there.").to_string(), t.font(11.0), t.text);
            let hr = Rect::from_min_size(pos2(free.left(), free.bottom() - 24.0), hint.size() + vec2(20.0, 24.0 - hint.size().y));
            crate::theme::paint_chip(&painter, hr, &t);
            painter.galley(pos2(hr.left() + 10.0, hr.center().y - hint.size().y / 2.0), hint, t.text);
        }
        if planner && st.show_light {
            let a = painter.layout_no_wrap(map_tiles::LIGHT_POLLUTION_ATTRIBUTION.to_string(), t.font(9.5), t.text_dim);
            let ar = Rect::from_min_size(pos2(free.left(), free.top() + 44.0), a.size() + vec2(8.0, 4.0));
            painter.rect_filled(ar, 4.0, t.chrome.gamma_multiply(0.7));
            painter.galley(ar.min + vec2(4.0, 2.0), a, t.text_dim);
        }
    }
}

impl Page<'_> {
    /// The mode menu: a list under the rail's first icon (Points of Interest, Plans, Planner).
    fn mode_menu(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, button: Rect) {
        let t = self.t;
        let (w, row_h) = (210.0, 38.0);
        let r = Rect::from_min_size(pos2(button.left() - w - 8.0, button.top()), vec2(w, row_h * MAP_MODES.len() as f32 + 8.0));
        super::layout::surface(ui.painter(), r, &t);
        let inside = ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| r.contains(p) || button.contains(p));
        let mut picked = None;
        for (k, (m, id, label)) in MAP_MODES.iter().enumerate() {
            let cell = Rect::from_min_size(r.min + vec2(4.0, 4.0 + row_h * k as f32), vec2(w - 8.0, row_h));
            let resp = ui.interact(cell, egui::Id::new(("planner-map-mode", *id)), Sense::click());
            let p = ui.painter();
            if *m == st.map_mode {
                p.rect_filled(cell, R_CONTROL, t.tool_active);
            } else if resp.hovered() {
                p.rect_filled(cell, R_CONTROL, t.hover.gamma_multiply(0.6));
            }
            crate::icons::paint(p, Rect::from_center_size(pos2(cell.left() + 20.0, cell.center().y), vec2(18.0, 18.0)), m.icon(), t.text);
            p.text(pos2(cell.left() + 40.0, cell.center().y), Align2::LEFT_CENTER, crate::i18n::tr(label), t.font(13.0), t.text);
            if resp.clicked() {
                picked = Some(*m);
            }
        }
        if let Some(m) = picked {
            if m != st.map_mode {
                // each mode shows only its own things: the planner's mountains and target go away with it
                st.show_horizon = false;
                st.target_mode = false;
                st.map_mode = m;
                // (the side panel is never opened for you: it stays as it was)
            }
            rt.mode_menu = false;
        } else if !inside && ui.input(|i| i.pointer.any_pressed()) {
            rt.mode_menu = false;
        }
    }

    /// The map's layers: each with an eye, the Sun's and the Moon's with a style that a click steps through.
    fn layers_panel(&mut self, ui: &mut Ui, st: &mut PlannerState, rt: &mut Runtime, bar: Rect, free: Rect) {
        let t = self.t;
        let w = 232.0;
        let h = 460.0_f32.min(free.height() - 66.0).max(120.0);
        let r = Rect::from_min_size(pos2(bar.left() - w - 8.0, free.top() + if st.strip { 66.0 } else { 0.0 }), vec2(w, h));
        let painter = ui.painter();
        painter.rect_filled(r.translate(vec2(0.0, 1.5)), 12.0, Color32::from_black_alpha(50));
        crate::theme::paint_chip(painter, r, &t);
        let _ = ui.interact(r, egui::Id::new("planner-layers-bg"), Sense::click_and_drag());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(12.0, 8.0))).layout(egui::Layout::top_down(Align::Min)));
        child.spacing_mut().item_spacing.y = 0.0;
        let ui = &mut child;
        ui.label(egui::RichText::new(crate::i18n::tr("Map Layers")).font(t.semibold(12.5)).color(t.text));
        ui.add_space(4.0);
        let eye = |ui: &mut Ui, id: &str, label: &str, on: &mut bool, detail: Option<&str>| -> bool {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
            let cell = Rect::from_min_size(row.min, vec2(row.width(), 30.0));
            let resp = ui.interact(cell, egui::Id::new(("planner-layer", id.to_string())), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(cell, 6.0, t.hover.gamma_multiply(0.5));
            }
            let icon = if *on { Icon::Eye } else { Icon::EyeOff };
            crate::icons::paint(
                ui.painter(),
                Rect::from_center_size(pos2(cell.left() + 12.0, cell.center().y), vec2(16.0, 16.0)),
                icon,
                if *on { t.accent } else { t.text_disabled },
            );
            ui.painter().text(
                pos2(cell.left() + 28.0, cell.center().y),
                Align2::LEFT_CENTER,
                crate::i18n::tr(label),
                t.font(12.5),
                if *on { t.text } else { t.text_dim },
            );
            if let Some(d) = detail {
                ui.painter().text(pos2(cell.right() - 6.0, cell.center().y), Align2::RIGHT_CENTER, crate::i18n::tr(d), t.font(11.0), t.text_dim);
            }
            if resp.clicked() {
                *on = !*on;
            }
            resp.clicked()
        };
        let mut sun = st.layers[0];
        eye(ui, "sun", "Sun", &mut sun, None);
        st.layers[0] = sun;
        style_row(ui, &t, "sunStyle", &mut st.sun_style);
        let mut moon = st.layers[1];
        eye(ui, "moon", "Moon", &mut moon, None);
        st.layers[1] = moon;
        style_row(ui, &t, "moonStyle", &mut st.moon_style);
        let mut core = st.layers[2];
        eye(ui, "core", "Milky Way core", &mut core, None);
        st.layers[2] = core;
        eye(ui, "arc", "Milky Way arc", &mut st.show_arc, None);
        eye(ui, "twilight", "Twilights", &mut st.show_twilight, None);
        eye(ui, "eclipse", "Solar eclipse track", &mut st.show_eclipse, None);
        eye(ui, "size", "Sun and Moon size", &mut st.show_size, None);
        eye(ui, "long", "True azimuth lines", &mut st.long_lines, None);
        eye(ui, "portrait", "Camera held upright", &mut st.portrait, None);
        let mut on = st.meteor.is_some();
        let name = st.meteor.and_then(|m| SHOWERS.get(m)).map(|s| s.name);
        if eye(ui, "meteor", "Meteor shower", &mut on, name) {
            st.meteor = on.then_some(st.shower);
        }
        if st.meteor.is_some() {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
            let resp = ui.interact(row, egui::Id::new("planner-layer-shower"), Sense::click());
            ui.painter().text(pos2(row.left() + 28.0, row.center().y), Align2::LEFT_CENTER, crate::i18n::tr("Next shower"), t.font(11.5), t.accent);
            if resp.clicked() {
                st.shower = (st.shower + 1) % SHOWERS.len();
                st.meteor = Some(st.shower);
            }
        }
        ui.add_space(4.0);
        if text_button(ui, "plannerLayersReset", "Reset Layers", false).clicked() {
            st.layers = [true; 3];
            st.sun_style = LineStyle::Standard;
            st.moon_style = LineStyle::Standard;
            st.show_arc = true;
            st.show_twilight = false;
            st.show_eclipse = false;
            st.show_size = false;
            st.long_lines = false;
            st.meteor = None;
        }
        let _ = rt;
    }
}

/// The line under a body in the layers panel that names its style; a click steps to the next.
fn style_row(ui: &mut Ui, t: &Tokens, id: &str, style: &mut LineStyle) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    let resp = ui.interact(row, egui::Id::new(("planner-style", id.to_string())), Sense::click());
    ui.painter().text(pos2(row.left() + 28.0, row.center().y), Align2::LEFT_CENTER, crate::i18n::tr(style.label()), t.font(11.5), t.accent);
    if resp.clicked() {
        *style = style.next();
    }
}

/// Where a position is on screen (the copy of the world nearest the place), if it is in view and not under a panel.
fn visible(cam: &Cam, place_x: f64, lat: f64, lon: f64, outer: Rect, blocked: &[Rect]) -> Option<Pos2> {
    if !(lat.is_finite() && lon.is_finite()) {
        return None;
    }
    let (x, y) = map_tiles::project(lat, lon);
    let mut dx = x - place_x;
    dx -= dx.round();
    let at = cam.screen(place_x + dx, y);
    (outer.expand(20.0).contains(at) && !blocked.iter().any(|r| r.contains(at))).then_some(at)
}

/// A map pin: a drop in `color` with an icon on its head, its tip at `at`.
fn pin(ui: &mut Ui, painter: &egui::Painter, at: Pos2, color: Color32, icon: Icon, id: egui::Id, _name: String) -> egui::Response {
    let head = Rect::from_center_size(pos2(at.x, at.y - 17.0), vec2(26.0, 26.0));
    let resp = ui.interact(head.union(Rect::from_center_size(at, vec2(12.0, 2.0))), id, Sense::click());
    let lift = if resp.hovered() { 2.0 } else { 0.0 };
    let head = head.translate(vec2(0.0, -lift));
    painter.circle_filled(head.center() + vec2(0.0, 1.5 + lift), 13.5, Color32::from_black_alpha(80));
    painter.add(Shape::convex_polygon(vec![at, pos2(at.x + 6.0, head.bottom() - 4.0), pos2(at.x - 6.0, head.bottom() - 4.0)], color, Stroke::NONE));
    painter.circle_filled(head.center(), 13.0, color);
    painter.circle_stroke(head.center(), 13.0, Stroke::new(1.5, Color32::WHITE));
    crate::icons::paint(painter, head.shrink(6.5), icon, Color32::WHITE);
    resp
}

/// The direction of an azimuth on the screen (north up).
fn ray_dir(az: f64) -> egui::Vec2 {
    let a = az.to_radians();
    vec2(a.sin() as f32, -a.cos() as f32)
}

/// A small label at the end of a line along an azimuth.
fn label_chip(p: &egui::Painter, t: &Tokens, origin: Pos2, az: f64, length: f32, color: Color32, label: &str) {
    let dir = ray_dir(az);
    let end = origin + dir * length;
    let g = p.layout_no_wrap(label.to_string(), t.semibold(11.0), t.text);
    let chip = Rect::from_center_size(end + dir * (14.0 + g.size().x / 2.0 * dir.x.abs()) + vec2(0.0, dir.y * 6.0), g.size() + vec2(12.0, 6.0));
    p.rect_filled(chip, 9.0, crate::theme::chip_fill(t));
    p.rect_stroke(chip, 9.0, Stroke::new(1.4, color), StrokeKind::Inside);
    p.galley(chip.center() - g.size() / 2.0, g, t.text);
}

/// The black pin: a black drop with a white rim, its tip at `at`.
fn black_pin(p: &egui::Painter, at: Pos2) {
    let head = at - vec2(0.0, 19.0);
    p.circle_filled(head + vec2(0.0, 2.0), 10.0, Color32::from_black_alpha(80));
    p.add(Shape::convex_polygon(vec![at, pos2(at.x + 6.0, head.y + 6.0), pos2(at.x - 6.0, head.y + 6.0)], Color32::BLACK, Stroke::NONE));
    p.circle_filled(head, 10.0, Color32::BLACK);
    p.circle_stroke(head, 10.0, Stroke::new(1.6, Color32::WHITE));
    p.circle_filled(head, 3.0, Color32::WHITE);
}

/// A filled arrow head at the end of a line of `length` along an azimuth.
fn arrow_head(p: &egui::Painter, origin: Pos2, az: f64, length: f32, color: Color32) {
    let a = az.to_radians();
    let dir = vec2(a.sin() as f32, -a.cos() as f32);
    let side = vec2(-dir.y, dir.x);
    let tip = origin + dir * length;
    let base = tip - dir * 14.0;
    p.add(Shape::convex_polygon(vec![tip + dir * 3.0, base + side * 6.0, base - side * 6.0], color, Stroke::NONE));
}
