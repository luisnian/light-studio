//! The one-page sheet of a plan: a single self-contained HTML file (no network, no scripts needed) with the time, the day's
//! light, the framing, the target, the notes and the notebook of the place, to read offline or print (or save as PDF) from the
//! browser. It uses what the planner already works out; nothing is looked up.

use light_studio_astro::Span;

use super::notebook::Notebook;
use super::plans::{distance_label, sight_of};
use super::*;

/// Escapes text for HTML.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Text with its line breaks kept.
fn block(s: &str) -> String {
    esc(s.trim()).replace('\n', "<br>")
}

fn section(out: &mut String, title: &str, body: &str) {
    if body.trim().is_empty() {
        return;
    }
    out.push_str(&format!("<h2>{}</h2>\n{body}\n", esc(title)));
}

fn rows(items: &[(String, String)]) -> String {
    let mut t = String::from("<table>\n");
    for (k, v) in items {
        t.push_str(&format!("<tr><th>{}</th><td>{}</td></tr>\n", esc(k), esc(v)));
    }
    t.push_str("</table>");
    t
}

/// The notebook of the saved place the plan is for, if there is one.
fn notebook_of<'a>(plan: &Plan, base: &'a PlannerState) -> Option<(&'a str, &'a Notebook)> {
    base.saved
        .iter()
        .find(|s| (s.lat - plan.lat).abs() <= super::notebook::NEAR_DEGREES && (s.lon - plan.lon).abs() <= super::notebook::NEAR_DEGREES)
        .map(|s| (s.name.as_str(), &s.notebook))
}

/// The sheet for a plan, as HTML.
pub(super) fn html(plan: &Plan, base: &PlannerState, now: f64) -> String {
    let tr = |s: &str| crate::i18n::tr(s).to_string();
    let st = plan.state(base);
    let c = compute(&st, now);
    let start = c.start + plan.minute * 60.0;
    let p = &c.plan;
    let at = |t: Option<f64>| t.map_or_else(|| "—".to_string(), |t| clock(t, c.offset));
    let span = |s: Option<Span>| s.map_or_else(|| "—".to_string(), |s| format!("{}–{}", clock(s.start, c.offset), clock(s.end, c.offset)));
    let name = if plan.name.trim().is_empty() { tr("Photo plan") } else { plan.name.trim().to_string() };
    let place = if plan.place.trim().is_empty() { format!("{:.5}, {:.5}", plan.lat, plan.lon) } else { plan.place.trim().to_string() };

    let mut body = String::new();
    body.push_str(&format!(
        "<h1>{}</h1>\n<p class=\"when\">{} · {}</p>\n<p class=\"where\">{} · {:.5}, {:.5}</p>\n",
        esc(&name),
        esc(&short_date(c.date)),
        esc(&clock(start, c.offset)),
        esc(&place),
        plan.lat,
        plan.lon
    ));
    body.push_str(&format!("<p class=\"small\">{}</p>\n", esc(&format!("{} {}", tr("Times are local to the place,"), utc_label(c.offset)))));

    let phase = moon_phase(start);
    let mut light = vec![
        (tr("Sunrise"), at(p.sun.rise)),
        (tr("Sunset"), at(p.sun.set)),
        (tr("Golden hour"), format!("{} / {}", span(p.sun.golden_am), span(p.sun.golden_pm))),
        (tr("Blue hour"), format!("{} / {}", span(p.sun.blue_am), span(p.sun.blue_pm))),
        (tr("Moon"), format!("{} ({:.0} %)", tr(phase.name.label()), phase.illumination * 100.0)),
        (tr("Moonrise"), at(p.moon.rise)),
        (tr("Moonset"), at(p.moon.set)),
    ];
    if let Some(w) = c.night_core.first() {
        light.push((tr("Milky Way core"), format!("{}–{}", clock(w.start, c.offset), clock(w.end, c.offset))));
    }
    section(&mut body, &tr("The light that day"), &rows(&light));

    let mut shot = Vec::new();
    if let Some(f) = &plan.frame {
        let sensor = SENSORS.get(f.sensor).map_or("", |s| s.name);
        shot.push((tr("Direction"), format!("{:.0}°", f.az)));
        shot.push((tr("Tilt"), format!("{:.0}°", f.alt)));
        shot.push((tr("Lens"), format!("{:.0} mm · {} · {sensor}", f.focal, tr(if f.portrait { "portrait" } else { "landscape" }))));
    }
    if let Some(s) = sight_of(&st, None) {
        shot.push((tr("Target"), format!("{} · {}", distance_label(s.distance), az_text(s.azimuth))));
    }
    section(&mut body, &tr("Framing"), &if shot.is_empty() { String::new() } else { rows(&shot) });
    section(&mut body, &tr("Notes"), &if plan.notes.trim().is_empty() { String::new() } else { format!("<p>{}</p>", block(&plan.notes)) });

    if let Some((site, nb)) = notebook_of(plan, base) {
        let mut text = String::new();
        for (label, value) in [(tr("Access"), &nb.access), (tr("Gear"), &nb.gear), (tr("Conditions"), &nb.conditions)] {
            if !value.trim().is_empty() {
                text.push_str(&format!("<h3>{}</h3>\n<p>{}</p>\n", esc(&label), block(value)));
            }
        }
        let pending: Vec<_> = nb.shots.iter().filter(|s| !s.done).collect();
        if !pending.is_empty() {
            text.push_str(&format!("<h3>{}</h3>\n<ul>\n", esc(&tr("Shots to take"))));
            for s in pending {
                let idea = if s.idea.trim().is_empty() { String::new() } else { format!(" — {}", esc(s.idea.trim())) };
                text.push_str(&format!("<li>{}{idea}</li>\n", esc(s.title.trim())));
            }
            text.push_str("</ul>\n");
        }
        let refs = nb.reference_list();
        if !refs.is_empty() {
            text.push_str(&format!("<h3>{}</h3>\n<ul>\n", esc(&tr("Reference photos"))));
            for r in refs {
                text.push_str(&format!("<li>{}</li>\n", esc(r)));
            }
            text.push_str("</ul>\n");
        }
        section(&mut body, &format!("{} · {}", tr("Notebook"), site), &text);
    }

    format!(
        "<!doctype html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{title}</title>\n<style>{CSS}</style>\n</head>\n<body>\n<button class=\"noprint\" onclick=\"window.print()\">{print}</button>\n{body}<p class=\"small foot\">Light Studio · {made}</p>\n</body>\n</html>\n",
        lang = if crate::i18n::tr("Sunrise") == "Sunrise" { "en" } else { "es" },
        title = esc(&name),
        print = esc(&tr("Print / Save as PDF")),
        made = esc(&short_date(Date::from_unix(now, c.offset))),
    )
}

const CSS: &str = "
@page { size: A4; margin: 14mm; }
body { font: 14px/1.45 -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif; color: #1a1a1a; max-width: 720px; margin: 24px auto; padding: 0 16px; }
h1 { font-size: 24px; margin: 0 0 4px; }
h2 { font-size: 15px; margin: 18px 0 6px; padding-bottom: 3px; border-bottom: 1px solid #bbb; text-transform: uppercase; letter-spacing: .04em; }
h3 { font-size: 13px; margin: 10px 0 2px; }
p { margin: 4px 0; }
.when { font-size: 18px; font-weight: 600; } .where { color: #444; }
.small { font-size: 12px; color: #666; } .foot { margin-top: 24px; }
table { border-collapse: collapse; width: 100%; } th { text-align: left; font-weight: 500; color: #444; width: 36%; padding: 2px 8px 2px 0; vertical-align: top; } td { padding: 2px 0; }
ul { margin: 2px 0 2px 18px; padding: 0; }
button { font: inherit; padding: 6px 12px; float: right; }
@media print { .noprint { display: none; } body { margin: 0; max-width: none; } h2, table, ul { break-inside: avoid; } }
";

/// A file name for the sheet.
fn file_name(plan: &Plan) -> String {
    super::plans::ics_file_name(plan).replace(".ics", ".html")
}

/// Opens a file with the system's default app.
fn open_file(path: &str) -> Result<(), String> {
    use std::process::Command;
    let mut cmd = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else {
        Command::new("xdg-open")
    };
    cmd.arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Writes the plan's sheet and opens it.
pub(super) fn open_sheet(app: &mut App, ctx: &egui::Context, id: u64) {
    let Some(plan) = app.ui.planner.plans.iter().find(|p| p.id == id).cloned() else { return };
    let text = html(&plan, &app.ui.planner, now_unix());
    let Some(dir) = crate::paths::config_dir().map(|d| d.join("plans")) else {
        app.toast(ctx, crate::i18n::tr("There is nowhere to write the sheet").to_string());
        return;
    };
    let path = dir.join(file_name(&plan));
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, text)) {
        app.toast(ctx, format!("{}: {e}", path.display()));
        return;
    }
    let shown = path.to_string_lossy().to_string();
    match open_file(&shown) {
        Ok(()) => app.toast(ctx, crate::i18n::tr("Sheet opened: print it or save it as PDF from the browser").to_string()),
        Err(e) => app.toast(ctx, crate::i18n::tr("Sheet saved: {path}").replace("{path}", &format!("{shown} ({e})"))),
    }
}

#[cfg(test)]
mod tests {
    use super::super::notebook::Shot;
    use super::super::viewfinder::FrameShot;
    use super::*;

    fn plan() -> Plan {
        Plan {
            id: 9,
            name: "Faro <b>&\"x\"</b>".into(),
            lat: 43.39,
            lon: -1.79,
            place: "Hondarribia".into(),
            date: "2026-10-18".into(),
            minute: 19.0 * 60.0 + 5.0,
            utc_offset: Some(120),
            notes: "Trípode\n<script>alert(1)</script>".into(),
            frame: Some(FrameShot { az: 285.0, alt: 3.0, focal: 24.0, portrait: false, sensor: 6 }),
            ..Plan::default()
        }
    }

    #[test]
    fn the_sheet_has_the_plan_and_escapes_everything() {
        let h = html(&plan(), &PlannerState::default(), 1_760_000_000.0);
        assert!(h.starts_with("<!doctype html>") && h.contains("Hondarribia") && h.contains("19:05") && h.contains("285°") && h.contains("24 mm"));
        assert!(h.contains("UTC+02:00"));
        assert!(!h.contains("<script>alert") && !h.contains("<b>"), "user text never becomes markup");
        assert!(h.contains("&lt;script&gt;") && h.contains("&amp;"));
        // no network at all: nothing to fetch
        assert!(!h.contains("src=") && !h.contains("http://") && !h.contains("stylesheet"));
        assert_eq!(file_name(&plan()), "Faro-b-x-b-2026-10-18-9.html");
    }

    #[test]
    fn the_notebook_of_the_place_goes_on_the_sheet_only_when_there_is_one() {
        let mut base = PlannerState::default();
        assert!(!html(&plan(), &base, 1_760_000_000.0).contains("Notebook"));
        let nb = Notebook {
            access: "Aparcar en el faro".into(),
            gear: "Trípode <i>y</i> filtros".into(),
            references: "https://example.org/a.jpg\n\nfoto2".into(),
            shots: vec![
                Shot { title: "Ola larga".into(), idea: "30 s".into(), ..Default::default() },
                Shot { title: "Hecha".into(), done: true, ..Default::default() },
            ],
            ..Default::default()
        };
        base.saved.push(SavedPlace { name: "Faro".into(), lat: 43.3901, lon: -1.7899, notebook: nb, ..Default::default() });
        let h = html(&plan(), &base, 1_760_000_000.0);
        assert!(h.contains("Aparcar en el faro") && h.contains("Ola larga") && h.contains("foto2"));
        assert!(!h.contains("Hecha"), "shots already taken stay off the sheet");
        assert!(h.contains("&lt;i&gt;y&lt;/i&gt;"));
        // another place's notebook is not this plan's
        base.saved[0].lat = 10.0;
        assert!(!html(&plan(), &base, 1_760_000_000.0).contains("Aparcar"));
    }

    #[test]
    fn hostile_plans_do_not_panic() {
        let h = html(
            &Plan { lat: f64::NAN, lon: f64::INFINITY, minute: f64::NAN, date: "x".into(), ..Plan::default() },
            &PlannerState::default(),
            f64::NAN,
        );
        assert!(h.contains("</html>"));
    }
}
