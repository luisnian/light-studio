//! The settings window: language, the map, and the link with the phone's points of interest (the Worker, its key, the
//! codes that link phones, and the files points are exported to and imported from).

use egui::{Align2, Color32, Context, RichText, Ui, Window, vec2};

use crate::App;
use crate::i18n::{self, Language, tr};
use crate::pdi_pair::Job;
use crate::services::pdi_sync::{self, PdiConfig};
use crate::text_field::TextField;
use crate::theme::Tokens;

pub fn show(app: &mut App, ctx: &Context) {
    let mut open = true;
    let t = Tokens::get(ctx);
    Window::new(tr("Settings"))
        .open(&mut open)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .collapsible(false)
        .resizable([true, true])
        .default_size(vec2(640.0, 620.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                general(app, ui, &t);
                phone(app, ui, &t);
                link_phone(app, ui, &t);
                phone_app(app, ui, &t);
                files(app, ui, &t);
            });
        });
    if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input() {
        app.settings_open = false;
    }
}

fn heading(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(tr(text)).font(t.semibold(15.0)).color(t.text));
    ui.add_space(2.0);
    ui.separator();
}

fn hint(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).font(t.font(11.5)).color(t.text_dim)).wrap());
}

fn row(ui: &mut Ui, t: &Tokens, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(150.0, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            if !label.is_empty() {
                ui.label(RichText::new(tr(label)).font(t.font(12.5)).color(t.text_dim));
            }
        });
        add(ui);
    });
}

fn general(app: &mut App, ui: &mut Ui, t: &Tokens) {
    heading(ui, t, "General");
    row(ui, t, "Language", |ui| {
        let current = i18n::language();
        for language in Language::ALL {
            if ui.selectable_label(current == language, language.name()).clicked() {
                i18n::set_language(language);
                app.ui.settings.language = language.code().to_string();
            }
        }
    });
    row(ui, t, "CARTO key", |ui| {
        TextField::singleline("field:cartoKey", &mut app.ui.map_carto_key).hint(tr("Optional: a key for CARTO's plain map")).width(260.0).show(ui);
    });
    hint(ui, t, tr("Without a key the plain map comes from OpenStreetMap. A free key is given at carto.com/basemaps/apikey."));
}

fn phone(app: &mut App, ui: &mut Ui, t: &Tokens) {
    heading(ui, t, "Points of Interest from the Phone");
    ui.checkbox(&mut app.ui.settings.pdi.enabled, tr("Look for new points of interest from the phone when Light Studio opens and every few minutes"));
    row(ui, t, "Worker", |ui| {
        TextField::singleline("field:pdiUrl", &mut app.ui.settings.pdi.url).hint("https://pdi.example.com").width(260.0).show(ui);
    });
    row(ui, t, "Existing key", |ui| {
        let mut key = std::mem::take(&mut app.pdi_key_draft);
        let r = TextField::singleline("field:pdiKey", &mut key).hint(tr("Paste the Worker's key")).width(260.0).show(ui);
        let can = !key.trim().is_empty() && pdi_sync::SUPPORTED;
        let save = ui.add_enabled(can, egui::Button::new(tr("Save Key")));
        if save.clicked() || (r.committed() && can) {
            match crate::pdi::save_key(&key) {
                Ok(()) => {
                    key.clear();
                    app.pdi.last = Some(tr("Key saved in the keychain").to_string());
                }
                Err(e) => app.pdi.last = Some(e),
            }
        }
        app.pdi_key_draft = key;
    });
    row(ui, t, "", |ui| {
        if ui.add_enabled(!app.pdi.running(), egui::Button::new(tr("Sync Now"))).clicked()
            && let Err(e) = crate::pdi::start(app, true)
        {
            app.pdi.last = Some(e);
        }
        if let Some(line) = &app.pdi.last {
            hint(ui, t, line);
        }
    });
    row(ui, t, "", |ui| {
        if ui.add_enabled(pdi_sync::SUPPORTED, egui::Button::new(tr("Generate a New Key"))).clicked() {
            match pdi_sync::generate_key().and_then(|k| crate::pdi::save_key(&k).map(|()| k)) {
                Ok(k) => {
                    app.pdi_new_key = Some(k);
                    app.pdi.last = Some(tr("New key saved in the keychain").to_string());
                }
                Err(e) => app.pdi.last = Some(e),
            }
        }
    });
    if !pdi_sync::SUPPORTED {
        hint(ui, t, tr("The key can only be kept on the Mac or on Windows. On other systems, set it with the LIGHT_STUDIO_PDI_KEY variable."));
    }
    if let Some(k) = app.pdi_new_key.clone() {
        row(ui, t, "New key", |ui| {
            // read-only but selectable, so the whole key can be copied by hand
            let mut shown = k.as_str();
            let r = ui.add(
                egui::TextEdit::singleline(&mut shown)
                    .font(egui::TextStyle::Monospace)
                    .text_color(t.text)
                    .desired_width((ui.available_width() - 8.0).clamp(320.0, 620.0)),
            );
            if r.gained_focus() {
                let mut state = egui::TextEdit::load_state(ui.ctx(), r.id).unwrap_or_default();
                let end = k.chars().count();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(end))));
                state.store(ui.ctx(), r.id);
            }
        });
        row(ui, t, "", |ui| {
            if ui.button(tr("Copy Key")).clicked() {
                ui.ctx().copy_text(k.clone());
                app.pdi.last = Some(tr("Key copied").to_string());
            }
            if ui.button(tr("Hide the Key")).clicked() {
                app.pdi_new_key = None;
            }
        });
        hint(
            ui,
            t,
            tr(
                "This is the main key, shown only now. Copy it and set it in the Worker (wrangler secret put API_KEY); this computer already has it in the keychain. Phones are not given this key: they are linked with a code (below).",
            ),
        );
    }
}

fn qr_code(ui: &mut Ui, text: &str) {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else { return };
    let n = code.width();
    let dark: Vec<bool> = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    let side = 168.0;
    let quiet = 2usize;
    let cell = side / (n + 2 * quiet) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
    ui.painter().rect_filled(rect, 6.0, Color32::WHITE);
    for (i, d) in dark.iter().enumerate() {
        if *d {
            let (x, y) = ((i % n + quiet) as f32, (i / n + quiet) as f32);
            let m = egui::Rect::from_min_size(rect.min + vec2(x * cell, y * cell), vec2(cell, cell));
            ui.painter().rect_filled(m, 0.0, Color32::BLACK);
        }
    }
}

fn link_phone(app: &mut App, ui: &mut Ui, t: &Tokens) {
    heading(ui, t, "Link a phone");
    let now = ui.input(|i| i.time);
    row(ui, t, "", |ui| {
        let can = !app.pdi_pair.busy() && pdi_sync::SUPPORTED;
        if ui.add_enabled(can, egui::Button::new(tr("Link a Phone"))).clicked() {
            crate::pdi_pair::start(app, Job::Code);
        }
        if ui.add_enabled(can, egui::Button::new(tr("Refresh the List"))).clicked() {
            crate::pdi_pair::start(app, Job::Devices);
        }
    });
    if let Some(left) = app.pdi_pair.left(now) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        if let Some((code, _)) = app.pdi_pair.code.clone() {
            row(ui, t, "Code", |ui| {
                ui.label(RichText::new(&code.code).monospace().size(26.0).strong().color(t.text));
                let rest =
                    tr("valid for {m}:{s}").replace("{m}", &format!("{}", left as u64 / 60)).replace("{s}", &format!("{:02}", left as u64 % 60));
                ui.label(RichText::new(rest).color(t.text_dim));
            });
            row(ui, t, "", |ui| {
                if ui.button(tr("Copy Code")).clicked() {
                    ui.ctx().copy_text(code.code.clone());
                }
                if ui.button(tr("Copy Link")).clicked() {
                    ui.ctx().copy_text(code.link.clone());
                }
            });
            qr_code(ui, &code.link);
            hint(
                ui,
                t,
                tr(
                    "Scan this QR with the phone camera: it opens Safari and links it with no typing. The link does not reach the app installed on the home screen: there, open the app → Settings → Linking code, type this code and tap Link this phone.",
                ),
            );
        }
    }
    if let Some(m) = app.pdi_pair.message.clone() {
        hint(ui, t, &m);
    }
    if let Some(list) = app.pdi_pair.devices.clone() {
        if list.is_empty() {
            hint(ui, t, tr("No phone is linked yet."));
        }
        for d in list {
            row(ui, t, &d.name, |ui| {
                let used =
                    d.last_used.as_deref().map_or_else(|| tr("never used").to_string(), |u| u.chars().take(16).collect::<String>().replace('T', " "));
                ui.label(RichText::new(used).color(t.text_dim));
                if d.revoked {
                    ui.label(RichText::new(tr("revoked")).color(t.text_dim));
                } else if ui.add_enabled(!app.pdi_pair.busy(), egui::Button::new(tr("Revoke"))).clicked() {
                    crate::pdi_pair::start(app, Job::Revoke(d.id.clone()));
                }
            });
        }
    }
}

/// The phone's web app: its address (to copy or open), a QR code to open it on the phone, and how to install and use it.
fn phone_app(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let url = app.ui.settings.pdi.url.trim().trim_end_matches('/').to_string();
    let url = if url.is_empty() { PdiConfig::default().url } else { url };
    heading(ui, t, "The phone app");
    row(ui, t, "Address", |ui| {
        ui.label(RichText::new(&url).monospace().color(t.text));
        if ui.button(tr("Copy Address")).clicked() {
            ui.ctx().copy_text(url.clone());
        }
        if ui.button(tr("Open")).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url.clone()));
        }
    });
    qr_code(ui, &url);
    hint(ui, t, tr("Point the iPhone's camera at the code to open the address."));
    hint(ui, t, tr("Add it to the iPhone's home screen:"));
    for line in [
        "1. Open the address in Safari (it must be Safari).",
        "2. Tap the Share button (the square with an arrow).",
        "3. Choose “Add to Home Screen” and tap Add. It opens like an app, full screen.",
        "4. The first time, open Settings in the app, type the linking code and tap Link this phone (see above).",
    ] {
        hint(ui, t, tr(line));
    }
    hint(ui, t, tr("How it is used:"));
    for line in [
        "• In the field, tap + in the app, name the place and save: the position is taken from the phone, and you can add notes and photos.",
        "• Light Studio picks it up when it opens and every few minutes (or press Sync Now, or the cloud button on the map).",
        "• It goes both ways: points made or edited here appear on the phone, and deleting on one side deletes on the other.",
        "• If both sides change the same point, the phone's copy is kept and yours is saved as “(conflict)”.",
        "• The key stays in the Mac's keychain (on Windows, encrypted for your user), never in a plain file. To change it, generate a new one and set it in the Worker and the phone.",
    ] {
        hint(ui, t, tr(line));
    }
}

fn files(app: &mut App, ui: &mut Ui, t: &Tokens) {
    heading(ui, t, "Points of Interest File");
    row(ui, t, "", |ui| {
        if ui.button(tr("Export…")).clicked()
            && let Some(path) = rfd::FileDialog::new().set_file_name("points-of-interest.json").add_filter("JSON", &["json"]).save_file()
        {
            let ctx = ui.ctx().clone();
            match crate::planner::export_pois(app, &path.to_string_lossy()) {
                Ok(n) => app.toast(&ctx, tr("{n} points of interest exported").replace("{n}", &n.to_string())),
                Err(e) => app.toast_error(&ctx, e),
            }
        }
        if ui.button(tr("Import…")).clicked()
            && let Some(path) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file()
        {
            let ctx = ui.ctx().clone();
            match crate::planner::import_pois(app, &path.to_string_lossy()) {
                Ok(r) => {
                    let n = |k: &str| r.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
                    app.toast(
                        &ctx,
                        tr("Imported: {a} new, {u} updated").replace("{a}", &n("added").to_string()).replace("{u}", &n("updated").to_string()),
                    );
                }
                Err(e) => app.toast_error(&ctx, e),
            }
        }
    });
    hint(ui, t, tr("The same file format the phone app reads and writes."));
}
