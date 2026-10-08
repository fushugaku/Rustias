//! RADIAS synthesis algorithms. No CPU emulation, allocation, devices or files.
#![no_std]

pub mod amp_envelope;
pub mod amplifier_control;
pub mod bandlimit;
pub mod comb;
pub mod control_slew;
pub mod controller_comb;
pub mod controller_filter;
pub mod controller_filter2;
pub mod controller_mixer;
pub mod controller_noise;
pub mod controller_pan;
pub mod controller_pitch;
pub mod controller_primary;
pub mod controller_secondary;
pub mod controller_shaper;
pub mod drum;
pub mod drum_groups;
pub mod drum_pad;
pub mod envelope;
pub mod envelope_segment;
pub mod filter;
pub mod filter_control;
pub mod filter_routing;
pub mod fixed;
pub mod lfo;
pub mod lfo_tempo;
pub mod midi_clock;
pub mod mixer;
pub mod mod_envelope;
pub mod modulation;
pub mod mono_notes;
pub mod noise;
pub mod noise_control;
pub mod note_groups;
pub mod note_pitch;
pub mod oscillator;
pub mod pan;
pub mod performance;
pub mod pitch;
pub mod portamento;
pub mod primary_oscillator;
pub mod processor_link;
pub mod program;
pub mod program_binding;
pub mod secondary_control;
pub mod stereo_cache;
pub mod sustain;
pub mod unison;
pub mod unison_pitch;
pub mod voice;
pub mod voice_allocation;
pub mod voice_frame;
pub mod voice_group;
pub mod waveform;
pub mod waveshaper;

pub const SAMPLE_RATE: u32 = 48_000;

/// Signed sample at the original synthesis boundary; no normalization.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct Sample(pub i32);

/// One cycle occupies all 32 bits. Addition deliberately wraps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct Phase(pub u32);

impl Phase {
    pub fn advance(&mut self, increment: pitch::PhaseIncrement) {
        self.0 = self.0.wrapping_add(increment.0);
    }

    pub fn retreat(&mut self, increment: pitch::PhaseIncrement) {
        self.0 = self.0.wrapping_sub(increment.0);
    }
}
