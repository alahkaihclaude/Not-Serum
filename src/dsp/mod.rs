//! Plain-Rust DSP: no nih-plug types here, so `cargo test` runs without a host.

use std::f32::consts::TAU;

/// Phase 0 "hello" voice: one monophonic sine with a short linear fade to avoid clicks.
/// shortcut: last-note mono only, replaced by the wavetable voice engine in Phase 1.
pub struct SineVoice {
    phase: f32,
    freq: f32,
    gain: f32,
    target: f32,
    note: Option<u8>,
}

/// Fade time in seconds for note on/off.
const FADE_S: f32 = 0.005;

impl Default for SineVoice {
    fn default() -> Self {
        Self { phase: 0.0, freq: 440.0, gain: 0.0, target: 0.0, note: None }
    }
}

impl SineVoice {
    pub fn note_on(&mut self, note: u8, velocity: f32) {
        self.note = Some(note);
        self.freq = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
        self.target = velocity;
    }

    pub fn note_off(&mut self, note: u8) {
        if self.note == Some(note) {
            self.note = None;
            self.target = 0.0;
        }
    }

    pub fn next(&mut self, sample_rate: f32) -> f32 {
        let step = 1.0 / (FADE_S * sample_rate);
        self.gain += (self.target - self.gain).clamp(-step, step);
        let out = (self.phase * TAU).sin() * self.gain;
        self.phase = (self.phase + self.freq / sample_rate).fract();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_then_goes_silent() {
        let mut v = SineVoice::default();
        v.note_on(69, 1.0);
        let peak = (0..4800).map(|_| v.next(48000.0).abs()).fold(0.0, f32::max);
        assert!(peak > 0.9 && peak <= 1.0, "peak {peak}");
        v.note_off(69);
        for _ in 0..4800 {
            v.next(48000.0);
        }
        assert_eq!(v.next(48000.0), 0.0);
    }
}
