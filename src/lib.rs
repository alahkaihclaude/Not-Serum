use nih_plug::prelude::*;
use std::sync::Arc;

pub mod dsp;

pub struct Morphwave {
    params: Arc<MorphwaveParams>,
    sample_rate: f32,
    voice: dsp::SineVoice,
}

#[derive(Params)]
struct MorphwaveParams {
    // Parameter IDs are permanent: never rename or remove one once shipped.
    #[id = "master"]
    master: FloatParam,
}

impl Default for MorphwaveParams {
    fn default() -> Self {
        Self {
            master: FloatParam::new(
                "Master",
                util::db_to_gain(-12.0),
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

impl Default for Morphwave {
    fn default() -> Self {
        Self {
            params: Arc::new(MorphwaveParams::default()),
            sample_rate: 44100.0,
            voice: dsp::SineVoice::default(),
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

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        true
    }

    fn reset(&mut self) {
        self.voice = dsp::SineVoice::default();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut next_event = context.next_event();
        for (sample_id, channel_samples) in buffer.iter_samples().enumerate() {
            while let Some(event) = next_event {
                if event.timing() > sample_id as u32 {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { note, velocity, .. } => self.voice.note_on(note, velocity),
                    NoteEvent::NoteOff { note, .. } => self.voice.note_off(note),
                    _ => (),
                }
                next_event = context.next_event();
            }

            let out = self.voice.next(self.sample_rate) * self.params.master.smoothed.next();
            for sample in channel_samples {
                *sample = out;
            }
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
