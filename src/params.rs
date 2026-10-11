//! Host-visible parameters. IDs are permanent: never rename or remove one once shipped, only add.

use crate::dsp::filter::FilterMode;
use crate::dsp::lfo::LfoShape;
use crate::dsp::modmatrix::{Dest, ModSlot, Source};
use crate::voice::{EnvPatch, LfoPatch, OscPatch, Patch, VoiceMode};
use crate::wavetable_import::StoredTable;
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
    /// A table loaded from a WAV file. Keep it last: the indices before it map onto `wavetable::builtin()`.
    #[id = "custom"]
    Custom,
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
    /// The loaded WAV wavetable, stored in the project so it opens on other machines.
    #[persist = "wavetable"]
    pub custom: std::sync::Mutex<Option<StoredTable>>,
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
            custom: std::sync::Mutex::new(None),
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

#[derive(Enum, PartialEq, Clone, Copy)]
pub enum LfoShapeParam {
    #[id = "sine"]
    Sine,
    #[id = "tri"]
    Triangle,
    #[id = "saw"]
    Saw,
    #[id = "square"]
    Square,
    #[id = "sh"]
    #[name = "S&H"]
    SampleHold,
}

/// Same order as `dsp::lfo::DIVISIONS`.
#[derive(Enum, PartialEq, Clone, Copy)]
pub enum Division {
    #[id = "1/32"]
    #[name = "1/32"]
    D32,
    #[id = "1/16t"]
    #[name = "1/16T"]
    D16T,
    #[id = "1/16"]
    #[name = "1/16"]
    D16,
    #[id = "1/16d"]
    #[name = "1/16D"]
    D16D,
    #[id = "1/8t"]
    #[name = "1/8T"]
    D8T,
    #[id = "1/8"]
    #[name = "1/8"]
    D8,
    #[id = "1/8d"]
    #[name = "1/8D"]
    D8D,
    #[id = "1/4t"]
    #[name = "1/4T"]
    D4T,
    #[id = "1/4"]
    #[name = "1/4"]
    D4,
    #[id = "1/4d"]
    #[name = "1/4D"]
    D4D,
    #[id = "1/2"]
    #[name = "1/2"]
    D2,
    #[id = "1bar"]
    #[name = "1 bar"]
    Bar1,
    #[id = "2bar"]
    #[name = "2 bars"]
    Bar2,
    #[id = "4bar"]
    #[name = "4 bars"]
    Bar4,
    #[id = "8bar"]
    #[name = "8 bars"]
    Bar8,
}

/// Same order as `dsp::modmatrix::Source::ALL`.
#[derive(Enum, PartialEq, Clone, Copy)]
pub enum ModSource {
    #[id = "none"]
    #[name = "-"]
    None,
    #[id = "env1"]
    #[name = "Env 1"]
    Env1,
    #[id = "env2"]
    #[name = "Env 2"]
    Env2,
    #[id = "env3"]
    #[name = "Env 3"]
    Env3,
    #[id = "lfo1"]
    #[name = "LFO 1"]
    Lfo1,
    #[id = "lfo2"]
    #[name = "LFO 2"]
    Lfo2,
    #[id = "lfo3"]
    #[name = "LFO 3"]
    Lfo3,
    #[id = "lfo4"]
    #[name = "LFO 4"]
    Lfo4,
    #[id = "velocity"]
    Velocity,
    #[id = "modwheel"]
    #[name = "Mod Wheel"]
    ModWheel,
    #[id = "aftertouch"]
    Aftertouch,
    #[id = "note"]
    Note,
    #[id = "macro1"]
    #[name = "Macro 1"]
    Macro1,
    #[id = "macro2"]
    #[name = "Macro 2"]
    Macro2,
    #[id = "macro3"]
    #[name = "Macro 3"]
    Macro3,
    #[id = "macro4"]
    #[name = "Macro 4"]
    Macro4,
}

/// Same order as `dsp::modmatrix::Dest::ALL`. Only append new destinations.
#[derive(Enum, PartialEq, Clone, Copy)]
pub enum ModDest {
    #[id = "none"]
    #[name = "-"]
    None,
    #[id = "a_pos"]
    #[name = "A Position"]
    APosition,
    #[id = "a_level"]
    #[name = "A Level"]
    ALevel,
    #[id = "a_pan"]
    #[name = "A Pan"]
    APan,
    #[id = "a_pitch"]
    #[name = "A Pitch"]
    APitch,
    #[id = "a_detune"]
    #[name = "A Detune"]
    ADetune,
    #[id = "a_blend"]
    #[name = "A Blend"]
    ABlend,
    #[id = "a_width"]
    #[name = "A Width"]
    AWidth,
    #[id = "b_pos"]
    #[name = "B Position"]
    BPosition,
    #[id = "b_level"]
    #[name = "B Level"]
    BLevel,
    #[id = "b_pan"]
    #[name = "B Pan"]
    BPan,
    #[id = "b_pitch"]
    #[name = "B Pitch"]
    BPitch,
    #[id = "b_detune"]
    #[name = "B Detune"]
    BDetune,
    #[id = "b_blend"]
    #[name = "B Blend"]
    BBlend,
    #[id = "b_width"]
    #[name = "B Width"]
    BWidth,
    #[id = "pitch"]
    #[name = "Pitch (all)"]
    Pitch,
    #[id = "sub_level"]
    #[name = "Sub Level"]
    SubLevel,
    #[id = "noise_level"]
    #[name = "Noise Level"]
    NoiseLevel,
    #[id = "cutoff"]
    Cutoff,
    #[id = "res"]
    Resonance,
    #[id = "drive"]
    Drive,
    #[id = "volume"]
    Volume,
    #[id = "lfo1_rate"]
    #[name = "LFO 1 Rate"]
    Lfo1Rate,
    #[id = "lfo2_rate"]
    #[name = "LFO 2 Rate"]
    Lfo2Rate,
    #[id = "lfo3_rate"]
    #[name = "LFO 3 Rate"]
    Lfo3Rate,
    #[id = "lfo4_rate"]
    #[name = "LFO 4 Rate"]
    Lfo4Rate,
}

#[derive(Params)]
pub struct LfoParams {
    #[id = "shape"]
    pub shape: EnumParam<LfoShapeParam>,
    #[id = "sync"]
    pub sync: BoolParam,
    #[id = "rate"]
    pub rate: FloatParam,
    #[id = "div"]
    pub division: EnumParam<Division>,
    #[id = "retrig"]
    pub retrigger: BoolParam,
    #[id = "phase"]
    pub phase: FloatParam,
}

impl LfoParams {
    fn new(name: &str, d: &LfoPatch) -> Self {
        Self {
            shape: EnumParam::new(format!("{name} Shape"), LfoShapeParam::Sine),
            sync: BoolParam::new(format!("{name} Sync"), d.sync),
            rate: FloatParam::new(
                format!("{name} Rate"),
                d.rate_hz,
                FloatRange::Skewed { min: 0.01, max: 40.0, factor: FloatRange::skew_factor(-2.0) },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            division: EnumParam::new(format!("{name} Division"), Division::D4),
            retrigger: BoolParam::new(format!("{name} Retrigger"), d.retrigger),
            phase: percent(format!("{name} Phase"), d.phase, 0.0),
        }
    }

    fn patch(&self) -> LfoPatch {
        LfoPatch {
            shape: LfoShape::ALL[self.shape.value().to_index()],
            sync: self.sync.value(),
            rate_hz: self.rate.value(),
            division: self.division.value().to_index(),
            retrigger: self.retrigger.value(),
            phase: self.phase.value(),
        }
    }
}

#[derive(Params)]
pub struct SlotParams {
    #[id = "src"]
    pub source: EnumParam<ModSource>,
    #[id = "dst"]
    pub dest: EnumParam<ModDest>,
    #[id = "amt"]
    pub amount: FloatParam,
    #[id = "curve"]
    pub curve: FloatParam,
}

impl SlotParams {
    fn new(i: usize, d: &ModSlot) -> Self {
        let n = i + 1;
        Self {
            source: EnumParam::new(format!("Mod {n} Source"), ModSource::from_index(d.source as usize)),
            dest: EnumParam::new(format!("Mod {n} Destination"), ModDest::from_index(d.dest as usize)),
            amount: percent(format!("Mod {n} Amount"), d.amount, -1.0),
            curve: percent(format!("Mod {n} Curve"), d.curve, -1.0),
        }
    }

    fn patch(&self, n: u32) -> ModSlot {
        ModSlot {
            source: Source::ALL[self.source.value().to_index()],
            dest: Dest::ALL[self.dest.value().to_index()],
            amount: self.amount.smoothed.next_step(n),
            curve: self.curve.value(),
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
    /// Retired in v0.5 (Phase 4): Env 2 > Cutoff is now mod slot 1. Kept hidden so old projects load;
    /// `Morphwave::filter_state` moves its value into the matrix.
    #[id = "filt_env"]
    pub env2_amount: FloatParam,

    #[nested(id_prefix = "env1", group = "Env 1")]
    pub env1: EnvParams,
    #[nested(id_prefix = "env2", group = "Env 2")]
    pub env2: EnvParams,
    #[nested(id_prefix = "env3", group = "Env 3")]
    pub env3: EnvParams,

    #[nested(id_prefix = "lfo1", group = "LFO 1")]
    pub lfo1: LfoParams,
    #[nested(id_prefix = "lfo2", group = "LFO 2")]
    pub lfo2: LfoParams,
    #[nested(id_prefix = "lfo3", group = "LFO 3")]
    pub lfo3: LfoParams,
    #[nested(id_prefix = "lfo4", group = "LFO 4")]
    pub lfo4: LfoParams,

    #[id = "macro1"]
    pub macro1: FloatParam,
    #[id = "macro2"]
    pub macro2: FloatParam,
    #[id = "macro3"]
    pub macro3: FloatParam,
    #[id = "macro4"]
    pub macro4: FloatParam,

    /// Mod matrix. Param IDs are `src_1`, `dst_1`, `amt_1`, `curve_1` ... `_16`.
    #[nested(array, group = "Mod")]
    pub mods: [SlotParams; 16],

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
            env2_amount: percent("Env 2 > Cutoff (old)".into(), 0.0, -1.0).hide(),

            env1: EnvParams::new("Env 1", &d.env[0]),
            env2: EnvParams::new("Env 2", &d.env[1]),
            env3: EnvParams::new("Env 3", &d.env[2]),
            lfo1: LfoParams::new("LFO 1", &d.lfo[0]),
            lfo2: LfoParams::new("LFO 2", &d.lfo[1]),
            lfo3: LfoParams::new("LFO 3", &d.lfo[2]),
            lfo4: LfoParams::new("LFO 4", &d.lfo[3]),
            macro1: percent("Macro 1".into(), d.macros[0], 0.0),
            macro2: percent("Macro 2".into(), d.macros[1], 0.0),
            macro3: percent("Macro 3".into(), d.macros[2], 0.0),
            macro4: percent("Macro 4".into(), d.macros[3], 0.0),
            mods: std::array::from_fn(|i| SlotParams::new(i, &d.mods[i])),

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
            env: [self.env1.patch(n), self.env2.patch(n), self.env3.patch(n)],
            lfo: [self.lfo1.patch(), self.lfo2.patch(), self.lfo3.patch(), self.lfo4.patch()],
            macros: [
                self.macro1.smoothed.next_step(n),
                self.macro2.smoothed.next_step(n),
                self.macro3.smoothed.next_step(n),
                self.macro4.smoothed.next_step(n),
            ],
            mods: std::array::from_fn(|i| self.mods[i].patch(n)),
            voice_mode: VoiceMode::ALL[self.voice_mode.value().to_index()],
            glide: self.glide.value(),
            bend_range: self.bend_range.value() as f32,
            master: self.master.smoothed.next_step(n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::lfo::DIVISIONS;
    use crate::voice::VoiceMode;

    /// Host enums are mapped onto DSP lists by index, so their lengths must match.
    #[test]
    fn host_enums_match_dsp_lists() {
        assert_eq!(ModSource::variants().len(), Source::ALL.len());
        assert_eq!(ModDest::variants().len(), Dest::ALL.len());
        assert_eq!(Division::variants().len(), DIVISIONS.len());
        assert_eq!(LfoShapeParam::variants().len(), LfoShape::ALL.len());
        assert_eq!(FilterType::variants().len(), FilterMode::ALL.len());
        assert_eq!(Mode::variants().len(), VoiceMode::ALL.len());
        assert_eq!(Table::variants().len(), crate::dsp::wavetable::builtin().len() + 1); // + Custom
    }

    #[test]
    fn default_params_rebuild_the_default_patch() {
        let p = MorphwaveParams::default().patch(1);
        let d = Patch::default();
        assert_eq!(p.mods[0].source, Source::Env2);
        assert_eq!(p.mods[0].dest, Dest::Cutoff);
        assert_eq!(p.lfo[0].division, d.lfo[0].division);
        assert_eq!(p.env.len(), 3);
    }
}
