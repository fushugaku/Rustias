//! Whole SYS01eee0 startup, retaining cached publications and physical order.
use crate::{
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorPlan,
    actor_lifecycle::ActorLifecycle,
    actor_startup::{CoefficientPriming, PhaseCallback, PhaseCallbackTables, PhysicalPhaseTables},
    amplifier_delivery::AmplifierRateTable,
    controller_mixer::MixerLevel,
    controller_noise::FormantCounterSeeds,
    controller_pan::PanTables,
    dsp_control::ParameterPacket,
    envelope_segment::EnvelopeTimingTables,
    parameter_template::{ParameterTemplateTables, TemplateCompilationError},
    primary_pitch_dispatch::PrimaryPitchSendTable,
};
pub struct CompleteStartupTables<'a> {
    pub descriptors: &'a ParameterTemplateTables,
    pub phases: &'a PhysicalPhaseTables,
    pub callbacks: &'a PhaseCallbackTables,
    pub counters: &'a FormantCounterSeeds,
    pub pitch: PrimaryPitchSendTable,
    pub pan: &'a PanTables,
    pub amplifier: &'a AmplifierRateTable,
    pub timing: &'a EnvelopeTimingTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompleteStartupError {
    InvalidSlot,
    UnsupportedPrimary,
    UnsupportedShaper,
    Descriptor(TemplateCompilationError),
}
pub struct CompiledActorStartup {
    pub controller: ActorControlState,
    pub lifecycle: ActorLifecycle,
    pub publication: DescriptorPlan,
}
fn append(plan: &mut DescriptorPlan, part: &DescriptorPlan, before: u16) {
    let mut work = DescriptorPlan::default();
    work.work(before);
    plan.append_compacted(&work);
    plan.append_compacted(part);
}
impl ActorControlState {
    /// Procedure0 is whole SYS01eee0;1..20 are whole called publishers.
    pub fn compile_complete_startup(
        &self,
        procedure: u8,
        slot: usize,
        body: &[u8; 104],
        lifecycle: ActorLifecycle,
        tables: &CompleteStartupTables<'_>,
    ) -> Result<CompiledActorStartup, CompleteStartupError> {
        if slot >= 24 || procedure > 20 {
            return Err(CompleteStartupError::InvalidSlot);
        }
        let mut controller = *self;
        let mut lifecycle = lifecycle;
        let mut plan = DescriptorPlan::default();
        let physical = 0x3000 + 64 * (slot % 12) as u16;
        let parameter = 0x2000 + 160 * (slot % 12) as u16;
        let physical_offset = physical - parameter;
        if procedure == 0 {
            // Callee order and delayed calls in the original wrapper.
            for (part, before) in [
                (1, 4),
                (2, 4),
                (3, 4),
                (4, 4),
                (5, 4),
                (6, 3),
                (7, 3),
                (8, 3),
                (9, 3),
                (10, 4),
                (11, 4),
                (12, 4),
                (13, 4),
                (14, 4),
                (15, 4),
                (16, 4),
                (20, 4),
            ] {
                let compiled =
                    controller.compile_complete_startup(part, slot, body, lifecycle, tables)?;
                append(&mut plan, &compiled.publication, before);
                controller = compiled.controller;
                lifecycle = compiled.lifecycle;
            }
            let mut finish = DescriptorPlan::default();
            finish.work(4);
            plan.append_compacted(&finish);
            return Ok(CompiledActorStartup {
                controller,
                lifecycle,
                publication: plan,
            });
        }
        let selection = controller.bytes[0x1e0];
        let primary_index = PhaseCallbackTables::index(selection);
        if primary_index >= 24 {
            return Err(CompleteStartupError::UnsupportedPrimary);
        }
        match procedure {
            1 => {
                let sub = controller.compile_complete_startup(17, slot, body, lifecycle, tables)?;
                append(&mut plan, &sub.publication, 5);
                let mut cached = *body;
                cached[22] = selection;
                cached[24] = controller.bytes[0x1ec];
                let primary = DescriptorPlan::compile(
                    11,
                    &cached,
                    controller.descriptor_cache(),
                    tables.descriptors,
                )
                .map_err(CompleteStartupError::Descriptor)?;
                append(&mut plan, &primary, 4);
                let mut end = DescriptorPlan::default();
                end.work(4);
                plan.append_compacted(&end);
            }
            2 => {
                for (part, before) in [(18, 4), (19, 3)] {
                    let sub =
                        controller.compile_complete_startup(part, slot, body, lifecycle, tables)?;
                    append(&mut plan, &sub.publication, before);
                }
                let comb = controller
                    .compile_comb_pointer_publication()
                    .map_err(|_| CompleteStartupError::InvalidSlot)?;
                append(&mut plan, &comb.publication, 3);
                controller = comb.controller;
                // Cached Filter1 resonance, followed by the two Filter2 publishers.
                let mut resonance = DescriptorPlan::default();
                resonance.send(18, 0x3e, controller.long(0xbc) as u32, 24, 0);
                resonance.send(0, 0x36, u32::from(controller.word(0x110) as u16), 8, 7);
                append(&mut plan, &resonance, 3);
                let flags = controller.bytes[0x1e2];
                let mut second = DescriptorPlan::default();
                if flags & 0x30 == 0x30 {
                    second.send(14, 0x68, controller.long(0xb8) as u32, 34, 0);
                    second.send(14, 0x60, controller.long(0xc0) as u32, 44, 23);
                } else {
                    second.send(17, 0x64, controller.long(0xb8) as u32, 33, 12);
                }
                append(&mut plan, &second, 3);
                let mut res = DescriptorPlan::default();
                if flags & 0x30 == 0x30 {
                    res.send(14, 0x60, controller.long(0xc0) as u32, 34, 15);
                } else {
                    res.send(18, 0x66, controller.long(0xc0) as u32, 34, 0);
                    res.send(0, 0x5e, u32::from(controller.word(0x112) as u16), 8, 12);
                }
                append(&mut plan, &res, 3);
                let mut end = DescriptorPlan::default();
                end.work(4);
                plan.append_compacted(&end);
            }
            3 | 4 | 11 => {
                let index = if procedure == 3 {
                    1
                } else if procedure == 4 {
                    2
                } else {
                    0
                };
                let raw = controller.word(0x102 + 2 * index) as u16 as u32;
                let scale = if index == 0 {
                    controller.word(0xfe) as u16
                } else if index == 1 {
                    controller.word(0x100) as u16
                } else {
                    0x3333
                };
                let gain = (((raw * raw) >> 16) * u32::from(scale)) >> 16;
                plan.send(
                    0,
                    0x2f + 2 * index as u16,
                    gain,
                    if index == 2 { 33 } else { 34 },
                    8,
                );
            }
            5 => {
                let mut cached = *body;
                cached[46] = controller.bytes[0x1e3];
                cached[47] = controller.bytes[0x1e4];
                plan = DescriptorPlan::compile(
                    9,
                    &cached,
                    controller.descriptor_cache(),
                    tables.descriptors,
                )
                .map_err(CompleteStartupError::Descriptor)?;
            }
            6 => {
                let phases = tables
                    .phases
                    .get(selection)
                    .ok_or(CompleteStartupError::UnsupportedPrimary)?;
                if phases.count == 0 {
                    plan.work(31);
                } else {
                    for (index, word) in
                        phases.words[..usize::from(phases.count)].iter().enumerate()
                    {
                        plan.send(
                            14,
                            physical_offset + word.offset,
                            word.value,
                            if index == 0 { 43 } else { 15 },
                            if index + 1 == usize::from(phases.count) {
                                17
                            } else {
                                0
                            },
                        );
                    }
                }
            }
            7 => {
                let flags = controller.bytes[0x1e1];
                let wave = flags & 3;
                let sync = flags & 32 != 0;
                if sync || wave >= 2 {
                    let before = if !sync {
                        26
                    } else if wave == 0 {
                        25
                    } else if wave == 1 {
                        27
                    } else {
                        29
                    };
                    plan.send(14, physical_offset + 0x14, 0, before, 7);
                } else {
                    plan.work(24);
                }
            }
            8 => match tables.callbacks.phases[primary_index] {
                PhaseCallback::None => plan.work(31),
                PhaseCallback::CachedWord => {
                    plan.send(0, 15, u32::from(controller.word(0x114) as u16), 53, 14)
                }
                kind => plan.send(
                    if kind == PhaseCallback::UnisonTriangle {
                        15
                    } else {
                        16
                    },
                    22,
                    (u32::from(controller.primary_inputs().phase_code()) << 16)
                        | u32::from(physical),
                    85,
                    17,
                ),
            },
            9 => {
                if tables.callbacks.counters[primary_index] {
                    plan.send(
                        0,
                        10,
                        u32::from(tables.counters.for_slot(controller.bytes[0x34]) as u16),
                        58,
                        15,
                    );
                } else {
                    plan.work(33);
                }
            }
            10 => {
                let level = MixerLevel {
                    level: body[30],
                    manual_offset: controller.word(0x18a),
                    modulation: controller.word(0x11a),
                    scale: 0,
                };
                controller.set_word(0x102, level.composed() as i16);
                let raw = 2
                    * ((i32::from(body[30] as i8) << 8)
                        + i32::from(level.manual_offset)
                        + 2 * i32::from(level.modulation));
                plan.work(if raw < 0 {
                    39
                } else if raw > 65535 {
                    42
                } else {
                    41
                });
            }
            12 => plan.send(
                tables
                    .pitch
                    .sender(selection)
                    .ok_or(CompleteStartupError::UnsupportedPrimary)?,
                2,
                u32::from(controller.word(0xec) as u16),
                46,
                9,
            ),
            13 => plan.send(
                0,
                0x7f,
                u32::from(tables.pan.compile(controller.word(0xfc) as u16)),
                27,
                6,
            ),
            14 => {
                let mode = controller.bytes[0x1e3] & 3;
                let kind = controller.bytes[0x1e4] & 15;
                if mode == 0 {
                    plan.work(19);
                } else if mode == 1 {
                    plan.work(28);
                } else if kind == 5 || kind == 6 {
                    plan.send(11, physical_offset, u32::from(parameter + 0x98), 50, 12);
                } else {
                    plan.work(31);
                }
            }
            15 => {
                let priming = CoefficientPriming {
                    routing: controller.bytes[0x1e3],
                    shaper: controller.bytes[0x1e4],
                };
                if priming.pickup() {
                    plan.send(
                        ParameterPacket::PICKUP_PRIME_SENDER,
                        2,
                        0,
                        priming.first_sender_gap(),
                        0,
                    );
                }
                plan.send(
                    10,
                    6,
                    1,
                    if priming.pickup() {
                        17
                    } else {
                        priming.first_sender_gap()
                    },
                    6,
                );
            }
            16 => {
                controller.bytes[0x1e5] = 0;
                let code = (i32::from(body[60] & 127) + i32::from(controller.word(0x148)))
                    .clamp(0, 127) as usize;
                let increment = tables.timing.increments[3][code];
                let index = tables.timing.increments[3]
                    .iter()
                    .position(|v| increment >= *v)
                    .unwrap();
                let rate = tables.amplifier.attack_rates[index.min(3)];
                let work = if index <= 3 { 96 } else { 95 } + 6 * index as u16;
                plan.send(
                    19,
                    0x7c,
                    (u32::from(rate) << 16) | u32::from(controller.word(0xf0) as u16),
                    work,
                    0,
                );
                let gain = body[51];
                if gain == 0 {
                    let mut tail = DescriptorPlan::default();
                    tail.work(21);
                    plan.append_compacted(&tail);
                } else {
                    let target = if gain < 64 {
                        u32::from(gain) * 520
                    } else {
                        32767
                    };
                    plan.send(
                        14,
                        0x9c,
                        0x10000 | target,
                        if gain < 64 { 45 } else { 39 },
                        13,
                    );
                }
            }
            20 => {
                lifecycle.activate(slot);
                plan.send(0, 0, 1, 29, 6);
            }
            17 => plan.send(8, 0x25, u32::from(controller.word(0xee) as u16), 23, 6),
            18 => {
                plan.send(
                    13,
                    0x35,
                    u32::from(controller.compile_live_filter_mix(body)),
                    47,
                    7,
                );
            }
            19 => {
                plan = controller.compile_initial_filter1_frequency(body);
            }
            _ => unreachable!(),
        }
        Ok(CompiledActorStartup {
            controller,
            lifecycle,
            publication: plan,
        })
    }
}
