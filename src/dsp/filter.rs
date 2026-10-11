//! Topology-preserving-transform state-variable filter (Zavalishin / Simper), stereo, 12 or 24 dB.

use super::fast_tanh;
use std::f32::consts::PI;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FilterMode {
    Lp12,
    Lp24,
    Hp12,
    Hp24,
    Bp12,
    Bp24,
}

impl FilterMode {
    /// Same order as the `Filter Type` parameter.
    pub const ALL: [Self; 6] = [Self::Lp12, Self::Lp24, Self::Hp12, Self::Hp24, Self::Bp12, Self::Bp24];
}

#[derive(Clone, Copy)]
pub struct Coefs {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Coefs {
    /// `resonance` in [0, 1]; 1 is close to self-oscillation but always stable.
    pub fn new(cutoff: f32, resonance: f32, sample_rate: f32) -> Self {
        let g = (PI * cutoff.clamp(10.0, 0.49 * sample_rate) / sample_rate).tan();
        let k = 2.0 - 1.98 * resonance.clamp(0.0, 1.0);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self { a1, a2, a3: g * a2, k }
    }
}

#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    #[inline]
    fn tick(&mut self, c: &Coefs, mode: FilterMode, v0: f32) -> f32 {
        let v3 = v0 - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        match mode {
            FilterMode::Lp12 | FilterMode::Lp24 => v2,
            FilterMode::Hp12 | FilterMode::Hp24 => v0 - c.k * v1 - v2,
            // scaled by k so the peak stays at unity gain as resonance rises
            FilterMode::Bp12 | FilterMode::Bp24 => c.k * v1,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Filter {
    /// `[channel][stage]`
    s: [[Svf; 2]; 2],
}

impl Filter {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `c2` is the second (non-resonant) stage used by the 24 dB modes. `drive` 0 bypasses the saturator.
    #[inline]
    pub fn process(&mut self, c: &Coefs, c2: &Coefs, mode: FilterMode, drive: f32, l: f32, r: f32) -> (f32, f32) {
        let pole24 = matches!(mode, FilterMode::Lp24 | FilterMode::Hp24 | FilterMode::Bp24);
        let mut run = |ch: usize, x: f32| {
            let x = if drive > 0.0 { fast_tanh(x * (1.0 + 15.0 * drive * drive)) } else { x };
            let y = self.s[ch][0].tick(c, mode, x);
            if pole24 {
                self.s[ch][1].tick(c2, mode, y)
            } else {
                y
            }
        };
        (run(0, l), run(1, r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_at(mode: FilterMode, cutoff: f32, freq: f32) -> f32 {
        let sr = 48000.0;
        let c = Coefs::new(cutoff, 0.0, sr);
        let c2 = Coefs::new(cutoff, 0.3, sr);
        let mut f = Filter::default();
        let mut peak = 0.0f32;
        for i in 0..48000 {
            let x = (i as f32 * freq / sr * std::f32::consts::TAU).sin();
            let (y, _) = f.process(&c, &c2, mode, 0.0, x, x);
            if i > 24000 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn lowpass_and_highpass_respond_correctly() {
        assert!(gain_at(FilterMode::Lp12, 1000.0, 100.0) > 0.95);
        assert!(gain_at(FilterMode::Lp12, 1000.0, 10000.0) < 0.02);
        assert!(gain_at(FilterMode::Lp24, 1000.0, 10000.0) < gain_at(FilterMode::Lp12, 1000.0, 10000.0));
        assert!(gain_at(FilterMode::Hp12, 1000.0, 100.0) < 0.02);
        assert!(gain_at(FilterMode::Hp12, 1000.0, 10000.0) > 0.95);
    }

    #[test]
    fn full_resonance_stays_bounded() {
        let sr = 48000.0;
        let c = Coefs::new(2000.0, 1.0, sr);
        let mut f = Filter::default();
        for i in 0..48000 {
            let x = if i % 100 == 0 { 1.0 } else { 0.0 };
            let (l, r) = f.process(&c, &c, FilterMode::Lp24, 1.0, x, x);
            assert!(l.is_finite() && r.is_finite() && l.abs() < 100.0);
        }
    }
}
