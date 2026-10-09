//! Native motion/manual parameter stores and complete called controller work.
use crate::{
    actor_amplifier_preparation::ActorAmplifierPorts,
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorPlan,
    actor_filter_preparation::FilterPreparation,
    actor_pitch_preparation::ActorPitchPorts,
    controller_noise::{FormantControlTarget, NoiseControl},
    primary_pitch_dispatch::PrimaryPitchSendTable,
    virtual_patch_live::{
        LiveCompilerPorts, LiveCompilerTables, LiveDestinationUpdate, LiveModulationCompiler,
    },
};
pub const MANUAL_PARAMETER_OFFSETS: [u16; 42] = [
    0, 0x17e, 0x180, 0x182, 0x184, 0x186, 0x188, 0x18a, 0x18c, 0x18e, 0x190, 0x192, 0x194, 0x196,
    0x198, 0x19a, 0x19c, 0x19e, 0x1a0, 0x1a2, 0x1a4, 0x1a6, 0x1a8, 0x1aa, 0x1ac, 0x1ae, 0x1b0,
    0x1b2, 0x1b4, 0x1b6, 0x1b8, 0x1ba, 0x1bc, 0x1be, 0x1c0, 0x1c2, 0x1c4, 0x1c6, 0x1c8, 0x1ca,
    0x1cc, 0x1ce,
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManualCompilerPorts {
    pub live: LiveCompilerPorts,
    pub pitch: ActorPitchPorts,
    pub amplifier: ActorAmplifierPorts,
}
pub struct ManualCompilerTables<'a> {
    pub live: &'a LiveCompilerTables<'a>,
    pub primary_pitch: PrimaryPitchSendTable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualCompilationError {
    InvalidParameter,
    UnsupportedPrimary,
    UnsupportedShaper,
    InvalidGainBank,
}
pub struct CompiledManualParameter {
    pub controller: ActorControlState,
    pub publication: DescriptorPlan,
}
impl ActorControlState {
    pub fn compile_manual_parameter(
        &self,
        parameter: u8,
        value: i16,
        body: &[u8; 104],
        ports: ManualCompilerPorts,
        tables: &ManualCompilerTables<'_>,
    ) -> Result<CompiledManualParameter, ManualCompilationError> {
        let offset = *MANUAL_PARAMETER_OFFSETS
            .get(parameter as usize)
            .ok_or(ManualCompilationError::InvalidParameter)?;
        if offset == 0 {
            return Err(ManualCompilationError::InvalidParameter);
        }
        let mut controller = *self;
        controller.set_word(offset as usize, value);
        let mut publication = DescriptorPlan::default();
        let live = tables.live;
        if parameter >= 34 {
            publication.work(22); // Whole SYS02c1fc, null callback table entry.
            return Ok(CompiledManualParameter {
                controller,
                publication,
            });
        }
        // SYS02c1fc stores the word before invoking the selected complete callback.
        let prefix = 18;
        let suffix = 5;
        match parameter {
            1 => {
                let work = controller
                    .prepare_primary_pitch(body, ports.pitch, live.fine)
                    .controller_clocks;
                let sender = tables
                    .primary_pitch
                    .sender(controller.bytes[0x1e0])
                    .ok_or(ManualCompilationError::UnsupportedPrimary)?;
                publication.send(
                    sender,
                    2,
                    controller.word(0xec) as u16 as u32,
                    prefix + 56 + work,
                    suffix + 13,
                );
            }
            2 => {
                controller.compile_live_portamento(
                    ports.live.portamento_time,
                    ports.live.portamento_switch_required,
                    ports.live.portamento_switch,
                    live.portamento,
                );
                let work = if !ports.live.portamento_switch_required {
                    39
                } else if ports.live.portamento_switch {
                    44
                } else {
                    25
                };
                publication.work(prefix + 9 + work + suffix);
            }
            3 => {
                let work = controller.primary_preparation_clocks();
                controller.refresh_primary();
                let selection = controller.bytes[0x1e0] & 63;
                let wave = selection & 15;
                let mode = selection >> 4;
                if wave >= 6 {
                    return Err(ManualCompilationError::UnsupportedPrimary);
                }
                let base = prefix + 12 + work + 25;
                if wave >= 4 && mode != 0 {
                    publication.work(prefix + 12 + work + 28 + suffix + 8);
                } else if mode == 1 {
                    publication.send(
                        0,
                        6,
                        controller.word(0x16a) as u16 as u32,
                        base + 22,
                        suffix + 18,
                    );
                } else if mode == 2 {
                    publication.send(
                        9,
                        6,
                        controller.word(0x176) as u16 as u32,
                        base + 23,
                        suffix + 18,
                    );
                } else if mode == 3 {
                    publication.send(0, 6, controller.word(0x174) as u16 as u32, base + 24, 0);
                    publication.send(
                        0,
                        8,
                        controller.primary_inputs().vpm_ratio() as u16 as u32,
                        28,
                        suffix + 18,
                    );
                } else if wave == 4 {
                    let target = NoiseControl {
                        control2: controller.bytes[0x1ec],
                        control2_modulation: controller.word(0x134),
                        control2_manual_offset: controller.bytes[0x184] as i8,
                    }
                    .colored(controller.word(0x16c));
                    publication.send(0, 6, target.color as u16 as u32, base + 23, 0);
                    publication.send(0, 8, target.frequency as u16 as u32, 42, suffix + 18);
                } else if wave == 5 {
                    let target = NoiseControl {
                        control2: controller.bytes[0x1ec],
                        control2_modulation: controller.word(0x134),
                        control2_manual_offset: controller.bytes[0x184] as i8,
                    }
                    .formant(
                        FormantControlTarget {
                            level: controller.word(0x16e),
                            feedback: controller.word(0x170),
                        },
                        controller.word(0xec),
                    );
                    publication.send(14, 16, target.shape, base + 39, 0);
                    publication.send(0, 6, target.input_gain as u16 as u32, 35, 0);
                    publication.send(0, 8, target.frequency as u16 as u32, 27, suffix + 18);
                } else {
                    publication.send(
                        0,
                        if wave == 3 { 11 } else { 6 },
                        controller.word(0x114) as u16 as u32,
                        base + 22,
                        suffix + 18,
                    );
                }
            }
            4 => {
                if controller.bytes[0x1e0] & 15 >= 6 {
                    return Err(ManualCompilationError::UnsupportedPrimary);
                }
                let work = controller.primary_preparation_clocks();
                controller.refresh_primary();
                publication.work(prefix + 20 + work + suffix);
            }
            14 => {
                let work =
                    controller.prepare_filter_control(FilterPreparation::FirstKey, body, live);
                publication.work(prefix + 9 + work + suffix);
            }
            19 => {
                let work = controller
                    .prepare_amplifier_target(body, ports.amplifier, live.amplifier)
                    .map_err(|_| ManualCompilationError::InvalidGainBank)?;
                publication.work(prefix + 9 + work + suffix);
            }
            22..=33 => {
                controller
                    .compile_live_envelope_parameter(
                        body,
                        (parameter - 22) / 4,
                        (parameter - 22) % 4,
                        live.timing,
                    )
                    .unwrap();
                let work = crate::virtual_patch_live_work::envelope_work(
                    &controller,
                    body,
                    parameter,
                    live,
                ) - 30;
                publication.work(prefix + work + suffix);
            }
            _ => {
                use LiveModulationCompiler::*;
                // Replace each changed VP store/dispatch prefix with the five-clock
                // manual callback prefix. The same arithmetic and publishers follow.
                let (destination, compiler, adjust) = match parameter {
                    5 | 6 => (1, SecondaryPitch, 10),
                    7..=9 => (
                        parameter - 4,
                        [PrimaryMixer, SecondaryMixer, NoiseMixer][(parameter - 7) as usize],
                        19,
                    ),
                    10 | 13 => (17, Filter1EnvelopeIntensity, 24),
                    11 => (8, Filter1Resonance, 21),
                    12 => (6, FilterMix, 19),
                    15 | 17 => (20, Filter2EnvelopeIntensity, 24),
                    16 => (19, Filter2Resonance, 24),
                    18 => (21, Filter2KeyTracking, 25),
                    20 => (12, Pan, 19),
                    21 => (10, ShaperDepth, 19),
                    _ => unreachable!(),
                };
                let mut sends = [(0u8, 0u16, 0u32); 2];
                let mut count = 1;
                match compiler {
                    SecondaryPitch => {
                        let value = controller.compile_live_secondary_pitch(body, live.fine);
                        sends[0] = (8, 0x25, value as u16 as u32);
                    }
                    PrimaryMixer | SecondaryMixer | NoiseMixer => {
                        let index = parameter - 7;
                        let value = controller
                            .compile_live_mixer(body, index)
                            .ok_or(ManualCompilationError::UnsupportedPrimary)?;
                        sends[0] = (0, 0x2f + 2 * u16::from(index), value as u16 as u32);
                    }
                    FilterMix => {
                        sends[0] = (
                            13,
                            0x35,
                            u32::from(controller.compile_live_filter_mix(body)),
                        )
                    }
                    Filter1Resonance => {
                        let (value, norm) =
                            controller.compile_live_filter1_resonance(body, live.resonance);
                        sends = [(18, 0x3e, value as u32), (0, 0x36, u32::from(norm))];
                        count = 2;
                    }
                    Filter1EnvelopeIntensity => {
                        let value = controller.compile_live_filter1_frequency(
                            body,
                            live.frequency,
                            live.amplifier,
                        );
                        sends[0] = (17, 0x3c, value);
                    }
                    Filter2Resonance => {
                        let (value, norm) = controller.compile_live_filter2_resonance(
                            body,
                            live.resonance,
                            live.comb,
                        );
                        sends[0] = (
                            if norm.is_some() { 18 } else { 14 },
                            if norm.is_some() { 0x66 } else { 0x60 },
                            value,
                        );
                        if let Some(norm) = norm {
                            sends[1] = (0, 0x5e, u32::from(norm));
                            count = 2;
                        }
                    }
                    Filter2EnvelopeIntensity | Filter2KeyTracking => {
                        if parameter == 18 {
                            controller.compile_live_filter2_key_tracking(body, live.frequency);
                        }
                        let (value, feedback) = controller.compile_live_filter2_frequency(
                            body,
                            live.frequency,
                            live.amplifier,
                            live.resonance,
                            live.comb,
                        );
                        sends[0] = (
                            if feedback.is_some() { 14 } else { 17 },
                            if feedback.is_some() { 0x68 } else { 0x64 },
                            value,
                        );
                        if let Some(feedback) = feedback {
                            sends[1] = (14, 0x60, feedback);
                            count = 2;
                        }
                    }
                    Pan => {
                        sends[0] = (
                            0,
                            0x7f,
                            u32::from(controller.compile_live_pan(
                                body,
                                ports.live.midi_pan,
                                live.pan,
                            )),
                        )
                    }
                    ShaperDepth => {
                        let value = controller
                            .compile_live_shaper_depth(body)
                            .ok_or(ManualCompilationError::UnsupportedShaper)?;
                        if let Some(value) = value {
                            sends[0] = (0, 0x54, value as u16 as u32);
                        } else {
                            count = 0;
                        }
                    }
                    _ => unreachable!(),
                }
                let work = crate::virtual_patch_live_work::destination_work(
                    destination,
                    LiveDestinationUpdate {
                        changed: true,
                        compiler: Some(compiler),
                    },
                    &controller,
                    body,
                    ports.live,
                    live,
                );
                if count == 0 {
                    publication.work(prefix + work.finish - 19 + suffix);
                } else {
                    for (index, (sender, offset, value)) in
                        sends[..count].iter().copied().enumerate()
                    {
                        publication.send(
                            sender,
                            offset,
                            value,
                            if index == 0 {
                                prefix + work.first - adjust
                            } else {
                                work.second
                            },
                            if index + 1 == count {
                                suffix + work.finish - if parameter == 18 { 4 } else { 0 }
                            } else {
                                0
                            },
                        );
                    }
                }
            }
        }
        Ok(CompiledManualParameter {
            controller,
            publication,
        })
    }
}
