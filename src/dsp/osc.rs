//! Wavetable oscillator with up to 16 unison voices.

use super::wavetable::{mip_for, Wavetable};
use super::Rng;

pub const MAX_UNISON: usize = 16;
/// Detune 1.0 spreads the outer unison voices by this many semitones either side.
const MAX_DETUNE_SEMIS: f32 = 1.0;

/// Per-block unison layout: frequency and stereo gain for each unison voice.
pub struct Unison {
    n: usize,
    inc: [f32; MAX_UNISON],
    gl: [f32; MAX_UNISON],
    gr: [f32; MAX_UNISON],
    mip: usize,
}

pub struct UnisonSettings {
    pub voices: usize,
    pub detune: f32,
    pub blend: f32,
    pub width: f32,
    pub pan: f32,
    pub level: f32,
}

impl Unison {
    pub fn new(freq: f32, sample_rate: f32, s: &UnisonSettings) -> Self {
        let n = s.voices.clamp(1, MAX_UNISON);
        let mut u = Self { n, inc: [0.0; MAX_UNISON], gl: [0.0; MAX_UNISON], gr: [0.0; MAX_UNISON], mip: 0 };
        let mut weights = [0.0f32; MAX_UNISON];
        let mut max_inc = 0.0f32;
        for i in 0..n {
            // spread position in [-1, 1]
            let t = if n == 1 { 0.0 } else { 2.0 * i as f32 / (n - 1) as f32 - 1.0 };
            let semis = t * s.detune * MAX_DETUNE_SEMIS;
            u.inc[i] = (freq * (semis / 12.0).exp2() / sample_rate).min(0.49);
            max_inc = max_inc.max(u.inc[i]);
            // the middle voice (two for an even count) is full level, blend sets the others
            let center = 2 * i + 1 == n || (n % 2 == 0 && (i == n / 2 || i + 1 == n / 2));
            weights[i] = if center { 1.0 } else { s.blend };
            let pan = (s.pan + t * s.width).clamp(-1.0, 1.0);
            u.gl[i] = (1.0 - pan).min(1.0);
            u.gr[i] = (1.0 + pan).min(1.0);
        }
        let norm = s.level / weights[..n].iter().map(|w| w * w).sum::<f32>().sqrt().max(1e-6);
        for i in 0..n {
            u.gl[i] *= weights[i] * norm;
            u.gr[i] *= weights[i] * norm;
        }
        u.mip = mip_for(max_inc);
        u
    }
}

#[derive(Clone, Copy, Default)]
pub struct Osc {
    phases: [f32; MAX_UNISON],
}

impl Osc {
    /// `phase` and `random` in [0, 1]: start phase, plus how much of a random offset each unison voice gets.
    pub fn reset(&mut self, phase: f32, random: f32, rng: &mut Rng) {
        for p in &mut self.phases {
            *p = (phase + random * rng.next_f32()).fract();
        }
    }

    #[inline]
    pub fn tick(&mut self, table: &Wavetable, frame_pos: f32, u: &Unison) -> (f32, f32) {
        let (mut l, mut r) = (0.0, 0.0);
        for i in 0..u.n {
            let s = table.sample(frame_pos, u.mip, self.phases[i]);
            l += s * u.gl[i];
            r += s * u.gr[i];
            self.phases[i] += u.inc[i];
            if self.phases[i] >= 1.0 {
                self.phases[i] -= 1.0;
            }
        }
        (l, r)
    }
}
