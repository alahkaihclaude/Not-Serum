//! Effects chain: distortion -> chorus -> delay -> reverb -> EQ. Stereo, fixed order.
//! Buffers are allocated in `Fx::new` (call it off the audio thread); `process` never allocates.

mod chorus;
mod delay;
mod distortion;
mod eq;
mod reverb;

pub use distortion::{drive_gain, DistMode};
pub use eq::{bands, Biquad};

use super::modmatrix::Dest;
use crate::voice::Transport;

#[derive(Clone, Copy, Debug)]
pub struct FxPatch {
    pub dist_on: bool,
    pub dist_mode: DistMode,
    pub dist_drive: f32,
    pub dist_mix: f32,
    pub chorus_on: bool,
    pub chorus_rate: f32,
    pub chorus_depth: f32,
    pub chorus_mix: f32,
    pub delay_on: bool,
    pub delay_sync: bool,
    pub delay_ms: f32,
    /// Index into `dsp::lfo::DIVISIONS`.
    pub delay_division: usize,
    pub delay_feedback: f32,
    pub delay_pingpong: bool,
    /// Low-pass cutoff in the feedback path, Hz.
    pub delay_tone: f32,
    pub delay_mix: f32,
    pub reverb_on: bool,
    pub reverb_size: f32,
    pub reverb_damping: f32,
    pub reverb_width: f32,
    pub reverb_mix: f32,
    pub eq_on: bool,
    /// dB
    pub eq_low: f32,
    pub eq_mid: f32,
    pub eq_mid_freq: f32,
    pub eq_high: f32,
}

impl Default for FxPatch {
    fn default() -> Self {
        Self {
            dist_on: false,
            dist_mode: DistMode::Soft,
            dist_drive: 0.3,
            dist_mix: 1.0,
            chorus_on: false,
            chorus_rate: 0.8,
            chorus_depth: 0.5,
            chorus_mix: 0.5,
            delay_on: false,
            delay_sync: true,
            delay_ms: 300.0,
            delay_division: 6, // 1/8D
            delay_feedback: 0.35,
            delay_pingpong: true,
            delay_tone: 6000.0,
            delay_mix: 0.25,
            reverb_on: false,
            reverb_size: 0.6,
            reverb_damping: 0.5,
            reverb_width: 1.0,
            reverb_mix: 0.25,
            eq_on: false,
            eq_low: 0.0,
            eq_mid: 0.0,
            eq_mid_freq: 1000.0,
            eq_high: 0.0,
        }
    }
}

/// EQ gain range either side of 0 dB.
pub const EQ_RANGE_DB: f32 = 15.0;

/// One-pole parameter smoother (~5 ms), stepped per sample to avoid zipper noise and clicks.
#[derive(Clone, Copy)]
struct Smooth {
    value: f32,
    coef: f32,
}

impl Smooth {
    fn new(value: f32, sample_rate: f32) -> Self {
        Self { value, coef: 1.0 - (-1.0 / (0.005 * sample_rate)).exp() }
    }

    #[inline]
    fn next(&mut self, target: f32) -> f32 {
        self.value += (target - self.value) * self.coef;
        self.value
    }
}

#[derive(PartialEq)]
enum Status {
    Run,
    Clear,
    Skip,
}

/// Fades an effect in/out when toggled. Reports when it's fully off so the effect can skip work and clear its state.
#[derive(Clone, Copy)]
struct Enable {
    gain: Smooth,
    /// Still holding state from when it was last on (cleared once after fading out).
    dirty: bool,
}

impl Enable {
    fn new(sample_rate: f32) -> Self {
        // slower than `Smooth` so toggling is a short crossfade, not a click
        Self { gain: Smooth { value: 0.0, coef: 1.0 - (-1.0 / (0.010 * sample_rate)).exp() }, dirty: false }
    }

    /// `Skip` while the effect is off and faded out; `Clear` exactly once when it gets there.
    fn begin(&mut self, on: bool) -> Status {
        if !on && self.gain.value < 1e-4 {
            self.gain.value = 0.0;
            if std::mem::take(&mut self.dirty) {
                Status::Clear
            } else {
                Status::Skip
            }
        } else {
            self.dirty = true;
            Status::Run
        }
    }

    #[inline]
    fn next(&mut self, on: bool) -> f32 {
        self.gain.next(if on { 1.0 } else { 0.0 })
    }
}

pub struct Fx {
    dist: distortion::Distortion,
    chorus: chorus::Chorus,
    delay: delay::Delay,
    reverb: reverb::Reverb,
    eq: eq::Eq,
}

impl Fx {
    /// Allocates delay/reverb memory for this sample rate.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            dist: distortion::Distortion::new(sample_rate),
            chorus: chorus::Chorus::new(sample_rate),
            delay: delay::Delay::new(sample_rate),
            reverb: reverb::Reverb::new(sample_rate),
            eq: eq::Eq::new(sample_rate),
        }
    }

    /// Silences all tails without reallocating (safe on the audio thread).
    pub fn reset(&mut self) {
        self.dist.clear();
        self.chorus.clear();
        self.delay.clear();
        self.reverb.clear();
        self.eq.clear();
    }

    /// In place. `mods` is the mod-matrix output (indexed by `Dest as usize`) of the most recent voice.
    pub fn process(&mut self, p: &FxPatch, mods: &[f32], transport: &Transport, left: &mut [f32], right: &mut [f32]) {
        let m = |d: Dest| mods[d as usize];
        let unit = |v: f32, d: Dest| (v + m(d)).clamp(0.0, 1.0);
        self.dist.process(p.dist_on, p.dist_mode, unit(p.dist_drive, Dest::DistDrive), unit(p.dist_mix, Dest::DistMix), left, right);
        self.chorus.process(p.chorus_on, p.chorus_rate, p.chorus_depth, unit(p.chorus_mix, Dest::ChorusMix), left, right);
        let delay_settings = delay::Settings {
            time_s: if p.delay_sync {
                60.0 / transport.tempo.max(1.0) * super::lfo::division_beats(p.delay_division, transport.beats_per_bar)
            } else {
                p.delay_ms / 1000.0
            },
            feedback: unit(p.delay_feedback, Dest::DelayFeedback) * 0.95,
            pingpong: p.delay_pingpong,
            tone: p.delay_tone,
            mix: unit(p.delay_mix, Dest::DelayMix),
        };
        self.delay.process(p.delay_on, &delay_settings, left, right);
        self.reverb.process(
            p.reverb_on,
            unit(p.reverb_size, Dest::ReverbSize),
            p.reverb_damping,
            p.reverb_width,
            unit(p.reverb_mix, Dest::ReverbMix),
            left,
            right,
        );
        let db = |v: f32, d: Dest| (v + m(d) * 2.0 * EQ_RANGE_DB).clamp(-EQ_RANGE_DB, EQ_RANGE_DB);
        self.eq.process(
            p.eq_on,
            db(p.eq_low, Dest::EqLow),
            db(p.eq_mid, Dest::EqMid),
            p.eq_mid_freq,
            db(p.eq_high, Dest::EqHigh),
            left,
            right,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::modmatrix::NUM_DESTS;
    use crate::dsp::Rng;

    const SR: f32 = 48000.0;

    fn all_on() -> FxPatch {
        FxPatch { dist_on: true, chorus_on: true, delay_on: true, reverb_on: true, eq_on: true, ..FxPatch::default() }
    }

    fn run(fx: &mut Fx, p: &FxPatch, input: impl Fn(usize) -> f32, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mods = [0.0; NUM_DESTS];
        let t = Transport::default();
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..n).map(|i| (input(i), input(i))).unzip();
        for (l, r) in l.chunks_mut(32).zip(r.chunks_mut(32)) {
            fx.process(p, &mods, &t, l, r);
        }
        (l, r)
    }

    #[test]
    fn extreme_settings_stay_finite_and_bounded() {
        let mut fx = Fx::new(SR);
        let p = FxPatch {
            dist_drive: 1.0,
            delay_feedback: 1.0,
            delay_sync: false,
            delay_ms: 1.0,
            reverb_size: 1.0,
            eq_low: 15.0,
            eq_mid: 15.0,
            eq_high: 15.0,
            ..all_on()
        };
        for mode in DistMode::ALL {
            let mut rng = Rng::new(3);
            let noise: Vec<f32> = (0..SR as usize * 2).map(|_| rng.bipolar()).collect();
            let (l, r) = run(&mut fx, &FxPatch { dist_mode: mode, ..p }, |i| noise[i], noise.len());
            assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 50.0), "{mode:?}");
        }
    }

    #[test]
    fn delay_echoes_at_the_set_time() {
        let mut fx = Fx::new(SR);
        let p = FxPatch {
            delay_on: true,
            delay_sync: false,
            delay_ms: 250.0,
            delay_pingpong: false,
            delay_mix: 0.5,
            delay_tone: 20000.0,
            ..FxPatch::default()
        };
        // let the enable fade-in settle, then a click
        let (l, _) = run(&mut fx, &p, |i| if i == 4800 { 1.0 } else { 0.0 }, 48000);
        let echo = 4800 + 12000;
        let peak = l[echo - 50..echo + 50].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let between = l[4900..echo - 200].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.2 && between < 0.01, "echo {peak}, between {between}");
    }

    #[test]
    fn reverb_tail_rings_after_input_stops() {
        let mut fx = Fx::new(SR);
        let p = FxPatch { reverb_on: true, reverb_mix: 0.5, ..FxPatch::default() };
        let (l, _) = run(&mut fx, &p, |i| if i < 4800 { ((i as f32) * 0.05).sin() * 0.5 } else { 0.0 }, 48000);
        let tail = l[24000..26400].iter().map(|x| x * x).sum::<f32>();
        assert!(tail > 1e-4, "tail energy {tail}");
    }

    #[test]
    fn toggling_does_not_click() {
        // a quiet sine through each effect, switched on halfway: the biggest step stays near the sine's own
        let sine = |i: usize| (i as f32 * 440.0 / SR * std::f32::consts::TAU).sin() * 0.3;
        let max_step = 0.3 * 440.0 / SR * std::f32::consts::TAU;
        for which in 0..5 {
            let mut fx = Fx::new(SR);
            let off = FxPatch { dist_drive: 1.0, dist_mode: DistMode::Hard, eq_low: 15.0, eq_high: -15.0, ..FxPatch::default() };
            let mut on = off;
            match which {
                0 => on.dist_on = true,
                1 => on.chorus_on = true,
                2 => on.delay_on = true,
                3 => on.reverb_on = true,
                _ => on.eq_on = true,
            }
            let (a, _) = run(&mut fx, &off, sine, 4800);
            let (b, _) = run(&mut fx, &on, |i| sine(i + 4800), 4800);
            let joined: Vec<f32> = a.into_iter().chain(b).collect();
            // a click is a jump at the switch; afterwards the effect may legitimately have sharp edges (hard clip)
            let worst = joined[4800 - 24..4800 + 48].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
            assert!(worst < max_step * 3.0, "effect {which}: step {worst}");
        }
    }

    #[test]
    fn eq_low_shelf_boosts_bass() {
        let mut fx = Fx::new(SR);
        let p = FxPatch { eq_on: true, eq_low: 12.0, ..FxPatch::default() };
        let (l, _) = run(&mut fx, &p, |i| (i as f32 * 50.0 / SR * std::f32::consts::TAU).sin(), 48000);
        let peak = l[24000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let db = 20.0 * peak.log10();
        assert!((db - 12.0).abs() < 1.5, "{db} dB");
    }
}
