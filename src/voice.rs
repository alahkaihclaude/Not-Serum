//! Voice allocation and per-voice rendering. Plain Rust (no nih-plug types) so it can be tested and rendered offline.

use crate::dsp::env::Adsr;
use crate::dsp::filter::{Coefs, Filter, FilterMode};
use crate::dsp::fx::{Fx, FxPatch};
use crate::dsp::lfo::{division_beats, LfoShape, LfoState};
use crate::dsp::modmatrix::{self, Dest, ModSlot, Source, NUM_DESTS, NUM_SOURCES};
use crate::dsp::osc::{Osc, Unison, UnisonSettings};
use crate::dsp::wavetable::{self, mip_for, Wavetable};
use crate::dsp::{midi_to_hz, soft_clip, Rng};
use std::sync::Arc;

pub const MAX_VOICES: usize = 16;
/// Control-rate block: pitch, cutoff and unison layout update this often.
pub const BLOCK: usize = 32;
/// Fade-out time for a stolen voice before it restarts.
const STEAL_FADE_S: f32 = 0.003;

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

#[derive(Clone, Copy, Debug)]
pub struct LfoPatch {
    pub shape: LfoShape,
    /// Rate follows host tempo (`division`) instead of `rate_hz`.
    pub sync: bool,
    pub rate_hz: f32,
    /// Index into `dsp::lfo::DIVISIONS`.
    pub division: usize,
    /// Restart at `phase` on every note; otherwise free-running and shared by all voices.
    pub retrigger: bool,
    pub phase: f32,
}

/// Host tempo and position, set once per process call.
#[derive(Clone, Copy, Debug)]
pub struct Transport {
    pub tempo: f32,
    /// Quarter notes per bar.
    pub beats_per_bar: f32,
    /// Song position in quarter notes while playing.
    pub pos_beats: Option<f64>,
}

impl Default for Transport {
    fn default() -> Self {
        Self { tempo: 120.0, beats_per_bar: 4.0, pos_beats: None }
    }
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
    pub env: [EnvPatch; 3],
    pub lfo: [LfoPatch; 4],
    pub macros: [f32; 4],
    pub mods: [ModSlot; 16],
    pub fx: FxPatch,
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
            env: [
                EnvPatch { attack: 0.002, decay: 1.0, sustain: 1.0, release: 0.2 },
                EnvPatch { attack: 0.002, decay: 0.5, sustain: 0.0, release: 0.2 },
                EnvPatch { attack: 0.002, decay: 0.5, sustain: 0.0, release: 0.2 },
            ],
            lfo: [LfoPatch { shape: LfoShape::Sine, sync: false, rate_hz: 2.0, division: 8, retrigger: true, phase: 0.0 }; 4],
            macros: [0.0; 4],
            // Env 2 -> Cutoff is pre-wired at 0 (it replaced the Phase 1 "Env 2 > Cutoff" knob)
            mods: std::array::from_fn(|i| {
                if i == 0 {
                    ModSlot { source: Source::Env2, dest: Dest::Cutoff, ..ModSlot::default() }
                } else {
                    ModSlot::default()
                }
            }),
            fx: FxPatch::default(),
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
    env: [Adsr; 3],
    /// Per-voice LFOs (used in retrigger mode).
    lfo: [LfoState; 4],
    /// Last mod-matrix output, so the effects can follow the newest voice.
    mods: Option<[f32; NUM_DESTS]>,
    /// Polyphonic aftertouch for this voice's note.
    pressure: f32,
    filter: Filter,
}

/// Synth-wide inputs a voice reads for one block.
struct BlockCtx<'a> {
    osc_tables: [&'a Wavetable; 2],
    basic: &'a Wavetable,
    sr: f32,
    bend: f32,
    mod_wheel: f32,
    channel_pressure: f32,
    /// Free-running LFOs shared by all voices.
    global_lfo: [LfoState; 4],
    /// Unmodulated LFO rates in Hz (tempo sync already applied).
    lfo_hz: [f32; 4],
}

pub struct Synth {
    tables: Arc<Vec<Wavetable>>,
    /// Per-osc user tables, used when an osc's table index is past the built-ins.
    custom: [Option<Arc<Wavetable>>; 2],
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
    mod_wheel: f32,
    channel_pressure: f32,
    global_lfo: [LfoState; 4],
    transport: Transport,
    /// Set by `set_transport`; synced free-running LFOs snap to the song position once per process call.
    snap_lfos: bool,
    fx: Fx,
    /// Mod-matrix output of the newest voice, applied to the effects (they process the summed signal).
    fx_mods: [f32; NUM_DESTS],
}

impl Synth {
    /// Builds the built-in tables, so this allocates. Create it off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            tables: Arc::new(wavetable::builtin()),
            custom: [None, None],
            voices: [Voice::default(); MAX_VOICES],
            sample_rate,
            rng: Rng::new(0x1234_5678),
            age: 0,
            held: [0; MAX_VOICES],
            held_len: 0,
            last_pitch: 60.0,
            bend: 0.0,
            mod_wheel: 0.0,
            channel_pressure: 0.0,
            global_lfo: [LfoState::default(); 4],
            transport: Transport::default(),
            snap_lfos: false,
            fx: Fx::new(sample_rate),
            fx_mods: [0.0; NUM_DESTS],
        }
    }

    pub fn set_transport(&mut self, transport: Transport) {
        self.transport = transport;
        self.snap_lfos = true;
    }

    pub fn mod_wheel(&mut self, value: f32) {
        self.mod_wheel = value.clamp(0.0, 1.0);
    }

    pub fn channel_pressure(&mut self, value: f32) {
        self.channel_pressure = value.clamp(0.0, 1.0);
    }

    pub fn poly_pressure(&mut self, note: u8, value: f32) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.note == note) {
            v.pressure = value.clamp(0.0, 1.0);
        }
    }

    /// Shared with the GUI for drawing.
    pub fn tables(&self) -> Arc<Vec<Wavetable>> {
        self.tables.clone()
    }

    /// Swaps in a user table for osc `osc`. Returns the old one: the caller must free it off the audio thread.
    #[must_use]
    pub fn set_custom_table(&mut self, osc: usize, table: Option<Arc<Wavetable>>) -> Option<Arc<Wavetable>> {
        std::mem::replace(&mut self.custom[osc], table)
    }

    /// Reallocates effect buffers when the rate changes, so call it from `initialize`, not the audio thread.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate != self.sample_rate {
            self.fx = Fx::new(sample_rate);
        }
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.voices = [Voice::default(); MAX_VOICES];
        self.held_len = 0;
        self.bend = 0.0;
        self.mod_wheel = 0.0;
        self.channel_pressure = 0.0;
        self.fx.reset();
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
        for (l, p) in v.lfo.iter_mut().zip(&patch.lfo) {
            l.reset(p.phase, &mut self.rng);
        }
        v.env.iter_mut().for_each(Adsr::gate_on);
    }

    /// Adds nothing outside `[0, len)`; overwrites `left` and `right`.
    pub fn render(&mut self, patch: &Patch, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        // Arc clones (refcount bumps, no allocation) so the borrows don't pin `self`
        let tables = self.tables.clone();
        let custom = self.custom.clone();
        // an index past the built-ins means "Custom"; with nothing loaded it falls back to Basic Shapes
        let osc_tables: [&Wavetable; 2] = std::array::from_fn(|i| match tables.get(patch.osc[i].table) {
            Some(t) => t,
            None => custom[i].as_deref().unwrap_or(&tables[0]),
        });
        let t = self.transport;
        let lfo_hz: [f32; 4] = std::array::from_fn(|k| {
            let lfo = &patch.lfo[k];
            if lfo.sync {
                t.tempo / 60.0 / division_beats(lfo.division, t.beats_per_bar)
            } else {
                lfo.rate_hz
            }
        });
        // free-running synced LFOs lock to the song position while the host plays
        if std::mem::take(&mut self.snap_lfos) {
            if let Some(pos) = t.pos_beats {
                for (g, lfo) in self.global_lfo.iter_mut().zip(&patch.lfo) {
                    if lfo.sync && !lfo.retrigger {
                        let cycles = pos / division_beats(lfo.division, t.beats_per_bar) as f64;
                        g.phase = (cycles as f32 + lfo.phase).rem_euclid(1.0);
                    }
                }
            }
        }

        for (l, r) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)) {
            let ctx = BlockCtx {
                osc_tables,
                basic: &tables[0],
                sr: self.sample_rate,
                bend: self.bend * patch.bend_range,
                mod_wheel: self.mod_wheel,
                channel_pressure: self.channel_pressure,
                global_lfo: self.global_lfo,
                lfo_hz,
            };
            for i in 0..MAX_VOICES {
                let v = &self.voices[i];
                if let (Some((note, vel)), true) = (v.pending, v.fade <= 0.0) {
                    self.start(i, patch, note, vel);
                }
                if self.voices[i].active {
                    self.voices[i].render(patch, &ctx, &mut self.rng, l, r);
                }
            }
            for (g, hz) in self.global_lfo.iter_mut().zip(lfo_hz) {
                g.advance(hz * l.len() as f32 / self.sample_rate, &mut self.rng);
            }
            // effects follow the newest sounding voice; with none sounding they keep the last values (tails)
            if let Some(m) = self.voices.iter().filter(|v| v.active).max_by_key(|v| v.age).and_then(|v| v.mods) {
                self.fx_mods = m;
            }
            self.fx.process(&patch.fx, &self.fx_mods, &self.transport, l, r);
            let gain = patch.master;
            for (l, r) in l.iter_mut().zip(r.iter_mut()) {
                *l = soft_clip(*l * gain);
                *r = soft_clip(*r * gain);
            }
        }
    }
}

impl Voice {
    /// Modulation sources at the start of this block, indexed by `Source as usize`.
    fn sources(&self, p: &Patch, ctx: &BlockCtx) -> [f32; NUM_SOURCES] {
        let mut s = [0.0; NUM_SOURCES];
        for k in 0..3 {
            s[Source::Env1 as usize + k] = self.env[k].level();
        }
        for k in 0..4 {
            let state = if p.lfo[k].retrigger { &self.lfo[k] } else { &ctx.global_lfo[k] };
            s[Source::Lfo1 as usize + k] = state.value(p.lfo[k].shape);
        }
        s[Source::Velocity as usize] = self.velocity;
        s[Source::ModWheel as usize] = ctx.mod_wheel;
        s[Source::Aftertouch as usize] = self.pressure.max(ctx.channel_pressure);
        s[Source::Note as usize] = ((self.note as f32 - 60.0) / 60.0).clamp(-1.0, 1.0);
        for k in 0..4 {
            s[Source::Macro1 as usize + k] = p.macros[k];
        }
        s
    }

    fn render(&mut self, p: &Patch, ctx: &BlockCtx, rng: &mut Rng, out_l: &mut [f32], out_r: &mut [f32]) {
        let (n, sr) = (out_l.len(), ctx.sr);
        for (e, s) in self.env.iter_mut().zip(&p.env) {
            e.set(s.attack, s.decay, s.sustain, s.release, sr);
        }

        // shortcut: modulation is control-rate (once per BLOCK), fine for envelopes and LFOs up to ~50 Hz
        let m = modmatrix::evaluate(&p.mods, &self.sources(p, ctx));
        self.mods = Some(m);
        let md = |d: Dest| m[d as usize];
        for k in 0..4 {
            if p.lfo[k].retrigger {
                let hz = ctx.lfo_hz[k] * (m[Dest::Lfo1Rate as usize + k] * modmatrix::LFO_RATE_OCTAVES).exp2();
                self.lfo[k].advance(hz * n as f32 / sr, rng);
            }
        }

        // glide: exponential approach, ~99% of the way after `glide` seconds
        self.pitch = if p.glide > 0.0 {
            let k = 1.0 - (-4.6 * n as f32 / (p.glide * sr)).exp();
            self.pitch + (self.target_pitch - self.pitch) * k
        } else {
            self.target_pitch
        };
        let pitch = self.pitch + ctx.bend + md(Dest::Pitch) * modmatrix::PITCH_SEMIS;

        let osc_setup: [Option<(&Wavetable, f32, Unison)>; 2] = std::array::from_fn(|i| {
            let o = &p.osc[i];
            let om = &m[Dest::osc_base(i)..][..7]; // position, level, pan, pitch, detune, blend, width
            let level = (o.level + om[1]).clamp(0.0, 1.0);
            (level > 0.0).then(|| {
                let table = ctx.osc_tables[i];
                let frame_pos = (o.position + om[0]).clamp(0.0, 1.0) * (table.frames() - 1) as f32;
                let semis = (o.octave * 12 + o.semi) as f32 + o.fine / 100.0 + om[3] * modmatrix::PITCH_SEMIS;
                let settings = UnisonSettings {
                    voices: o.unison,
                    detune: (o.detune + om[4]).clamp(0.0, 1.0),
                    blend: (o.blend + om[5]).clamp(0.0, 1.0),
                    width: (o.width + om[6]).clamp(0.0, 1.0),
                    pan: (o.pan + om[2]).clamp(-1.0, 1.0),
                    level,
                };
                (table, frame_pos, Unison::new(midi_to_hz(pitch + semis), sr, &settings))
            })
        });

        let basic = ctx.basic;
        let sub_level = (p.sub_level + md(Dest::SubLevel)).clamp(0.0, 1.0);
        let noise_level = (p.noise_level + md(Dest::NoiseLevel)).clamp(0.0, 1.0);
        let sub_inc = (midi_to_hz(pitch + (p.sub_octave * 12) as f32) / sr).min(0.49);
        let sub_mip = mip_for(sub_inc);
        let sub_frame = p.sub_shape.min(basic.frames() - 1) as f32;

        let filter_on = p.filter_on;
        let drive = (p.drive + md(Dest::Drive)).clamp(0.0, 1.0);
        let (c1, c2) = if filter_on {
            let octaves = p.keytrack * (pitch - 60.0) / 12.0 + md(Dest::Cutoff) * modmatrix::CUTOFF_OCTAVES;
            let cutoff = p.cutoff * octaves.exp2();
            let resonance = (p.resonance + md(Dest::Resonance)).clamp(0.0, 1.0);
            (Coefs::new(cutoff, resonance, sr), Coefs::new(cutoff, 0.3, sr))
        } else {
            (Coefs::new(1000.0, 0.0, sr), Coefs::new(1000.0, 0.0, sr))
        };
        let volume = (1.0 + md(Dest::Volume)).max(0.0);

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
            if sub_level > 0.0 {
                let s = basic.sample(sub_frame, sub_mip, self.sub_phase) * sub_level;
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
            if noise_level > 0.0 {
                let s = rng.bipolar() * noise_level;
                if p.noise_to_filter {
                    fl += s;
                    fr += s;
                } else {
                    dl += s;
                    dr += s;
                }
            }

            let (fl, fr) = if filter_on { self.filter.process(&c1, &c2, p.filter_mode, drive, fl, fr) } else { (fl, fr) };

            let amp = self.env[0].next() * self.velocity * self.fade * volume;
            self.env[1].next();
            self.env[2].next();
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
        p.mods[0].amount = 1.0; // Env 2 -> Cutoff
        p.mods[1] = ModSlot { source: Source::Lfo1, dest: Dest::Pitch, amount: 1.0, curve: 0.5 };
        p.mods[2] = ModSlot { source: Source::Note, dest: Dest::Resonance, amount: 1.0, curve: 0.0 };
        p.lfo[0].rate_hz = 40.0;
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
    fn velocity_to_volume_can_silence_a_voice() {
        let mut s = Synth::new(48000.0);
        let mut p = Patch::default();
        p.mods[3] = ModSlot { source: Source::Velocity, dest: Dest::Volume, amount: -1.0, curve: 0.0 };
        s.note_on(&p, 60, 1.0);
        let (peak, _) = run(&mut s, &p, 4800);
        assert!(peak < 1e-6, "peak {peak}");
    }

    /// Free-running LFO synced to 1/4 at 120 BPM: a square into Volume (-100%) mutes the first
    /// eighth note and doubles the second.
    #[test]
    fn synced_lfo_follows_host_tempo() {
        let sr = 48000.0;
        let mut s = Synth::new(sr);
        let mut p = Patch::default();
        p.osc[0].random_phase = 0.0;
        p.lfo[0] = LfoPatch { shape: LfoShape::Square, sync: true, division: 8, retrigger: false, ..p.lfo[0] };
        p.mods[3] = ModSlot { source: Source::Lfo1, dest: Dest::Volume, amount: -1.0, curve: 0.0 };
        s.set_transport(Transport { tempo: 120.0, beats_per_bar: 4.0, pos_beats: None });
        s.note_on(&p, 60, 1.0);
        let (mut l, mut r) = (vec![0.0; 48000], vec![0.0; 48000]);
        s.render(&p, &mut l, &mut r);
        let rms = |a: f32, b: f32| {
            let w = &l[(a * sr) as usize..(b * sr) as usize];
            (w.iter().map(|x| x * x).sum::<f32>() / w.len() as f32).sqrt()
        };
        // 1/4 at 120 BPM = 0.5 s cycle: muted 0..0.25 s, loud 0.25..0.5 s, muted again 0.5..0.75 s
        assert!(rms(0.02, 0.23) < 1e-3);
        assert!(rms(0.27, 0.48) > 0.05);
        assert!(rms(0.52, 0.73) < 1e-3);
    }

    #[test]
    fn env3_to_pitch_raises_frequency() {
        let sr = 48000.0;
        let zero_crossings = |p: &Patch| {
            let mut s = Synth::new(sr);
            s.note_on(p, 57, 1.0); // 220 Hz
            let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
            s.render(p, &mut l, &mut r);
            l[480..].windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
        };
        let mut p = Patch::default();
        p.osc[0].position = 0.0; // sine
        p.osc[0].random_phase = 0.0;
        p.env[2] = EnvPatch { attack: 0.0, decay: 10.0, sustain: 1.0, release: 0.1 };
        let base = zero_crossings(&p);
        p.mods[3] = ModSlot { source: Source::Env3, dest: Dest::Pitch, amount: 0.5, curve: 0.0 }; // +12 st
        let up = zero_crossings(&p);
        assert!((up as f32 / base as f32 - 2.0).abs() < 0.1, "{base} -> {up}");
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
