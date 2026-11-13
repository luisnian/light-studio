//! `cargo run --release --example snapshot -- <state.json|-> <out.png> [WxH] [scale] [frames] [card|-] [settings]`
//!
//! Draws the app in the given saved state (a `state.json`; `-` is the defaults) into a PNG with no window and no GPU.
//! Set `LIGHT_STUDIO_CONFIG_DIR=` (empty) so nothing is read from or written to the real settings folder, and
//! `LIGHT_STUDIO_MAP_TILES` to a tile server on this machine to have a map under the cards.

use light_studio::snapshot::Session;
use light_studio::state::UiState;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let state_path = args.first().ok_or("usage: snapshot <state.json|-> <out.png> [WxH] [scale] [frames]")?;
    let out = args.get(1).ok_or("missing output path")?;
    let size = args
        .get(2)
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?]))
        .unwrap_or([1360.0, 860.0]);
    let scale: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let frames: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(40);
    let state = if state_path == "-" {
        UiState::default()
    } else {
        let text = std::fs::read_to_string(state_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())?
    };
    let mut session = Session::new(state, size, scale);
    if let Some(card) = args.get(5).filter(|c| c.as_str() != "-")
        && !light_studio::planner::open_card_named(&mut session.app.planner, card)
    {
        return Err(format!("no card called {card}"));
    }
    session.app.settings_open = args.get(6).is_some_and(|s| s == "settings");
    session.settle(frames, std::time::Duration::from_millis(60));
    let image = session.image();
    let file = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), image.size[0] as u32, image.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    writer.write_image_data(&bytes).map_err(|e| e.to_string())?;
    Ok(())
}
