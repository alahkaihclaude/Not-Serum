use nih_plug::prelude::*;
use nih_plug::wrapper::state::ParamValue;
use std::sync::Arc;

pub mod dsp;
mod editor;
pub mod params;
pub mod voice;
pub mod wavetable_import;

use params::MorphwaveParams;
use voice::{Patch, Synth, Transport, BLOCK};
use wavetable_import::TableSlot;

pub struct Morphwave {
    params: Arc<MorphwaveParams>,
    synth: Synth,
    /// Last block's settings, used when a note event arrives between blocks.
    patch: Patch,
    /// Custom wavetable hand-off for Osc A and Osc B (loader thread / GUI / audio thread).
    slots: Arc<[TableSlot; 2]>,
}

impl Default for Morphwave {
    fn default() -> Self {
        Self {
            params: Arc::new(MorphwaveParams::default()),
            synth: Synth::new(44100.0),
            patch: Patch::default(),
            slots: Arc::new(Default::default()),
        }
    }
}

impl Plugin for Morphwave {
    const NAME: &'static str = "Morphwave";
    const VENDOR: &'static str = "Morphwave";
    const URL: &'static str = "https://github.com/alahkaihclaude/Not-Serum";
    const EMAIL: &'static str = "alahkaihclaude@gmail.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    // MidiCCs is needed for pitch bend.
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    /// Migrates state saved by older versions before it's applied.
    fn filter_state(state: &mut PluginState) {
        // Before v0.5 there was no mod matrix and "Env 2 > Cutoff" (filt_env) was a fixed knob.
        // Move it into slot 1 (Env 2 -> Cutoff, same 8-octave scale) so old projects sound the same.
        if !state.params.contains_key("amt_1") {
            if let Some(ParamValue::F32(amount)) = state.params.get("filt_env").cloned() {
                state.params.insert("src_1".into(), ParamValue::String("env2".into()));
                state.params.insert("dst_1".into(), ParamValue::String("cutoff".into()));
                state.params.insert("amt_1".into(), ParamValue::F32(amount));
                state.params.insert("filt_env".into(), ParamValue::F32(0.0));
            }
        }
    }

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.synth.tables(), self.slots.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.synth.set_sample_rate(buffer_config.sample_rate);
        // Runs after a state load too, so this is where saved custom wavetables come back.
        for (i, osc) in [&self.params.osc_a, &self.params.osc_b].into_iter().enumerate() {
            let table = self.slots[i].restore(&osc.custom);
            drop(self.synth.set_custom_table(i, table));
        }
        true
    }

    fn reset(&mut self) {
        self.synth.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(table) = slot.take_new() {
                if let Some(old) = self.synth.set_custom_table(i, Some(table)) {
                    slot.retire(old);
                }
            }
        }

        let t = context.transport();
        let beats_per_bar = match (t.time_sig_numerator, t.time_sig_denominator) {
            (Some(num), Some(den)) if num > 0 && den > 0 => num as f32 * 4.0 / den as f32,
            _ => 4.0,
        };
        self.synth.set_transport(Transport {
            tempo: t.tempo.unwrap_or(120.0) as f32,
            beats_per_bar,
            pos_beats: if t.playing { t.pos_beats() } else { None },
        });

        let num_samples = buffer.samples();
        let (left, right) = buffer.as_slice().split_at_mut(1);
        let (left, right) = (&mut *left[0], &mut *right[0]);

        let mut next_event = context.next_event();
        let mut start = 0;
        while start < num_samples {
            while let Some(event) = next_event {
                if event.timing() as usize > start {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { note, velocity, .. } => self.synth.note_on(&self.patch, note, velocity),
                    NoteEvent::NoteOff { note, .. } => self.synth.note_off(&self.patch, note),
                    NoteEvent::Choke { note, .. } => self.synth.choke(note),
                    NoteEvent::MidiPitchBend { value, .. } => self.synth.pitch_bend(value * 2.0 - 1.0),
                    NoteEvent::MidiCC { cc: 1, value, .. } => self.synth.mod_wheel(value),
                    NoteEvent::MidiChannelPressure { pressure, .. } => self.synth.channel_pressure(pressure),
                    NoteEvent::PolyPressure { note, pressure, .. } => self.synth.poly_pressure(note, pressure),
                    _ => (),
                }
                next_event = context.next_event();
            }

            // split blocks at the next event so notes start sample-accurately
            let end = next_event
                .map_or(num_samples, |e| e.timing() as usize)
                .min(num_samples)
                .min(start + BLOCK);
            self.patch = self.params.patch((end - start) as u32);
            self.synth.render(&self.patch, &mut left[start..end], &mut right[start..end]);
            start = end;
        }
        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for Morphwave {
    const CLAP_ID: &'static str = "com.morphwave.synth";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Wavetable synthesizer");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
}

nih_export_clap!(Morphwave);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn state(params: &[(&str, ParamValue)]) -> PluginState {
        PluginState {
            version: "0.4.0".into(),
            params: params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            fields: BTreeMap::new(),
        }
    }

    #[test]
    fn old_env2_cutoff_knob_moves_into_mod_slot_1() {
        let mut s = state(&[("filt_env", ParamValue::F32(0.4))]);
        Morphwave::filter_state(&mut s);
        assert!(matches!(s.params["src_1"], ParamValue::String(ref id) if id == "env2"));
        assert!(matches!(s.params["dst_1"], ParamValue::String(ref id) if id == "cutoff"));
        assert!(matches!(s.params["amt_1"], ParamValue::F32(a) if a == 0.4));
        assert!(matches!(s.params["filt_env"], ParamValue::F32(a) if a == 0.0));
    }

    #[test]
    fn new_state_is_left_alone() {
        let mut s = state(&[("filt_env", ParamValue::F32(0.0)), ("amt_1", ParamValue::F32(-0.3))]);
        Morphwave::filter_state(&mut s);
        assert!(matches!(s.params["amt_1"], ParamValue::F32(a) if a == -0.3));
        assert!(!s.params.contains_key("src_1"));
    }
}
