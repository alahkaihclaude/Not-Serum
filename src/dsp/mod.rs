//! Plain-Rust DSP: no nih-plug types here, so `cargo test` runs without a host.

pub mod env;
pub mod filter;
pub mod lfo;
pub mod modmatrix;
pub mod osc;
pub mod wavetable;

pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

/// Rational tanh approximation, accurate to ~2% and exact at 0 and ±inf.
#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// Output safety: transparent below 0.8, then rounds off so the output never exceeds 1.0.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        (KNEE + (1.0 - KNEE) * fast_tanh((a - KNEE) / (1.0 - KNEE))).copysign(x)
    }
}

/// xorshift32: allocation-free noise and random phases for the audio thread.
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// Uniform in [0, 1).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }

    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_clip_is_bounded_and_transparent_below_knee() {
        assert_eq!(soft_clip(0.5), 0.5);
        for x in [1.0f32, 2.0, 10.0, 1e9, -1e9] {
            assert!(soft_clip(x).abs() <= 1.0);
        }
    }
}
