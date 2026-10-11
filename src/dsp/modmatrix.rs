//! Modulation matrix: sources, destinations and per-voice evaluation.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Source {
    None,
    Env1,
    Env2,
    Env3,
    Lfo1,
    Lfo2,
    Lfo3,
    Lfo4,
    Velocity,
    ModWheel,
    Aftertouch,
    /// Note number, bipolar: -1 at MIDI 0, 0 at middle C, ~+1 at MIDI 127.
    Note,
    Macro1,
    Macro2,
    Macro3,
    Macro4,
}

impl Source {
    /// Same order as the `Mod Source` parameter.
    pub const ALL: [Self; 16] = [
        Self::None,
        Self::Env1,
        Self::Env2,
        Self::Env3,
        Self::Lfo1,
        Self::Lfo2,
        Self::Lfo3,
        Self::Lfo4,
        Self::Velocity,
        Self::ModWheel,
        Self::Aftertouch,
        Self::Note,
        Self::Macro1,
        Self::Macro2,
        Self::Macro3,
        Self::Macro4,
    ];
}

pub const NUM_SOURCES: usize = Source::ALL.len();

/// Modulation targets. Units at amount 100%: pitch +-24 semitones, cutoff +-8 octaves,
/// LFO rate +-4 octaves, everything else the parameter's full range. FX destinations follow the newest voice.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dest {
    None,
    // Osc A, then Osc B in the same order (voice.rs indexes these by offset)
    APosition,
    ALevel,
    APan,
    APitch,
    ADetune,
    ABlend,
    AWidth,
    BPosition,
    BLevel,
    BPan,
    BPitch,
    BDetune,
    BBlend,
    BWidth,
    Pitch,
    SubLevel,
    NoiseLevel,
    Cutoff,
    Resonance,
    Drive,
    Volume,
    Lfo1Rate,
    Lfo2Rate,
    Lfo3Rate,
    Lfo4Rate,
    DistDrive,
    DistMix,
    ChorusMix,
    DelayMix,
    DelayFeedback,
    ReverbMix,
    ReverbSize,
    EqLow,
    EqMid,
    EqHigh,
}

impl Dest {
    /// Same order as the `Mod Destination` parameter.
    pub const ALL: [Self; 36] = [
        Self::None,
        Self::APosition,
        Self::ALevel,
        Self::APan,
        Self::APitch,
        Self::ADetune,
        Self::ABlend,
        Self::AWidth,
        Self::BPosition,
        Self::BLevel,
        Self::BPan,
        Self::BPitch,
        Self::BDetune,
        Self::BBlend,
        Self::BWidth,
        Self::Pitch,
        Self::SubLevel,
        Self::NoiseLevel,
        Self::Cutoff,
        Self::Resonance,
        Self::Drive,
        Self::Volume,
        Self::Lfo1Rate,
        Self::Lfo2Rate,
        Self::Lfo3Rate,
        Self::Lfo4Rate,
        Self::DistDrive,
        Self::DistMix,
        Self::ChorusMix,
        Self::DelayMix,
        Self::DelayFeedback,
        Self::ReverbMix,
        Self::ReverbSize,
        Self::EqLow,
        Self::EqMid,
        Self::EqHigh,
    ];

    /// First of the seven per-osc destinations for osc `i`.
    pub fn osc_base(i: usize) -> usize {
        [Self::APosition, Self::BPosition][i] as usize
    }
}

pub const NUM_DESTS: usize = Dest::ALL.len();
pub const PITCH_SEMIS: f32 = 24.0;
pub const CUTOFF_OCTAVES: f32 = 8.0;
pub const LFO_RATE_OCTAVES: f32 = 4.0;

#[derive(Clone, Copy, Debug)]
pub struct ModSlot {
    pub source: Source,
    pub dest: Dest,
    /// Bipolar, [-1, 1].
    pub amount: f32,
    /// [-1, 1]: 0 linear, positive bends toward slow-start (exponential), negative toward fast-start.
    pub curve: f32,
}

impl Default for ModSlot {
    fn default() -> Self {
        Self { source: Source::None, dest: Dest::None, amount: 0.0, curve: 0.0 }
    }
}

#[inline]
pub fn shape(x: f32, curve: f32) -> f32 {
    if curve == 0.0 {
        x
    } else {
        x.abs().powf((curve * 3.0).exp2()).copysign(x)
    }
}

/// Sum of all slots per destination. `sources` is indexed by `Source as usize`.
pub fn evaluate(slots: &[ModSlot], sources: &[f32; NUM_SOURCES]) -> [f32; NUM_DESTS] {
    let mut out = [0.0; NUM_DESTS];
    for s in slots {
        if s.source != Source::None && s.dest != Dest::None && s.amount != 0.0 {
            out[s.dest as usize] += shape(sources[s.source as usize], s.curve) * s.amount;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_sum_per_destination() {
        let mut src = [0.0; NUM_SOURCES];
        src[Source::Lfo1 as usize] = 0.5;
        src[Source::Velocity as usize] = 1.0;
        let slots = [
            ModSlot { source: Source::Lfo1, dest: Dest::Cutoff, amount: 0.5, curve: 0.0 },
            ModSlot { source: Source::Velocity, dest: Dest::Cutoff, amount: -0.1, curve: 0.0 },
            ModSlot { source: Source::Velocity, dest: Dest::None, amount: 1.0, curve: 0.0 },
            ModSlot { source: Source::None, dest: Dest::Volume, amount: 1.0, curve: 0.0 },
        ];
        let m = evaluate(&slots, &src);
        assert!((m[Dest::Cutoff as usize] - 0.15).abs() < 1e-6);
        assert_eq!(m[Dest::Volume as usize], 0.0);
    }

    #[test]
    fn curve_keeps_endpoints_and_sign() {
        for c in [-1.0, -0.3, 0.0, 0.5, 1.0] {
            assert!((shape(1.0, c) - 1.0).abs() < 1e-6);
            assert!((shape(-1.0, c) + 1.0).abs() < 1e-6);
            assert_eq!(shape(0.0, c), 0.0);
        }
        assert!(shape(0.5, 1.0) < 0.5 && shape(0.5, -1.0) > 0.5);
    }

    #[test]
    fn osc_destinations_line_up() {
        assert_eq!(Dest::ALL[Dest::osc_base(0) + 3], Dest::APitch);
        assert_eq!(Dest::ALL[Dest::osc_base(1) + 6], Dest::BWidth);
        for (i, d) in Dest::ALL.iter().enumerate() {
            assert_eq!(*d as usize, i);
        }
        for (i, s) in Source::ALL.iter().enumerate() {
            assert_eq!(*s as usize, i);
        }
    }
}
