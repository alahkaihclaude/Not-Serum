//! 3-band EQ: low shelf, peaking mid, high shelf (RBJ cookbook biquads).

use super::{Enable, Status};
use std::f32::consts::{PI, TAU};

pub const LOW_HZ: f32 = 200.0;
pub const HIGH_HZ: f32 = 5000.0;
const MID_Q: f32 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
}

#[derive(Clone, Copy, PartialEq)]
pub enum Band {
    LowShelf,
    Peak,
    HighShelf,
}

impl Biquad {
    pub fn new(band: Band, freq: f32, gain_db: f32, sr: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w = TAU * freq.clamp(10.0, 0.45 * sr) / sr;
        let (sin, cos) = w.sin_cos();
        let (b, a_) = match band {
            Band::Peak => {
                let alpha = sin / (2.0 * MID_Q);
                ([1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a], [1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a])
            }
            Band::LowShelf | Band::HighShelf => {
                let alpha = sin / 2.0 * std::f32::consts::SQRT_2; // shelf slope 1
                let sq = 2.0 * a.sqrt() * alpha;
                if band == Band::LowShelf {
                    (
                        [a * ((a + 1.0) - (a - 1.0) * cos + sq), 2.0 * a * ((a - 1.0) - (a + 1.0) * cos), a * ((a + 1.0) - (a - 1.0) * cos - sq)],
                        [(a + 1.0) + (a - 1.0) * cos + sq, -2.0 * ((a - 1.0) + (a + 1.0) * cos), (a + 1.0) + (a - 1.0) * cos - sq],
                    )
                } else {
                    (
                        [a * ((a + 1.0) + (a - 1.0) * cos + sq), -2.0 * a * ((a - 1.0) + (a + 1.0) * cos), a * ((a + 1.0) + (a - 1.0) * cos - sq)],
                        [(a + 1.0) - (a - 1.0) * cos + sq, 2.0 * ((a - 1.0) - (a + 1.0) * cos), (a + 1.0) - (a - 1.0) * cos - sq],
                    )
                }
            }
        };
        Self { b: [b[0] / a_[0], b[1] / a_[0], b[2] / a_[0]], a: [a_[1] / a_[0], a_[2] / a_[0]] }
    }

    /// Gain at `freq` (linear), for drawing the response curve.
    pub fn magnitude(&self, freq: f32, sr: f32) -> f32 {
        let w = 2.0 * PI * freq / sr;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let num = ((self.b[0] + self.b[1] * c1 + self.b[2] * c2).powi(2) + (self.b[1] * s1 + self.b[2] * s2).powi(2)).sqrt();
        let den = ((1.0 + self.a[0] * c1 + self.a[1] * c2).powi(2) + (self.a[0] * s1 + self.a[1] * s2).powi(2)).sqrt();
        num / den
    }

    /// Transposed direct form II; `z` is the per-channel state.
    #[inline]
    fn tick(&self, z: &mut [f32; 2], x: f32) -> f32 {
        let y = self.b[0] * x + z[0];
        z[0] = self.b[1] * x - self.a[0] * y + z[1];
        z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The three bands for the given settings (shared with the GUI's response curve).
pub fn bands(low_db: f32, mid_db: f32, mid_hz: f32, high_db: f32, sr: f32) -> [Biquad; 3] {
    [
        Biquad::new(Band::LowShelf, LOW_HZ, low_db, sr),
        Biquad::new(Band::Peak, mid_hz, mid_db, sr),
        Biquad::new(Band::HighShelf, HIGH_HZ, high_db, sr),
    ]
}

pub struct Eq {
    on: Enable,
    settings: [f32; 4],
    filters: [Biquad; 3],
    /// `[band][channel]`
    z: [[[f32; 2]; 2]; 3],
    sr: f32,
}

impl Eq {
    pub fn new(sr: f32) -> Self {
        Self { on: Enable::new(sr), settings: [0.0, 0.0, 1000.0, 0.0], filters: bands(0.0, 0.0, 1000.0, 0.0, sr), z: [[[0.0; 2]; 2]; 3], sr }
    }

    pub fn clear(&mut self) {
        self.z = [[[0.0; 2]; 2]; 3];
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process(&mut self, on: bool, low: f32, mid: f32, mid_hz: f32, high: f32, left: &mut [f32], right: &mut [f32]) {
        match self.on.begin(on) {
            Status::Run => {}
            Status::Clear => return self.clear(),
            Status::Skip => return,
        }
        // shortcut: coefficients update per block (32 samples), fine for knob moves and slow modulation
        if self.settings != [low, mid, mid_hz, high] {
            self.settings = [low, mid, mid_hz, high];
            self.filters = bands(low, mid, mid_hz, high, self.sr);
        }
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let amount = self.on.next(on);
            for (c, x) in [l, r].into_iter().enumerate() {
                let mut y = *x;
                for (f, z) in self.filters.iter().zip(self.z.iter_mut()) {
                    y = f.tick(&mut z[c], y);
                }
                *x += (y - *x) * amount;
            }
        }
    }
}
