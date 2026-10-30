//! What the app remembers between sessions: the planner's place, day and layers, the map's look and the settings. It is one
//! JSON file in the configuration folder, written whole through a temporary file so a failure never leaves half of it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::planner::PlannerState;

/// The points-of-interest Worker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
#[derive(Default)]
pub struct PdiSettings {
    /// Look for new points of interest when the app opens and every few minutes.
    pub enabled: bool,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub pdi: PdiSettings,
    /// `en` or `es`; empty follows the system.
    pub language: String,
    /// The red-light look, to keep night vision in the field.
    pub red_light: bool,
    /// No blurred map under the cards (a plain translucent glass).
    pub plain_glass: bool,
    /// No animations: cards and bars appear at once.
    pub reduce_motion: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub planner: PlannerState,
    /// Satellite imagery with place names, else the plain map.
    pub map_satellite: bool,
    /// A CARTO key for the plain map (empty: OpenStreetMap's).
    pub map_carto_key: String,
    /// A PMTiles map file on this computer, used instead of the network (empty: none).
    pub map_offline: String,
    /// A folder of 1-metre LiDAR height files (.asc) for fine shadows (empty: none).
    pub fine_terrain: String,
    pub settings: Settings,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            planner: PlannerState::default(),
            map_satellite: true,
            map_carto_key: String::new(),
            map_offline: String::new(),
            fine_terrain: String::new(),
            settings: Settings::default(),
        }
    }
}

fn file() -> Option<PathBuf> {
    crate::paths::config_dir().map(|d| d.join("state.json"))
}

impl UiState {
    /// Reads the saved state; a missing or damaged file gives the defaults (a damaged one is kept as `state.json.bad`).
    pub fn load() -> UiState {
        let Some(path) = file() else { return UiState::default() };
        let Ok(text) = std::fs::read_to_string(&path) else { return UiState::default() };
        match serde_json::from_str::<UiState>(&text) {
            Ok(state) => state,
            Err(e) => {
                log::warn!("the saved state is not readable ({e}); starting afresh");
                let _ = std::fs::rename(&path, path.with_extension("json.bad"));
                UiState::default()
            }
        }
    }

    /// The text that would be written.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }
}

/// Writes `text` as the saved state.
pub fn write(text: &str) -> Result<(), String> {
    let Some(path) = file() else { return Ok(()) };
    let dir = path.parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not write {}: {e}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_survives_a_round_trip_and_unknown_fields_are_ignored() {
        let mut s = UiState::default();
        s.planner.lat = 43.3;
        s.settings.pdi.url = "https://pdi.example.com".into();
        s.settings.language = "es".into();
        let text = s.to_json().unwrap();
        assert!(text.contains("mapSatellite"));
        let back: UiState = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);
        let partial: UiState = serde_json::from_str(r#"{"mapCartoKey":"k","somethingNew":1}"#).unwrap();
        assert_eq!(partial.map_carto_key, "k");
        assert!(partial.map_satellite, "the defaults fill what is missing");
    }
}
