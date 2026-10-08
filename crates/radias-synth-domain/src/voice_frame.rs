//! Physical DSP frame memory survives note retirement and parameter-block reset.
use crate::{
    Phase, Sample,
    noise::{MixerNoise, NoiseFrameSeeds},
    voice::Voice,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceFrameState {
    pub primary_phase: Phase,
    pub modulated_phase: Phase,
    pub previous_primary: Phase,
    pub secondary_phase: Phase,
    pub mixer_noise: MixerNoise,
    pub previous_secondary: Sample,
    pub stereo_cache: crate::stereo_cache::StereoCache,
    pub stereo_bus: crate::pan::VoiceBus,
    pub last_amplified: Sample,
    pub last_pan_current: i16,
}

impl VoiceFrameState {
    pub fn from_boot(seeds: &NoiseFrameSeeds, slot: usize) -> Self {
        Self {
            primary_phase: seeds.primary[slot],
            secondary_phase: seeds.secondary[slot],
            mixer_noise: seeds.mixer[slot],
            ..Self::default()
        }
    }
    pub fn capture(voice: &Voice) -> Self {
        Self {
            primary_phase: voice.primary.phase,
            modulated_phase: voice.primary.modulated_phase,
            previous_primary: voice.previous_primary,
            // Oscillator stores the phase of its next output. Physical frame20
            // stores the phase just emitted, before the next decrement.
            secondary_phase: Phase(
                voice
                    .secondary
                    .phase()
                    .0
                    .wrapping_add(voice.secondary.increment().0),
            ),
            mixer_noise: voice.mixer_noise,
            previous_secondary: voice.previous_secondary,
            stereo_cache: Default::default(),
            stereo_bus: Default::default(),
            last_amplified: Sample(0),
            last_pan_current: 0,
        }
    }
    pub fn restore(self, voice: &mut Voice) {
        voice.primary.phase = self.primary_phase;
        voice.primary.modulated_phase = self.modulated_phase;
        voice.previous_primary = self.previous_primary;
        voice.secondary.sync_phase(Phase(
            self.secondary_phase
                .0
                .wrapping_sub(voice.secondary.increment().0),
        ));
        voice.mixer_noise = self.mixer_noise;
        voice.previous_secondary = self.previous_secondary;
    }
}
