//! Undo and redo of the pins: the place the planner looks from and the target (the black pin). Every change the view sees
//! is kept, so a click on the wrong spot is one step back.

use super::PlannerState;

/// How many steps are kept.
const MAX_STEPS: usize = 50;

/// The two pins and the place's name.
#[derive(Clone, Debug, PartialEq)]
pub struct Pins {
    pub lat: f64,
    pub lon: f64,
    pub place: String,
    pub target: Option<[f64; 2]>,
}

impl Pins {
    pub fn of(st: &PlannerState) -> Pins {
        Pins { lat: st.lat, lon: st.lon, place: st.place.clone(), target: st.target }
    }
}

#[derive(Default)]
pub struct PinHistory {
    undo: Vec<Pins>,
    redo: Vec<Pins>,
    last: Option<Pins>,
}

impl PinHistory {
    /// Looks at the pins as they are now: a change since the last look becomes a step.
    pub fn observe(&mut self, now: Pins) {
        match &self.last {
            None => {}
            Some(last) if *last == now => return,
            Some(last) => {
                self.undo.push(last.clone());
                if self.undo.len() > MAX_STEPS {
                    self.undo.remove(0);
                }
                self.redo.clear();
            }
        }
        self.last = Some(now);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The pins one step back; `now` is kept to redo.
    pub fn undo(&mut self, now: Pins) -> Option<Pins> {
        let back = self.undo.pop()?;
        self.redo.push(now);
        self.last = Some(back.clone());
        Some(back)
    }

    /// The pins one step forward again.
    pub fn redo(&mut self, now: Pins) -> Option<Pins> {
        let next = self.redo.pop()?;
        self.undo.push(now);
        self.last = Some(next.clone());
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(lat: f64, target: Option<[f64; 2]>) -> Pins {
        Pins { lat, lon: 0.0, place: String::new(), target }
    }

    #[test]
    fn changes_become_steps_that_undo_and_redo() {
        let mut h = PinHistory::default();
        h.observe(at(1.0, None));
        assert!(!h.can_undo(), "the first look is the start, not a step");
        h.observe(at(1.0, None));
        h.observe(at(2.0, None));
        h.observe(at(2.0, Some([3.0, 3.0])));
        assert!(h.can_undo() && !h.can_redo());
        let back = h.undo(at(2.0, Some([3.0, 3.0]))).unwrap();
        assert_eq!(back, at(2.0, None));
        // what undo restored is not a new step
        h.observe(back.clone());
        assert!(h.can_redo());
        let back2 = h.undo(back).unwrap();
        assert_eq!(back2, at(1.0, None));
        assert!(!h.can_undo());
        let fwd = h.redo(back2).unwrap();
        assert_eq!(fwd, at(2.0, None));
        // a new change forgets the redo steps
        h.observe(fwd);
        h.observe(at(9.0, None));
        assert!(!h.can_redo());
        assert!(h.undo(at(9.0, None)).is_some());
    }

    #[test]
    fn only_the_last_steps_are_kept() {
        let mut h = PinHistory::default();
        for i in 0..200 {
            h.observe(at(f64::from(i), None));
        }
        let mut n = 0;
        let mut now = at(199.0, None);
        while let Some(b) = h.undo(now.clone()) {
            now = b;
            n += 1;
        }
        assert_eq!(n, MAX_STEPS);
    }
}
