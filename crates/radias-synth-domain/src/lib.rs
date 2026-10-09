//! RADIAS synthesis algorithms. No CPU emulation, allocation, devices or files.
#![no_std]
pub mod controller_service;
pub mod dsp_buffers;
pub mod dsp_control;
pub mod dsp_dispatch;
pub mod effect_control;
pub mod effect_curves;
pub mod effect_setters;
pub mod decimator_effect;
pub mod effect_updates;
pub mod inactive_frame;

pub mod actor_amplifier_preparation;
pub mod actor_construction;
pub mod actor_control_state;
pub mod actor_copy;
pub mod actor_descriptors;
pub mod actor_envelope_initialization;
pub mod actor_filter_preparation;
pub mod actor_lfo_initialization;
pub mod actor_lifecycle;
pub mod actor_note_initialization;
pub mod actor_note_preparation;
pub mod actor_pitch_preparation;
pub mod actor_startup;
pub mod actor_virtual_patch;
pub mod amp_envelope;
pub mod amplifier_control;
pub mod amplifier_delivery;
pub mod bandlimit;
pub mod comb;
pub mod comb_pointer_publication;
pub mod complete_actor_startup;
pub mod construction_first_pass;
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
pub mod filter1_initial_publication;
pub mod filter_control;
pub mod filter_routing;
pub mod fixed;
pub mod lfo;
mod lfo_controller_work;
pub mod lfo_tempo;
pub mod manual_parameters;
pub mod midi_clock;
pub mod mixer;
pub mod mod_envelope;
pub mod modulation;
pub mod mono_notes;
pub mod motion_initialization;
pub mod noise;
pub mod noise_control;
pub mod note_groups;
pub mod note_modulators;
pub mod note_pitch;
pub mod note_refresh;
pub mod oscillator;
pub mod pan;
pub mod parameter_template;
pub mod parameter_upload;
pub mod performance;
pub mod pitch;
pub mod pitch_receiver;
pub mod portamento;
pub mod primary_initialization;
pub mod primary_oscillator;
pub mod primary_parameters;
pub mod primary_pitch_dispatch;
pub mod processor_link;
pub mod program;
pub mod program_binding;
pub mod raw_note_scale;
pub mod secondary_control;
pub mod stereo_cache;
pub mod sustain;
pub mod unison;
pub mod unison_pitch;
pub mod virtual_patch_live;
mod virtual_patch_live_work;
pub mod virtual_patch_work;
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
