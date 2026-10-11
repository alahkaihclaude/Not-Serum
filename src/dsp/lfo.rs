//! LFO shapes and phase state. Output is bipolar, [-1, 1].

use super::Rng;
use std::f32::consts::TAU;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LfoShape {
    Sine,
    Triangle,
    Saw,
    Square,
    SampleHold,
}

impl LfoShape {
    /// Same order as the `LFO Shape` parameter.
    pub const ALL: [Self; 5] = [Self::Sine, Self::Triangle, Self::Saw, Self::Square, Self::SampleHold];
}

/// Tempo-sync divisions, same order as the `LFO Division` parameter: (quarter notes, bars).
/// Length in quarter notes = `qn + bars * quarter notes per bar`.
pub const DIVISIONS: [(f32, f32); 15] = [
    (0.125, 0.0),       // 1/32
    (0.25 / 1.5, 0.0),  // 1/16T
    (0.25, 0.0),        // 1/16
    (0.375, 0.0),       // 1/16D
    (0.5 / 1.5, 0.0),   // 1/8T
    (0.5, 0.0),         // 1/8
    (0.75, 0.0),        // 1/8D
    (1.0 / 1.5, 0.0),   // 1/4T
    (1.0, 0.0),         // 1/4
    (1.5, 0.0),         // 1/4D
    (2.0, 0.0),         // 1/2
    (0.0, 1.0),         // 1 bar
    (0.0, 2.0),         // 2 bars
    (0.0, 4.0),         // 4 bars
    (0.0, 8.0),         // 8 bars
];

/// Length of one LFO cycle in quarter notes.
pub fn division_beats(index: usize, beats_per_bar: f32) -> f32 {
    let (qn, bars) = DIVISIONS[index.min(DIVISIONS.len() - 1)];
    qn + bars * beats_per_bar
}

#[derive(Clone, Copy, Default, Debug)]
pub struct LfoState {
    pub phase: f32,
    /// Current sample & hold value.
    pub held: f32,
}

impl LfoState {
    pub fn reset(&mut self, phase: f32, rng: &mut Rng) {
        self.phase = phase.rem_euclid(1.0);
        self.held = rng.bipolar();
    }

    /// Advance by `cycles`; S&H picks a new value each time the phase wraps.
    pub fn advance(&mut self, cycles: f32, rng: &mut Rng) {
        self.phase += cycles.max(0.0);
        if self.phase >= 1.0 {
            self.phase = self.phase.fract();
            self.held = rng.bipolar();
        }
    }

    pub fn value(&self, shape: LfoShape) -> f32 {
        let p = self.phase;
        match shape {
            LfoShape::Sine => (p * TAU).sin(),
            LfoShape::Triangle => {
                if p < 0.25 {
                    4.0 * p
                } else if p < 0.75 {
                    2.0 - 4.0 * p
                } else {
                    4.0 * p - 4.0
                }
            }
            LfoShape::Saw => 2.0 * p - 1.0,
            LfoShape::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::SampleHold => self.held,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_are_bipolar_and_bounded() {
        let mut rng = Rng::new(7);
        let mut s = LfoState::default();
        for shape in LfoShape::ALL {
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for _ in 0..1000 {
                s.advance(0.013, &mut rng);
                let v = s.value(shape);
                lo = lo.min(v);
                hi = hi.max(v);
            }
            assert!(lo >= -1.0 && hi <= 1.0 && lo < -0.5 && hi > 0.5, "{shape:?}: {lo}..{hi}");
        }
    }

    #[test]
    fn divisions_in_quarter_notes() {
        assert_eq!(division_beats(8, 4.0), 1.0); // 1/4
        assert_eq!(division_beats(11, 4.0), 4.0); // 1 bar of 4/4
        assert_eq!(division_beats(11, 3.0), 3.0); // 1 bar of 3/4
        assert!((division_beats(4, 4.0) - 1.0 / 3.0).abs() < 1e-6); // 1/8T
    }
}
