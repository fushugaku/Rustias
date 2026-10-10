//! Compile source input controls; oscillator/filter DSP coefficients are separate.
use crate::{
    lfo::LfoParameters,
    mixer::MixerProgram,
    modulation::{ModulationProgram, PatchRoute},
    secondary::SecondaryProgram,
    voice_envelopes::ModEnvelopeProgram,
};
use radias_synth_domain::{
    controller_secondary::SecondaryPitch, modulation::ModulationDestination, program::Timbre,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidPatchDestination {
    pub route: usize,
    pub raw: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimbreControls {
    pub voice_mode: radias_synth_domain::mono_notes::VoiceMode,
    pub voice_group: radias_synth_domain::voice_group::VoiceGroupProgram,
    pub sustain: radias_synth_domain::sustain::SustainProgram,
    pub pitch: radias_synth_domain::note_pitch::PitchProgram,
    pub portamento: radias_synth_domain::portamento::PortamentoProgram,
    pub oscillator_selection: u8,
    pub oscillator_controls: [u8; 2],
    pub secondary: SecondaryProgram,
    pub mixer: MixerProgram,
    pub filter_route: u8,
    pub cutoff: [u8; 2],
    pub resonance: [u8; 2],
    pub filter_type: u8,
    pub eg1_intensity: u8,
    pub filter_key_tracking: u8,
    pub amplifier_level: u8,
    pub amplifier_key_tracking: u8,
    pub pan: u8,
    pub shaper: crate::shaper::ShaperProgram,
    pub filter2_eg_intensity: u8,
    pub filter2_key_tracking: u8,
    pub envelope: [ModEnvelopeProgram; 3],
    pub modulation: ModulationProgram,
}
impl TimbreControls {
    /// A drum instrument supplies the complete104-byte synthesis control body;
    /// the owning timbre retains channel, receive, allocation and sustain data.
    pub fn from_drum_instrument(
        owner: Timbre<'_>,
        instrument: &[u8; radias_synth_domain::drum::DRUM_INSTRUMENT_BYTES],
    ) -> Result<Self, InvalidPatchDestination> {
        let mut raw = *owner.bytes();
        raw[16..120].copy_from_slice(instrument);
        Self::from_timbre(Timbre::from_bytes(&raw))
    }
    /// Shared instrument gain/volume are explicit inputs rather than patched
    /// fixture constants. All eight EG2 fields come from this timbre's bytes.
    pub fn amplifier(
        &self,
        source_gain: u16,
        midi_volume: Option<u8>,
        program_volume: u8,
    ) -> crate::amplifier::AmplifierProgram {
        crate::amplifier::AmplifierProgram {
            envelope: self.envelope[1],
            level: self.amplifier_level,
            level_offset: 0,
            key_tracking: self.amplifier_key_tracking,
            source_gain,
            midi_volume,
            program_volume,
        }
    }
    pub fn primary(&self) -> crate::primary::PrimaryProgram {
        crate::primary::PrimaryProgram {
            selection: self.oscillator_selection,
            control: radias_synth_domain::controller_primary::PrimaryControl {
                control1: self.oscillator_controls[0],
                control2: self.oscillator_controls[1],
                ..Default::default()
            },
        }
    }
    pub fn from_timbre(timbre: Timbre<'_>) -> Result<Self, InvalidPatchDestination> {
        let p = timbre.synthesis();
        let mut modulation = ModulationProgram::default();
        for (i, lfo) in modulation.lfo.iter_mut().take(2).enumerate() {
            let b = 0x4c + i * 5;
            *lfo = LfoParameters {
                waveform: p[b],
                shape: p[b + 1],
                frequency: p[b + 2],
                phase_sync: p[b + 3],
                frequency_offset: 0,
                frequency_modulation: 0,
            };
            modulation.tempo_divisions[i] = p[b + 4];
        }
        for (i, route) in modulation.routes.iter_mut().take(6).enumerate() {
            let b = 0x56 + i * 3;
            *route = PatchRoute {
                source: p[b],
                destination: ModulationDestination::new(p[b + 1]).ok_or(
                    InvalidPatchDestination {
                        route: i,
                        raw: p[b + 1],
                    },
                )?,
                intensity: p[b + 2],
            };
        }
        let envelope = core::array::from_fn(|i| {
            let b = 0x34 + i * 8;
            ModEnvelopeProgram {
                adsr: p[b..b + 4].try_into().unwrap(),
                curve: p[b + 4],
                velocity_level_sensitivity: p[b + 5],
                velocity_time_sensitivity: p[b + 6],
                key_tracking: p[b + 7],
            }
        });
        Ok(Self {
            voice_mode: radias_synth_domain::mono_notes::VoiceMode::from_raw(p[0x10]),
            voice_group: radias_synth_domain::voice_group::VoiceGroupProgram {
                raw: timbre.bytes()[8],
                detune: timbre.bytes()[9],
                spread: timbre.bytes()[10],
            },
            sustain: radias_synth_domain::sustain::SustainProgram {
                enabled: timbre.bytes()[5] & 4 != 0,
            },
            portamento: radias_synth_domain::portamento::PortamentoProgram {
                time: timbre.bytes()[0xc],
                curve: timbre.bytes()[0xd] & 15,
                switch_required: timbre.bytes()[5] & 8 != 0,
            },
            pitch: radias_synth_domain::note_pitch::PitchProgram {
                transpose: p[0x13],
                fine_tune: p[0x14],
                vibrato_intensity: p[0x15],
                bend_range: timbre.bytes()[0xb],
                bend_enabled: timbre.bytes()[5] & 0x80 != 0,
                wheel_enabled: timbre.bytes()[5] & 0x10 != 0,
            },
            oscillator_selection: p[0x16],
            oscillator_controls: [p[0x17], p[0x18]],
            secondary: SecondaryProgram {
                selection: p[0x1b],
                pitch: SecondaryPitch {
                    semitone: p[0x1c],
                    fine_tune: p[0x1d],
                    ..Default::default()
                },
            },
            mixer: MixerProgram {
                selections: [p[0x16], p[0x1b]],
                levels: p[0x1e..0x21].try_into().unwrap(),
                manual_offsets: [0; 3],
            },
            filter_route: p[0x21],
            filter_type: p[0x22],
            cutoff: [p[0x23], p[0x28]],
            resonance: [p[0x24], p[0x29]],
            eg1_intensity: p[0x25],
            filter_key_tracking: p[0x26],
            amplifier_level: p[0x2d],
            amplifier_key_tracking: p[0x32],
            pan: p[0x31],
            shaper: crate::shaper::ShaperProgram {
                mode: crate::shaper::ShaperMode::from_allocation(p[0x2e] & 3, p[0x2f] & 15)
                    .unwrap_or_default(),
                position: if p[0x2e] & 16 != 0 {
                    radias_synth_domain::waveshaper::ShaperPosition::PreAmp
                } else {
                    radias_synth_domain::waveshaper::ShaperPosition::PreFilter
                },
                control: radias_synth_domain::controller_shaper::ShaperControl {
                    depth: p[0x30],
                    ..Default::default()
                },
            },
            filter2_eg_intensity: p[0x2a],
            filter2_key_tracking: p[0x2b],
            envelope,
            modulation,
        })
    }
}

/// The selected program owns four independent stored timbres. Device/MIDI
/// state is supplied by the caller rather than read from a reference machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoredTimbre {
    pub enabled: bool,
    pub channel: u8,
    pub key_window: [u8; 2],
    pub receive_flags: u8,
    pub controls: TimbreControls,
}
impl StoredTimbre {
    pub fn accepts(self, channel: u8, note: u8) -> bool {
        self.enabled
            && self.channel == channel
            && note < 128
            && self.key_window[0] <= note
            && note <= self.key_window[1]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoredProgram {
    pub timbres: [StoredTimbre; 4],
    pub tempo_tenths: u16,
    pub drum_timbre: u8,
    pub drum: radias_synth_domain::drum::DrumProgram,
    pub arpeggiator_flags: u8,
    pub vocoder_flags: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidStoredProgram {
    pub timbre: u8,
    pub patch: InvalidPatchDestination,
}
impl StoredProgram {
    pub fn from_program(
        program: &radias_synth_domain::program::Program,
        global_channel: u8,
    ) -> Result<Self, InvalidStoredProgram> {
        let bind = |index: usize| {
            let t = program.timbre(index).unwrap();
            Ok(StoredTimbre {
                enabled: t.enabled(),
                channel: t.channel(global_channel),
                key_window: t.key_window(),
                receive_flags: t.bytes()[5],
                controls: TimbreControls::from_timbre(t).map_err(|patch| InvalidStoredProgram {
                    timbre: index as u8,
                    patch,
                })?,
            })
        };
        Ok(Self {
            timbres: [bind(0)?, bind(1)?, bind(2)?, bind(3)?],
            tempo_tenths: program.tempo_tenths(),
            drum_timbre: program.drum_timbre(),
            drum: program.drum_program(),
            arpeggiator_flags: program.arpeggiator_flags(),
            vocoder_flags: program.vocoder_flags(),
        })
    }
}
