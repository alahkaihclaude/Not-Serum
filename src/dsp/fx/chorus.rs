use super::{Enable, Smooth, Status};
use std::f32::consts::TAU;

const BASE_S: f32 = 0.015;
/// Depth 1.0 sweeps this far either side of the base delay.
const DEPTH_S: f32 = 0.007;

/// Two modulated delay lines with quadrature LFOs (left/right 90 degrees apart).
pub struct Chorus {
    on: Enable,
    mix: Smooth,
    depth: Smooth,
    buf: [Vec<f32>; 2],
    pos: usize,
    phase: f32,
    sr: f32,
}

impl Chorus {
    pub fn new(sr: f32) -> Self {
        let len = ((BASE_S + DEPTH_S) * sr) as usize + 4;
        Self {
            on: Enable::new(sr),
            mix: Smooth::new(0.5, sr),
            depth: Smooth::new(0.5, sr),
            buf: [vec![0.0; len], vec![0.0; len]],
            pos: 0,
            phase: 0.0,
            sr,
        }
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|b| b.fill(0.0));
    }

    pub fn process(&mut self, on: bool, rate: f32, depth: f32, mix: f32, left: &mut [f32], right: &mut [f32]) {
        match self.on.begin(on) {
            Status::Run => {}
            Status::Clear => return self.clear(),
            Status::Skip => return,
        }
        let len = self.buf[0].len();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.phase = (self.phase + rate / self.sr).fract();
            let d = self.depth.next(depth) * DEPTH_S;
            let amount = self.mix.next(mix) * self.on.next(on);
            for (c, x) in [l, r].into_iter().enumerate() {
                let lfo = ((self.phase + 0.25 * c as f32) * TAU).sin();
                let delay = (BASE_S + d * lfo) * self.sr;
                let read = (self.pos + len) as f32 - delay;
                let i = read as usize;
                let t = read - i as f32;
                let b = &mut self.buf[c];
                let wet = b[i % len] + (b[(i + 1) % len] - b[i % len]) * t;
                b[self.pos] = *x;
                *x += (wet - *x) * amount;
            }
            self.pos = (self.pos + 1) % len;
        }
    }
}
