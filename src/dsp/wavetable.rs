//! Wavetable storage with band-limited mip levels and interpolated lookup.

use realfft::RealFftPlanner;
use std::f32::consts::TAU;

/// Samples per frame (one single cycle).
pub const FRAME: usize = 2048;
/// Mip level `k` keeps harmonics `1..=(FRAME/2 >> k)`, so level 10 is a pure sine.
pub const MIPS: usize = 11;
/// One guard sample per mip so linear interpolation never wraps.
const STRIDE: usize = FRAME + 1;

pub struct Wavetable {
    frames: usize,
    /// Layout: `[frame][mip][STRIDE]`.
    data: Vec<f32>,
}

/// Picks the mip level for a phase increment (cycles per sample) so no harmonic exceeds Nyquist.
#[inline]
pub fn mip_for(inc: f32) -> usize {
    // Level k keeps harmonics <= 1024 >> k; need (1024 >> k) * inc <= 0.5, i.e. k >= log2(2048 * inc).
    let k = (FRAME as f32 * inc).log2().ceil();
    if k > 0.0 {
        (k as usize).min(MIPS - 1)
    } else {
        0
    }
}

impl Wavetable {
    /// Builds a table from single-cycle frames of `FRAME` samples each. Allocates; never call on the audio thread.
    pub fn from_frames(frames: &[Vec<f32>]) -> Self {
        assert!(!frames.is_empty() && frames.iter().all(|f| f.len() == FRAME));
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(FRAME);
        let inv = planner.plan_fft_inverse(FRAME);
        let mut data = vec![0.0; frames.len() * MIPS * STRIDE];
        let mut spectrum = fwd.make_output_vec();
        let mut limited = fwd.make_output_vec();
        let mut buf = vec![0.0; FRAME];

        for (f, frame) in frames.iter().enumerate() {
            buf.copy_from_slice(frame);
            fwd.process(&mut buf, &mut spectrum).expect("fft size");
            spectrum[0] = Default::default(); // DC
            spectrum[FRAME / 2] = Default::default(); // Nyquist

            for mip in 0..MIPS {
                let max_h = (FRAME / 2) >> mip;
                limited.copy_from_slice(&spectrum);
                limited[max_h + 1..].iter_mut().for_each(|c| *c = Default::default());
                inv.process(&mut limited, &mut buf).expect("fft size");
                let out = &mut data[(f * MIPS + mip) * STRIDE..][..STRIDE];
                for (o, s) in out.iter_mut().zip(&buf) {
                    *o = s / FRAME as f32;
                }
                out[FRAME] = out[0];
            }

            // Normalize each frame to peak 1 at full bandwidth so morphing keeps a steady level.
            let frame_data = &mut data[f * MIPS * STRIDE..][..MIPS * STRIDE];
            let peak = frame_data[..FRAME].iter().fold(0.0f32, |m, s| m.max(s.abs()));
            if peak > 1e-9 {
                frame_data.iter_mut().for_each(|s| *s /= peak);
            }
        }
        Self { frames: frames.len(), data }
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// `frame_pos` in `[0, frames-1]` (fractional morphs between frames), `phase` in `[0, 1)`.
    #[inline]
    pub fn sample(&self, frame_pos: f32, mip: usize, phase: f32) -> f32 {
        let f0 = frame_pos as usize;
        let f1 = (f0 + 1).min(self.frames - 1);
        let ft = frame_pos - f0 as f32;
        let p = phase * FRAME as f32;
        let i = p as usize;
        let t = p - i as f32;
        let a = self.lerp(f0, mip, i, t);
        let b = self.lerp(f1, mip, i, t);
        a + (b - a) * ft
    }

    #[inline]
    fn lerp(&self, frame: usize, mip: usize, i: usize, t: f32) -> f32 {
        let s = &self.data[(frame * MIPS + mip) * STRIDE + i..][..2];
        s[0] + (s[1] - s[0]) * t
    }
}

/// Built-in tables, in the same order as the `Table` parameter.
pub fn builtin() -> Vec<Wavetable> {
    vec![basic_shapes(), pwm(), formant(), harmonics()]
}

fn frame_from(f: impl Fn(f32) -> f32) -> Vec<f32> {
    (0..FRAME).map(|i| f(i as f32 / FRAME as f32)).collect()
}

/// Sum of harmonics with the given amplitudes (index 0 = fundamental).
fn additive(amps: impl Iterator<Item = f32>) -> Vec<f32> {
    let sine: Vec<f32> = frame_from(|x| (x * TAU).sin());
    let mut out = vec![0.0; FRAME];
    for (h, a) in amps.enumerate().filter(|(_, a)| *a != 0.0) {
        for (i, o) in out.iter_mut().enumerate() {
            *o += a * sine[(i * (h + 1)) % FRAME];
        }
    }
    out
}

/// Sine -> triangle -> saw -> square, phase-aligned so the morph stays smooth.
pub fn basic_shapes() -> Wavetable {
    Wavetable::from_frames(&[
        frame_from(|x| (x * TAU).sin()),
        frame_from(|x| {
            if x < 0.25 {
                4.0 * x
            } else if x < 0.75 {
                2.0 - 4.0 * x
            } else {
                4.0 * x - 4.0
            }
        }),
        frame_from(|x| 2.0 * (x + 0.5).fract() - 1.0),
        frame_from(|x| if x < 0.5 { 1.0 } else { -1.0 }),
    ])
}

/// Pulse width sweeping from 50% down to 2%.
fn pwm() -> Wavetable {
    let frames: Vec<_> = (0..32)
        .map(|j| {
            let duty = 0.5 - 0.48 * j as f32 / 31.0;
            frame_from(|x| if x < duty { 1.0 } else { -1.0 })
        })
        .collect();
    Wavetable::from_frames(&frames)
}

/// A resonant peak sweeping up through the harmonics over a soft saw-like base.
fn formant() -> Wavetable {
    let frames: Vec<_> = (0..32)
        .map(|j| {
            let center = 40f32.powf(j as f32 / 31.0); // harmonic 1 -> 40
            let width = 1.0 + center * 0.15;
            additive((1..=96).map(|h| {
                let h = h as f32;
                0.25 / h + (-((h - center) / width).powi(2)).exp()
            }))
        })
        .collect();
    Wavetable::from_frames(&frames)
}

/// Adds harmonics one band at a time: sine at frame 0, bright saw-like at the end.
fn harmonics() -> Wavetable {
    let frames: Vec<_> = (0..32)
        .map(|j| {
            let n = 1.15f32.powi(j).round() as usize;
            additive((1..=n).map(|h| 1.0 / h as f32))
        })
        .collect();
    Wavetable::from_frames(&frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_selection_keeps_harmonics_below_nyquist() {
        assert_eq!(mip_for(0.0), 0);
        assert_eq!(mip_for(20.0 / 48000.0), 0);
        assert_eq!(mip_for(0.49), MIPS - 1);
        for f in [30.0f32, 440.0, 1000.0, 3001.0, 8000.0] {
            let inc = f / 48000.0;
            let k = mip_for(inc);
            let top = (FRAME / 2) >> k;
            assert!(top as f32 * f <= 24000.0, "{f} Hz: mip {k} keeps {top} harmonics");
            if k > 0 {
                // and the next-brighter level would alias, so we're not throwing away more than needed
                assert!(((FRAME / 2) >> (k - 1)) as f32 * f > 24000.0);
            }
        }
    }

    #[test]
    fn builtin_tables_are_finite_and_normalized() {
        for t in builtin() {
            assert!(t.data.iter().all(|s| s.is_finite()));
            for f in 0..t.frames {
                let peak = t.data[f * MIPS * STRIDE..][..FRAME].iter().fold(0.0f32, |m, s| m.max(s.abs()));
                assert!((peak - 1.0).abs() < 1e-4);
            }
        }
    }

    /// Fraction of power outside the fundamental's harmonics: aliasing shows up there.
    fn inharmonic_power(table: &Wavetable, mip: usize, freq: usize) -> f32 {
        const SR: usize = 48000;
        // f64 phase: f32 accumulation drifts the pitch by a hair, smearing harmonics into neighbouring bins
        let inc = freq as f64 / SR as f64;
        let mut phase = 0.0f64;
        let mut buf: Vec<f32> = (0..SR)
            .map(|_| {
                let s = table.sample(2.0, mip, phase as f32);
                phase += inc;
                if phase >= 1.0 {
                    phase -= 1.0;
                }
                s
            })
            .collect();
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(SR);
        let mut spec = fft.make_output_vec();
        fft.process(&mut buf, &mut spec).unwrap();
        let (mut harm, mut other) = (0.0, 0.0);
        for (bin, c) in spec.iter().enumerate() {
            // 1 Hz bins; allow one bin either side for f32 phase drift
            if [bin.saturating_sub(1), bin, bin + 1].iter().any(|b| b % freq == 0 && *b > 0) {
                harm += c.norm_sqr();
            } else {
                other += c.norm_sqr();
            }
        }
        other / (harm + other)
    }

    #[test]
    fn high_saw_has_no_aliasing() {
        let t = basic_shapes();
        let freq = 3001; // ~F#7, 1 Hz FFT bins at 48 kHz
        let clean = inharmonic_power(&t, mip_for(freq as f32 / 48000.0), freq);
        assert!(clean < 1e-6, "aliased power {clean}");
        // sanity: the full-bandwidth level does alias, so the check above can fail
        let dirty = inharmonic_power(&t, 0, freq);
        assert!(dirty > 1e-3, "full-band power {dirty}");
    }
}
