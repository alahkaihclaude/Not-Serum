//! Voice allocation and per-voice rendering. Plain Rust (no nih-plug types) so it can be tested and rendered offline.

use crate::dsp::env::Adsr;
use crate::dsp::filter::{Coefs, Filter, FilterMode};
use crate::dsp::osc::{Osc, Unison, UnisonSettings};
use crate::dsp::wavetable::{self, mip_for, Wavetable};
use crate::dsp::{midi_to_hz, soft_clip, Rng};
use std::sync::Arc;

pub const MAX_VOICES: usize = 16;
/// Control-rate block: pitch, cutoff and unison layout update this often.
pub const BLOCK: usize = 32;
/// Fade-out time for a stolen voice before it restarts.
const STEAL_FADE_S: f32 = 0.003;
/// Env 2 at 100% amount moves the cutoff this many octaves.
const ENV_CUTOFF_OCTAVES: f32 = 8.0;

#[derive(Clone, Copy, Debug)]
pub struct OscPatch {
    pub table: usize,
    pub position: f32,
    pub level: f32,
    pub pan: f32,
    pub octave: i32,
    pub semi: i32,
    pub fine: f32,
    pub unison: usize,
    pub detune: f32,
    pub blend: f32,
    pub width: f32,
    pub phase: f32,
    pub random_phase: f32,
    pub to_filter: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct EnvPatch {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceMode {
    Poly,
    Mono,
    Legato,
}

impl VoiceMode {
    /// Same order as the `Voice Mode` parameter.
    pub const ALL: [Self; 3] = [Self::Poly, Self::Mono, Self::Legato];
}

/// Every sound setting the engine reads. The plugin fills it from host parameters once per block.
#[derive(Clone, Copy, Debug)]
pub struct Patch {
    pub osc: [OscPatch; 2],
    /// Index into the Basic Shapes frames: sine, triangle, saw, square.
    pub sub_shape: usize,
    pub sub_octave: i32,
    pub sub_level: f32,
    pub sub_to_filter: bool,
    pub noise_level: f32,
    pub noise_to_filter: bool,
    pub filter_on: bool,
    pub filter_mode: FilterMode,
    pub cutoff: f32,
    pub resonance: f32,
    pub drive: f32,
    pub keytrack: f32,
    pub env2_amount: f32,
    pub env: [EnvPatch; 2],
    pub voice_mode: VoiceMode,
    pub glide: f32,
    pub bend_range: f32,
    pub master: f32,
}

impl Default for Patch {
    /// The "init" sound. Parameter defaults are taken from here.
    fn default() -> Self {
        let osc = OscPatch {
            table: 0,
            position: 2.0 / 3.0, // saw frame of Basic Shapes
            level: 0.75,
            pan: 0.0,
            octave: 0,
            semi: 0,
            fine: 0.0,
            unison: 1,
            detune: 0.25,
            blend: 0.75,
            width: 1.0,
            phase: 0.0,
            random_phase: 1.0,
            to_filter: true,
        };
        Self {
            osc: [osc, OscPatch { level: 0.0, ..osc }],
            sub_shape: 0,
            sub_octave: -1,
            sub_level: 0.0,
            sub_to_filter: true,
            noise_level: 0.0,
            noise_to_filter: true,
            filter_on: false,
            filter_mode: FilterMode::Lp12,
            cutoff: 20000.0,
            resonance: 0.1,
            drive: 0.0,
            keytrack: 0.0,
            env2_amount: 0.0,
            env: [
                EnvPatch { attack: 0.002, decay: 1.0, sustain: 1.0, release: 0.2 },
                EnvPatch { attack: 0.002, decay: 0.5, sustain: 0.0, release: 0.2 },
            ],
            voice_mode: VoiceMode::Poly,
            glide: 0.0,
            bend_range: 2.0,
            master: 0.25, // -12 dB: headroom for chords before the output soft-clip
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    released: bool,
    note: u8,
    velocity: f32,
    age: u64,
    pitch: f32,
    target_pitch: f32,
    /// 1 normally; ramps to 0 when stolen, then `pending` starts.
    fade: f32,
    pending: Option<(u8, f32)>,
    osc: [Osc; 2],
    sub_phase: f32,
    env: [Adsr; 2],
    filter: Filter,
}

pub struct Synth {
    tables: Arc<Vec<Wavetable>>,
    voices: [Voice; MAX_VOICES],
    sample_rate: f32,
    rng: Rng,
    age: u64,
    /// Held notes, oldest first, for mono/legato note priority.
    held: [u8; MAX_VOICES],
    held_len: usize,
    last_pitch: f32,
    /// Pitch bend in [-1, 1].
    bend: f32,
}

impl Synth {
    /// Builds the built-in tables, so this allocates. Create it off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            tables: Arc::new(wavetable::builtin()),
            voices: [Voice::default(); MAX_VOICES],
            sample_rate,
            rng: Rng::new(0x1234_5678),
            age: 0,
            held: [0; MAX_VOICES],
            held_len: 0,
            last_pitch: 60.0,
            bend: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.voices = [Voice::default(); MAX_VOICES];
        self.held_len = 0;
        self.bend = 0.0;
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    pub fn pitch_bend(&mut self, value: f32) {
        self.bend = value.clamp(-1.0, 1.0);
    }

    pub fn note_on(&mut self, patch: &Patch, note: u8, velocity: f32) {
        if self.held_len == MAX_VOICES {
            self.held.copy_within(1.., 0);
            self.held_len -= 1;
        }
        self.held[self.held_len] = note;
        self.held_len += 1;

        if patch.voice_mode == VoiceMode::Poly {
            let i = self.pick_voice();
            if self.voices[i].active {
                self.voices[i].pending = Some((note, velocity));
            } else {
                self.start(i, patch, note, velocity);
            }
            return;
        }

        let v = &mut self.voices[0];
        if v.active && !v.released && v.pending.is_none() {
            v.note = note;
            v.velocity = velocity;
            v.target_pitch = note as f32;
            if patch.voice_mode == VoiceMode::Mono {
                v.env.iter_mut().for_each(Adsr::gate_on);
            }
            self.last_pitch = note as f32;
        } else if v.active {
            v.pending = Some((note, velocity));
        } else {
            self.start(0, patch, note, velocity);
        }
    }

    pub fn note_off(&mut self, patch: &Patch, note: u8) {
        if let Some(pos) = self.held[..self.held_len].iter().position(|&n| n == note) {
            self.held.copy_within(pos + 1..self.held_len, pos);
            self.held_len -= 1;
        }

        if patch.voice_mode != VoiceMode::Poly && self.voices[0].note == note && self.held_len > 0 {
            // fall back to the most recent still-held note without retriggering
            let back = self.held[self.held_len - 1];
            let v = &mut self.voices[0];
            v.note = back;
            v.target_pitch = back as f32;
            return;
        }

        for v in &mut self.voices {
            if v.pending.map(|(n, _)| n) == Some(note) {
                // the stolen note's replacement was released before it started: let the old note finish
                v.pending = None;
                v.released = true;
                v.env.iter_mut().for_each(Adsr::gate_off);
            } else if v.active && !v.released && v.note == note && v.pending.is_none() {
                v.released = true;
                v.env.iter_mut().for_each(Adsr::gate_off);
            }
        }
    }

    /// Host "choke": silence the note immediately.
    pub fn choke(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.note == note {
                *v = Voice::default();
            }
        }
    }

    /// Free voice first, else the oldest released one, else the oldest.
    fn pick_voice(&self) -> usize {
        let score = |v: &Voice| match (v.active, v.released) {
            (false, _) => (0, 0),
            (true, true) => (1, v.age),
            (true, false) => (2, v.age),
        };
        (0..MAX_VOICES).min_by_key(|&i| score(&self.voices[i])).unwrap()
    }

    fn start(&mut self, i: usize, patch: &Patch, note: u8, velocity: f32) {
        self.age += 1;
        let pitch = note as f32;
        let start_pitch = if patch.glide > 0.0 { self.last_pitch } else { pitch };
        self.last_pitch = pitch;
        let v = &mut self.voices[i];
        *v = Voice {
            active: true,
            note,
            velocity,
            age: self.age,
            pitch: start_pitch,
            target_pitch: pitch,
            fade: 1.0,
            ..Voice::default()
        };
        for (o, p) in v.osc.iter_mut().zip(&patch.osc) {
            o.reset(p.phase, p.random_phase, &mut self.rng);
        }
        v.env.iter_mut().for_each(Adsr::gate_on);
    }

    /// Adds nothing outside `[0, len)`; overwrites `left` and `right`.
    pub fn render(&mut self, patch: &Patch, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        for (l, r) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)) {
            for i in 0..MAX_VOICES {
                let v = &self.voices[i];
                if let (Some((note, vel)), true) = (v.pending, v.fade <= 0.0) {
                    self.start(i, patch, note, vel);
                }
                if self.voices[i].active {
                    let bend = self.bend * patch.bend_range;
                    self.voices[i].render(patch, &self.tables, self.sample_rate, bend, &mut self.rng, l, r);
                }
            }
            let gain = patch.master;
            for (l, r) in l.iter_mut().zip(r.iter_mut()) {
                *l = soft_clip(*l * gain);
                *r = soft_clip(*r * gain);
            }
        }
    }
}

impl Voice {
    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        p: &Patch,
        tables: &[Wavetable],
        sr: f32,
        bend: f32,
        rng: &mut Rng,
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        let n = out_l.len();
        for (e, s) in self.env.iter_mut().zip(&p.env) {
            e.set(s.attack, s.decay, s.sustain, s.release, sr);
        }

        // glide: exponential approach, ~99% of the way after `glide` seconds
        self.pitch = if p.glide > 0.0 {
            let k = 1.0 - (-4.6 * n as f32 / (p.glide * sr)).exp();
            self.pitch + (self.target_pitch - self.pitch) * k
        } else {
            self.target_pitch
        };
        let pitch = self.pitch + bend;

        let osc_setup: [Option<(&Wavetable, f32, Unison)>; 2] = std::array::from_fn(|i| {
            let o = &p.osc[i];
            (o.level > 0.0).then(|| {
                let table = &tables[o.table.min(tables.len() - 1)];
                let frame_pos = o.position.clamp(0.0, 1.0) * (table.frames() - 1) as f32;
                let freq = midi_to_hz(pitch + (o.octave * 12 + o.semi) as f32 + o.fine / 100.0);
                let settings = UnisonSettings {
                    voices: o.unison,
                    detune: o.detune,
                    blend: o.blend,
                    width: o.width,
                    pan: o.pan,
                    level: o.level,
                };
                (table, frame_pos, Unison::new(freq, sr, &settings))
            })
        });

        let basic = &tables[0];
        let sub_inc = (midi_to_hz(pitch + (p.sub_octave * 12) as f32) / sr).min(0.49);
        let sub_mip = mip_for(sub_inc);
        let sub_frame = p.sub_shape.min(basic.frames() - 1) as f32;

        let filter_on = p.filter_on;
        let (c1, c2) = if filter_on {
            let octaves = p.keytrack * (pitch - 60.0) / 12.0 + p.env2_amount * ENV_CUTOFF_OCTAVES * self.env[1].level();
            let cutoff = p.cutoff * octaves.exp2();
            (Coefs::new(cutoff, p.resonance, sr), Coefs::new(cutoff, 0.3, sr))
        } else {
            (Coefs::new(1000.0, 0.0, sr), Coefs::new(1000.0, 0.0, sr))
        };

        let fade_step = 1.0 / (STEAL_FADE_S * sr);
        for j in 0..n {
            let (mut fl, mut fr, mut dl, mut dr) = (0.0, 0.0, 0.0, 0.0);
            for (i, setup) in osc_setup.iter().enumerate() {
                if let Some((table, pos, u)) = setup {
                    let (l, r) = self.osc[i].tick(table, *pos, u);
                    if p.osc[i].to_filter {
                        fl += l;
                        fr += r;
                    } else {
                        dl += l;
                        dr += r;
                    }
                }
            }
            if p.sub_level > 0.0 {
                let s = basic.sample(sub_frame, sub_mip, self.sub_phase) * p.sub_level;
                if p.sub_to_filter {
                    fl += s;
                    fr += s;
                } else {
                    dl += s;
                    dr += s;
                }
            }
            self.sub_phase += sub_inc;
            if self.sub_phase >= 1.0 {
                self.sub_phase -= 1.0;
            }
            if p.noise_level > 0.0 {
                let s = rng.bipolar() * p.noise_level;
                if p.noise_to_filter {
                    fl += s;
                    fr += s;
                } else {
                    dl += s;
                    dr += s;
                }
            }

            let (fl, fr) = if filter_on { self.filter.process(&c1, &c2, p.filter_mode, p.drive, fl, fr) } else { (fl, fr) };

            let amp = self.env[0].next() * self.velocity * self.fade;
            self.env[1].next();
            if self.pending.is_some() {
                self.fade = (self.fade - fade_step).max(0.0);
            }
            out_l[j] += (fl + dl) * amp;
            out_r[j] += (fr + dr) * amp;
        }

        if self.env[0].is_idle() && self.pending.is_none() {
            self.active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut Synth, p: &Patch, samples: usize) -> (f32, bool) {
        let (mut l, mut r) = (vec![0.0; samples], vec![0.0; samples]);
        s.render(p, &mut l, &mut r);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, x| m.max(x.abs()));
        (peak, l.iter().chain(&r).all(|x| x.is_finite()))
    }

    #[test]
    fn notes_sound_then_all_voices_go_idle() {
        let mut s = Synth::new(48000.0);
        let p = Patch::default();
        for n in [60, 64, 67] {
            s.note_on(&p, n, 1.0);
        }
        let (peak, finite) = run(&mut s, &p, 4800);
        assert!(peak > 0.1 && finite);
        assert_eq!(s.active_voices(), 3);
        for n in [60, 64, 67] {
            s.note_off(&p, n);
        }
        run(&mut s, &p, 48000);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn extreme_settings_stay_finite_and_bounded() {
        let mut s = Synth::new(44100.0);
        let mut p = Patch::default();
        for o in &mut p.osc {
            o.level = 1.0;
            o.unison = 16;
            o.detune = 1.0;
            o.octave = 4;
            o.semi = 12;
        }
        p.sub_level = 1.0;
        p.noise_level = 1.0;
        p.filter_on = true;
        p.filter_mode = FilterMode::Bp24;
        p.resonance = 1.0;
        p.drive = 1.0;
        p.env2_amount = 1.0;
        p.keytrack = 1.0;
        p.master = 4.0;
        for n in 0..=127 {
            s.note_on(&p, n, 1.0);
        }
        let (peak, finite) = run(&mut s, &p, 44100);
        assert!(finite && peak <= 1.0);
        assert_eq!(s.active_voices(), MAX_VOICES);
    }

    #[test]
    fn stealing_restarts_voice_with_new_note() {
        let mut s = Synth::new(48000.0);
        let p = Patch::default();
        for n in 40..40 + MAX_VOICES as u8 {
            s.note_on(&p, n, 1.0);
        }
        run(&mut s, &p, 480);
        s.note_on(&p, 100, 1.0);
        run(&mut s, &p, 960); // longer than the steal fade
        assert!(s.voices.iter().any(|v| v.active && v.note == 100));
        assert!(!s.voices.iter().any(|v| v.active && v.note == 40));
    }

    #[test]
    fn legato_glides_without_new_voice() {
        let mut s = Synth::new(48000.0);
        let p = Patch { voice_mode: VoiceMode::Legato, glide: 0.1, ..Patch::default() };
        s.note_on(&p, 60, 1.0);
        run(&mut s, &p, 480);
        s.note_on(&p, 72, 1.0);
        run(&mut s, &p, 480);
        assert_eq!(s.active_voices(), 1);
        let pitch = s.voices[0].pitch;
        assert!(pitch > 60.0 && pitch < 72.0, "mid-glide pitch {pitch}");
        s.note_off(&p, 72);
        assert_eq!(s.voices[0].target_pitch, 60.0); // back to the held note
    }
}
