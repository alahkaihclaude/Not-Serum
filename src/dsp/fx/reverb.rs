//! Freeverb-style reverb (8 damped combs + 4 allpasses per channel). The algorithm is public domain;
//! this is an independent implementation.

use super::{Enable, Smooth, Status};

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const INPUT_GAIN: f32 = 0.015;
const WET_GAIN: f32 = 3.0;

struct Comb {
    buf: Vec<f32>,
    pos: usize,
    store: f32,
}

impl Comb {
    #[inline]
    fn tick(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.pos];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.pos] = x + self.store * feedback;
        self.pos = (self.pos + 1) % self.buf.len();
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let delayed = self.buf[self.pos];
        self.buf[self.pos] = x + delayed * 0.5;
        self.pos = (self.pos + 1) % self.buf.len();
        delayed - x
    }
}

pub struct Reverb {
    on: Enable,
    mix: Smooth,
    size: Smooth,
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
}

impl Reverb {
    pub fn new(sr: f32) -> Self {
        // tunings are for 44.1 kHz
        let scale = |n: usize, spread: usize| (((n + spread) as f32 * sr / 44100.0) as usize).max(1);
        let channel = |spread: usize| {
            (
                COMBS.iter().map(|&n| Comb { buf: vec![0.0; scale(n, spread)], pos: 0, store: 0.0 }).collect(),
                ALLPASSES.iter().map(|&n| Allpass { buf: vec![0.0; scale(n, spread)], pos: 0 }).collect(),
            )
        };
        let (cl, al) = channel(0);
        let (cr, ar) = channel(STEREO_SPREAD);
        Self { on: Enable::new(sr), mix: Smooth::new(0.25, sr), size: Smooth::new(0.6, sr), combs: [cl, cr], allpasses: [al, ar] }
    }

    pub fn clear(&mut self) {
        for c in self.combs.iter_mut().flatten() {
            c.buf.fill(0.0);
            c.store = 0.0;
        }
        for a in self.allpasses.iter_mut().flatten() {
            a.buf.fill(0.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process(&mut self, on: bool, size: f32, damping: f32, width: f32, mix: f32, left: &mut [f32], right: &mut [f32]) {
        match self.on.begin(on) {
            Status::Run => {}
            Status::Clear => return self.clear(),
            Status::Skip => return,
        }
        let damp = damping.clamp(0.0, 1.0) * 0.4;
        let wet1 = width.clamp(0.0, 1.0) * 0.5 + 0.5;
        let wet2 = (1.0 - width.clamp(0.0, 1.0)) * 0.5;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let feedback = 0.7 + self.size.next(size) * 0.28;
            let amount = self.mix.next(mix) * self.on.next(on);
            let input = (*l + *r) * INPUT_GAIN;
            let mut out = [0.0f32; 2];
            for c in 0..2 {
                let mut acc: f32 = self.combs[c].iter_mut().map(|comb| comb.tick(input, feedback, damp)).sum();
                for a in &mut self.allpasses[c] {
                    acc = a.tick(acc);
                }
                out[c] = acc * WET_GAIN;
            }
            let (wl, wr) = (out[0] * wet1 + out[1] * wet2, out[1] * wet1 + out[0] * wet2);
            *l += (wl - *l) * amount;
            *r += (wr - *r) * amount;
        }
    }
}
