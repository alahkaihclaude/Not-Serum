//! ADSR: linear attack, exponential decay and release.

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

#[derive(Clone, Copy)]
pub struct Adsr {
    stage: Stage,
    level: f32,
    attack_step: f32,
    decay_coef: f32,
    sustain: f32,
    release_coef: f32,
}

/// Below this the envelope counts as silent (-80 dB).
const FLOOR: f32 = 1e-4;

impl Default for Adsr {
    fn default() -> Self {
        Self { stage: Stage::Idle, level: 0.0, attack_step: 1.0, decay_coef: 0.0, sustain: 1.0, release_coef: 0.0 }
    }
}

impl Adsr {
    /// Times in seconds. Cheap enough to call once per block.
    pub fn set(&mut self, attack: f32, decay: f32, sustain: f32, release: f32, sample_rate: f32) {
        // Exponential segments fall by 60 dB over the given time.
        let coef = |t: f32| (-6.9078 / (t.max(0.001) * sample_rate)).exp();
        self.attack_step = 1.0 / (attack * sample_rate).max(1.0);
        self.decay_coef = coef(decay);
        self.sustain = sustain.clamp(0.0, 1.0);
        self.release_coef = coef(release);
    }

    /// Starts from the current level, so retriggering a sounding voice doesn't click.
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            // Sustain is the tail of decay, so sustain changes are followed smoothly.
            Stage::Decay => self.level = self.sustain + (self.level - self.sustain) * self.decay_coef,
            Stage::Release => {
                self.level *= self.release_coef;
                if self.level < FLOOR {
                    self.reset();
                }
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaches_sustain_then_idle_after_release() {
        let sr = 48000.0;
        let mut e = Adsr::default();
        e.set(0.01, 0.1, 0.5, 0.2, sr);
        e.gate_on();
        let peak = (0..480).map(|_| e.next()).fold(0.0, f32::max);
        assert!((peak - 1.0).abs() < 1e-3);
        for _ in 0..48000 {
            e.next();
        }
        assert!((e.level() - 0.5).abs() < 1e-3);
        e.gate_off();
        // -60 dB in 0.2 s, -80 dB floor in ~0.27 s
        for _ in 0..(0.3 * sr) as usize {
            e.next();
        }
        assert!(e.is_idle());
        assert_eq!(e.next(), 0.0);
    }

    #[test]
    fn zero_sustain_release_during_attack_goes_idle() {
        let mut e = Adsr::default();
        e.set(1.0, 0.01, 0.0, 0.01, 48000.0);
        e.gate_on();
        for _ in 0..100 {
            e.next();
        }
        e.gate_off();
        for _ in 0..2000 {
            e.next();
        }
        assert!(e.is_idle());
    }
}
