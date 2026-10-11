use nih_plug::prelude::*;
use std::sync::Arc;

pub mod dsp;
mod editor;
pub mod params;
pub mod voice;
pub mod wavetable_import;

use params::MorphwaveParams;
use voice::{Patch, Synth, BLOCK};
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
