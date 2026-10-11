//! Host-visible parameters. IDs are permanent: never rename or remove one once shipped, only add.

use crate::dsp::filter::FilterMode;
use crate::voice::{EnvPatch, OscPatch, Patch, VoiceMode};
use nih_plug::prelude::*;
use std::sync::Arc;

#[derive(Enum, PartialEq, Clone, Copy)]
pub enum Table {
    #[id = "basic"]
    #[name = "Basic Shapes"]
    BasicShapes,
    #[id = "pwm"]
    #[name = "PWM Sweep"]
    Pwm,
    #[id = "formant"]
    #[name = "Formant Sweep"]
    Formant,
    #[id = "harmonics"]
    #[name = "Harmonic Build"]
    Harmonics,
}

#[derive(Enum, PartialEq, Clone, Copy)]
pub enum SubShape {
    #[id = "sine"]
    Sine,
    #[id = "tri"]
    Triangle,
    #[id = "saw"]
    Saw,
    #[id = "square"]
    Square,
}

#[derive(Enum, PartialEq, Clone, Copy)]
pub enum FilterType {
    #[id = "lp12"]
    #[name = "LP 12"]
    Lp12,
    #[id = "lp24"]
    #[name = "LP 24"]
    Lp24,
    #[id = "hp12"]
    #[name = "HP 12"]
    Hp12,
    #[id = "hp24"]
    #[name = "HP 24"]
    Hp24,
    #[id = "bp12"]
    #[name = "BP 12"]
    Bp12,
    #[id = "bp24"]
    #[name = "BP 24"]
    Bp24,
}

#[derive(Enum, PartialEq, Clone, Copy)]
pub enum Mode {
    #[id = "poly"]
    Poly,
    #[id = "mono"]
    Mono,
    #[id = "legato"]
    Legato,
}

const SMOOTH_MS: f32 = 20.0;

fn percent(name: String, default: f32, min: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min, max: 1.0 })
        .with_smoother(SmoothingStyle::Linear(SMOOTH_MS))
        .with_unit("%")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

fn seconds(name: String, default: f32, max: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Skewed { min: 0.0, max, factor: FloatRange::skew_factor(-2.0) })
        .with_value_to_string(Arc::new(|v| if v < 1.0 { format!("{:.0} ms", v * 1000.0) } else { format!("{v:.2} s") }))
        .with_string_to_value(Arc::new(|s| {
            let s = s.trim().to_ascii_lowercase();
            match s.strip_suffix("ms") {
                Some(ms) => ms.trim().parse::<f32>().ok().map(|v| v / 1000.0),
                None => s.trim_end_matches('s').trim().parse().ok(),
            }
        }))
}

#[derive(Params)]
pub struct OscParams {
    #[id = "table"]
    pub table: EnumParam<Table>,
    #[id = "pos"]
    pub position: FloatParam,
    #[id = "level"]
    pub level: FloatParam,
    #[id = "pan"]
    pub pan: FloatParam,
    #[id = "oct"]
    pub octave: IntParam,
    #[id = "semi"]
    pub semi: IntParam,
    #[id = "fine"]
    pub fine: FloatParam,
    #[id = "unison"]
    pub unison: IntParam,
    #[id = "detune"]
    pub detune: FloatParam,
    #[id = "blend"]
    pub blend: FloatParam,
    #[id = "width"]
    pub width: FloatParam,
    #[id = "phase"]
    pub phase: FloatParam,
    #[id = "rand"]
    pub random_phase: FloatParam,
    #[id = "filter"]
    pub to_filter: BoolParam,
}

impl OscParams {
    fn new(name: &str, d: &OscPatch) -> Self {
        Self {
            table: EnumParam::new(format!("{name} Table"), Table::BasicShapes),
            position: percent(format!("{name} Position"), d.position, 0.0),
            level: percent(format!("{name} Level"), d.level, 0.0),
            pan: FloatParam::new(format!("{name} Pan"), d.pan, FloatRange::Linear { min: -1.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(SMOOTH_MS))
                .with_value_to_string(formatters::v2s_f32_panning())
                .with_string_to_value(formatters::s2v_f32_panning()),
            octave: IntParam::new(format!("{name} Octave"), d.octave, IntRange::Linear { min: -4, max: 4 }),
            semi: IntParam::new(format!("{name} Semi"), d.semi, IntRange::Linear { min: -12, max: 12 }).with_unit(" st"),
            fine: FloatParam::new(format!("{name} Fine"), d.fine, FloatRange::Linear { min: -100.0, max: 100.0 })
                .with_unit(" ct")
                .with_step_size(0.1),
            unison: IntParam::new(format!("{name} Unison"), d.unison as i32, IntRange::Linear { min: 1, max: 16 }),
            detune: percent(format!("{name} Detune"), d.detune, 0.0),
            blend: percent(format!("{name} Blend"), d.blend, 0.0),
            width: percent(format!("{name} Width"), d.width, 0.0),
            phase: percent(format!("{name} Phase"), d.phase, 0.0),
            random_phase: percent(format!("{name} Random Phase"), d.random_phase, 0.0),
            to_filter: BoolParam::new(format!("{name} To Filter"), d.to_filter),
        }
    }

    fn patch(&self, n: u32) -> OscPatch {
        OscPatch {
            table: self.table.value().to_index(),
            position: self.position.smoothed.next_step(n),
            level: self.level.smoothed.next_step(n),
            pan: self.pan.smoothed.next_step(n),
            octave: self.octave.value(),
            semi: self.semi.value(),
            fine: self.fine.value(),
            unison: self.unison.value() as usize,
            detune: self.detune.smoothed.next_step(n),
            blend: self.blend.smoothed.next_step(n),
            width: self.width.smoothed.next_step(n),
            phase: self.phase.value(),
            random_phase: self.random_phase.value(),
            to_filter: self.to_filter.value(),
        }
    }
}

#[derive(Params)]
pub struct EnvParams {
    #[id = "a"]
    pub attack: FloatParam,
    #[id = "d"]
    pub decay: FloatParam,
    #[id = "s"]
    pub sustain: FloatParam,
    #[id = "r"]
    pub release: FloatParam,
}

impl EnvParams {
    fn new(name: &str, d: &EnvPatch) -> Self {
        Self {
            attack: seconds(format!("{name} Attack"), d.attack, 10.0),
            decay: seconds(format!("{name} Decay"), d.decay, 10.0),
            sustain: percent(format!("{name} Sustain"), d.sustain, 0.0),
            release: seconds(format!("{name} Release"), d.release, 10.0),
        }
    }

    fn patch(&self, n: u32) -> EnvPatch {
        EnvPatch {
            attack: self.attack.value(),
            decay: self.decay.value(),
            sustain: self.sustain.smoothed.next_step(n),
            release: self.release.value(),
        }
    }
}

#[derive(Params)]
pub struct MorphwaveParams {
    /// GUI window size. Persist keys are permanent too.
    #[persist = "editor-state"]
    pub editor_state: Arc<nih_plug_egui::EguiState>,

    #[nested(id_prefix = "osc_a", group = "Osc A")]
    pub osc_a: OscParams,
    #[nested(id_prefix = "osc_b", group = "Osc B")]
    pub osc_b: OscParams,

    #[id = "sub_shape"]
    pub sub_shape: EnumParam<SubShape>,
    #[id = "sub_oct"]
    pub sub_octave: IntParam,
    #[id = "sub_level"]
    pub sub_level: FloatParam,
    #[id = "sub_filter"]
    pub sub_to_filter: BoolParam,

    #[id = "noise_level"]
    pub noise_level: FloatParam,
    #[id = "noise_filter"]
    pub noise_to_filter: BoolParam,

    #[id = "filt_on"]
    pub filter_on: BoolParam,
    #[id = "filt_type"]
    pub filter_type: EnumParam<FilterType>,
    #[id = "filt_cutoff"]
    pub cutoff: FloatParam,
    #[id = "filt_res"]
    pub resonance: FloatParam,
    #[id = "filt_drive"]
    pub drive: FloatParam,
    #[id = "filt_key"]
    pub keytrack: FloatParam,
    #[id = "filt_env"]
    pub env2_amount: FloatParam,

    #[nested(id_prefix = "env1", group = "Env 1")]
    pub env1: EnvParams,
    #[nested(id_prefix = "env2", group = "Env 2")]
    pub env2: EnvParams,

    #[id = "voice_mode"]
    pub voice_mode: EnumParam<Mode>,
    #[id = "glide"]
    pub glide: FloatParam,
    #[id = "bend_range"]
    pub bend_range: IntParam,

    // Shipped in v0.1.
    #[id = "master"]
    pub master: FloatParam,
}

impl Default for MorphwaveParams {
    fn default() -> Self {
        let d = Patch::default();
        Self {
            editor_state: crate::editor::default_state(),
            osc_a: OscParams::new("Osc A", &d.osc[0]),
            osc_b: OscParams::new("Osc B", &d.osc[1]),

            sub_shape: EnumParam::new("Sub Shape", SubShape::Sine),
            sub_octave: IntParam::new("Sub Octave", d.sub_octave, IntRange::Linear { min: -4, max: 4 }),
            sub_level: percent("Sub Level".into(), d.sub_level, 0.0),
            sub_to_filter: BoolParam::new("Sub To Filter", d.sub_to_filter),

            noise_level: percent("Noise Level".into(), d.noise_level, 0.0),
            noise_to_filter: BoolParam::new("Noise To Filter", d.noise_to_filter),

            filter_on: BoolParam::new("Filter On", d.filter_on),
            filter_type: EnumParam::new("Filter Type", FilterType::Lp12),
            cutoff: FloatParam::new(
                "Cutoff",
                d.cutoff,
                FloatRange::Skewed { min: 20.0, max: 20000.0, factor: FloatRange::skew_factor(-2.0) },
            )
            .with_smoother(SmoothingStyle::Logarithmic(SMOOTH_MS))
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(1))
            .with_string_to_value(formatters::s2v_f32_hz_then_khz()),
            resonance: percent("Resonance".into(), d.resonance, 0.0),
            drive: percent("Drive".into(), d.drive, 0.0),
            keytrack: percent("Key Track".into(), d.keytrack, 0.0),
            env2_amount: percent("Env 2 > Cutoff".into(), d.env2_amount, -1.0),

            env1: EnvParams::new("Env 1", &d.env[0]),
            env2: EnvParams::new("Env 2", &d.env[1]),

            voice_mode: EnumParam::new("Voice Mode", Mode::Poly),
            glide: seconds("Glide".into(), d.glide, 5.0),
            bend_range: IntParam::new("Bend Range", d.bend_range as i32, IntRange::Linear { min: 0, max: 48 })
                .with_unit(" st"),

            master: FloatParam::new(
                "Master",
                d.master,
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(6.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 6.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}

impl MorphwaveParams {
    /// Snapshot for the next `n` samples; advances the smoothers by `n`.
    pub fn patch(&self, n: u32) -> Patch {
        Patch {
            osc: [self.osc_a.patch(n), self.osc_b.patch(n)],
            sub_shape: self.sub_shape.value().to_index(),
            sub_octave: self.sub_octave.value(),
            sub_level: self.sub_level.smoothed.next_step(n),
            sub_to_filter: self.sub_to_filter.value(),
            noise_level: self.noise_level.smoothed.next_step(n),
            noise_to_filter: self.noise_to_filter.value(),
            filter_on: self.filter_on.value(),
            filter_mode: FilterMode::ALL[self.filter_type.value().to_index()],
            cutoff: self.cutoff.smoothed.next_step(n),
            resonance: self.resonance.smoothed.next_step(n),
            drive: self.drive.smoothed.next_step(n),
            keytrack: self.keytrack.smoothed.next_step(n),
            env2_amount: self.env2_amount.smoothed.next_step(n),
            env: [self.env1.patch(n), self.env2.patch(n)],
            voice_mode: VoiceMode::ALL[self.voice_mode.value().to_index()],
            glide: self.glide.value(),
            bend_range: self.bend_range.value() as f32,
            master: self.master.smoothed.next_step(n),
        }
    }
}
