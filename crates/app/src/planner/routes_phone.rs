//! A route on the phone: the line and its waypoints go to the Light Studio PDI Worker (`pdi_sync::send_route`) so the
//! phone app can show it in the field, without coverage. The sending itself runs in `crate::pdi`.

use serde_json::{Value, json};

use super::routes::SavedRoute;

/// A fresh id for a route on the phone (the same UUID shape as a point's).
pub fn new_phone_id() -> String {
    super::pois::new_id()
}

/// The route as the Worker keeps it: `{id, name, distance_m, ascent_m, descent_m, points: [[lat, lon, ele?]],
/// waypoints: [{name, lat, lon}]}`. Positions that cannot be a place on Earth are left out.
pub fn phone_json(r: &SavedRoute, id: &str) -> Value {
    let points: Vec<Value> = r
        .points
        .iter()
        .filter(|p| p.0.is_finite() && p.1.is_finite() && p.0.abs() <= 90.0 && p.1.abs() <= 180.0)
        .map(|(la, lo, e)| match e.filter(|e| e.is_finite()) {
            Some(e) => json!([la, lo, e.round()]),
            None => json!([la, lo]),
        })
        .collect();
    let waypoints: Vec<Value> = r
        .waypoints
        .iter()
        .filter(|w| w.0.is_finite() && w.1.is_finite() && w.0.abs() <= 90.0 && w.1.abs() <= 180.0)
        .take(200)
        .map(|(la, lo, name)| json!({"name": name, "lat": la, "lon": lo}))
        .collect();
    json!({
        "id": id, "name": r.name,
        "distance_m": r.distance_m.max(0.0).round(), "ascent_m": r.ascent_m.max(0.0).round(), "descent_m": r.descent_m.max(0.0).round(),
        "points": points, "waypoints": waypoints,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phone_gets_the_line_the_waypoints_and_the_totals_without_impossible_places() {
        let r = SavedRoute {
            id: 5,
            name: "Cresta".into(),
            distance_m: 5400.4,
            ascent_m: 320.6,
            points: vec![(43.1, -2.1, Some(100.4)), (43.2, -2.2, None), (95.0, 0.0, None), (f64::NAN, 1.0, None)],
            waypoints: vec![(43.15, -2.15, "Fuente".into()), (200.0, 0.0, "Mal".into())],
            ..Default::default()
        };
        let j = phone_json(&r, "0f0f0f0f-0f0f-4f0f-8f0f-0f0f0f0f0f0f");
        assert_eq!(j["points"], json!([[43.1, -2.1, 100.0], [43.2, -2.2]]));
        assert_eq!(j["waypoints"], json!([{"name": "Fuente", "lat": 43.15, "lon": -2.15}]));
        assert_eq!((j["distance_m"].as_f64(), j["ascent_m"].as_f64(), j["descent_m"].as_f64()), (Some(5400.0), Some(321.0), Some(0.0)));
        assert_eq!(j["name"], "Cresta");
    }
}
