//! A framing kept with a plan or a planned shot: where the camera pointed and with what lens.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FrameShot {
    pub az: f64,
    pub focal: f64,
    pub portrait: bool,
    pub sensor: usize,
}

impl Default for FrameShot {
    fn default() -> Self {
        FrameShot { az: 90.0, focal: 24.0, portrait: false, sensor: optics::CANON_6D }
    }
}

impl FrameShot {
    /// The framing the planner has now.
    pub fn of(st: &PlannerState) -> FrameShot {
        FrameShot { az: st.cam_az, focal: st.focal, portrait: st.portrait, sensor: st.sensor }
    }

    /// Puts the framing back in the planner (numbers out of range are brought in) and shows the camera on the map.
    pub fn apply(&self, st: &mut PlannerState) {
        let fix = |v: f64, lo: f64, hi: f64, dflt: f64| if v.is_finite() { v.clamp(lo, hi) } else { dflt };
        st.cam_az = fix(self.az, 0.0, 360.0, 90.0);
        st.focal = fix(self.focal, 1.0, 2000.0, 24.0);
        st.portrait = self.portrait;
        st.sensor = if self.sensor < SENSORS.len() { self.sensor } else { optics::CANON_6D };
        if st.cam_tool == 0 {
            st.cam_tool = 1;
        }
    }
}
