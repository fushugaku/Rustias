//! Master parameter boundary and native coefficient controllers.
use crate::{
    cabinet_effect::CabinetEffectTables,
    decimator_effect::{DecimatorEffectChange, DecimatorEffectState, DecimatorEffectTables},
    delay_time::{DelayClock, DelayTimeState, DelayTimeTables, divide_192, encode_delay_frames},
    dynamics_effect::DynamicsEffectTables,
    early_reflect_effect::EarlyReflectEffectTables,
    effect_control::{EffectKind, EffectMix, MixContext, PreparedEffect},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_equalizer::EffectEqualizerTables,
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    effect_midi::{EffectMidiPolarity, EffectMidiSources, effect_controller_level},
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_program_staging::EffectStagedProgramWrite,
    effect_routing::EffectRoutingInstance,
    effect_setters::EffectSelectorWrites,
    effect_updates::{CoefficientChange, EffectCoefficientAssignments, UNASSIGNED_TARGET},
    ensemble_effect::EnsembleEffectTables,
    filter_effect::{FilterEffectCache, FilterEffectFrequency, FilterEffectTables},
    flanger_phaser_effect::FlangerPhaserTables,
    lfo_tempo::LfoTempoTables,
    master_reverb_control::MasterReverbTables,
    pitch_grain_shifter::PitchGrainTables,
    rotary_effect::RotaryTables,
    talking_effect::TalkingTables,
    tremolo_ring_mod_effect::TremoloRingModTables,
    wah_effect::WahEffectTables,
};
pub const SUPPORTED_MASTER_TYPES: [u8; 31] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30,
];
pub struct MasterDefinition {
    pub parameter_count: usize,
    pub defaults: [u8; 20],
    pub default_owner: u8,
    pub initialization_mask: u32,
    pub ranges: [EffectParameterRange; 20],
    pub dependencies: [[u32; 2]; 20],
    pub group_count: usize,
    pub groups: [EffectCoefficientGroup; 73],
    pub lfo_mapping: EffectLfoMapping,
}
pub struct MasterControlTables {
    pub definitions: [MasterDefinition; 31],
    pub dynamics: DynamicsEffectTables,
    pub ensemble: EnsembleEffectTables,
    pub tempo: LfoTempoTables,
    pub ring: TremoloRingModTables,
    pub pitch: PitchGrainTables,
    pub grain_milliseconds: [u16; 128],
    pub decimator: DecimatorEffectTables,
    pub equalizer: EffectEqualizerTables,
    pub equalizer_gain: [u32; 73],
    pub distortion_gain: [u32; 128],
    pub delay_times: DelayTimeTables,
    pub tape_milliseconds: [u16; 128],
    pub time_owners: [u8; 31],
    pub stereo_mod_milliseconds: [u16; 128],
    pub chorus_milliseconds: [u16; 128],
    pub modulation_rate: [u32; 128],
    pub flanger: FlangerPhaserTables,
    pub cabinet: CabinetEffectTables,
    pub filter: FilterEffectTables,
    pub wah: WahEffectTables,
    pub rotary: RotaryTables,
    pub talking: TalkingTables,
    pub master_talking_programs: [[u8; 720]; 2],
    pub early_reflect: EarlyReflectEffectTables,
    pub early_reflect_decay: [u32; 128],
    pub reverb: MasterReverbTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasterMidiBinding {
    pub source: u32,
    pub values: [i8; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasterControlState {
    pub assignments: EffectCoefficientAssignments,
    pub lfo: EffectLfoProgram,
    pub delay: DelayTimeState,
    pub pending: [u32; 2],
    pub pending_control: u32,
    pub owner: u32,
    pub update_marker: u32,
    pub filter_cache: FilterEffectCache,
    pub midi_binding: MasterMidiBinding,
    pub rotary_mode: u32,
    pub rotary_speed: u32,
    pub work_slot: u8,
    pub coefficient_scratch: [u32; 73],
}
#[derive(Clone, Copy)]
pub struct MasterEdit {
    pub kind: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub previous_parameters: [u8; 20],
    /// Decoded stored global controller, SYS0755D4. No interpreter dependency.
    pub stored_owner: u8,
    pub stored_effect_type: u8,
    pub stored_enabled: bool,
    pub update_marker: u32,
    pub origin: u16,
    pub owner: u32,
    pub direct_switch: u32,
    pub clock_rate: u32,
    pub clock: DelayClock,
    pub current_note: u8,
    pub midi: EffectMidiSources,
    pub polarity: EffectMidiPolarity,
    pub prefix_origin: u16,
    pub body_origin: u16,
    pub relocation_origin: u16,
    pub transition_marker: u32,
}
pub struct PreparedMasterEdit {
    pub next: MasterControlState,
    pub batch: EffectParameterBatch,
    pub program_writes: [Option<EffectStagedProgramWrite>; 2],
    pub body_program: Option<PreparedEffect>,
}
pub(crate) fn publish(
    state: &mut MasterControlState,
    batch: &mut EffectParameterBatch,
    control: EffectInterpolationControl,
    target: u32,
    value: u32,
    mode: u8,
) -> Option<()> {
    let p = state.assignments.prepare(CoefficientChange {
        direct_switch: control.direct_switch,
        standalone: false,
        enabled_argument: control.enabled_argument,
        target,
        value,
        mode,
    });
    batch.append(&p.plan)?;
    state.assignments = p.next;
    Some(())
}
impl MasterControlTables {
    fn filter_frequency(
        &self,
        state: &mut MasterControlState,
        batch: &mut EffectParameterBatch,
        edit: MasterEdit,
        value: i8,
    ) -> Option<()> {
        let prepared = self.filter.prepare_frequency(
            state.filter_cache,
            FilterEffectFrequency {
                origin: edit.origin,
                cutoff: edit.parameters[2],
                resonance: edit.parameters[3],
                modulation_depth: edit.parameters[6],
                modulation: (effect_controller_level(value) >> 8) as i16,
            },
        )?;
        batch.extend(&prepared.batch)?;
        state.filter_cache = prepared.next;
        Some(())
    }
    fn delay_time(
        &self,
        state: &mut MasterControlState,
        batch: &mut EffectParameterBatch,
        edit: MasterEdit,
    ) -> Option<()> {
        let control = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            self.time_owners[usize::from(edit.kind)],
            state.owner,
            0,
            true,
        );
        let p = edit.parameters;
        if edit.kind == 20 {
            let mut mapped = [0; 20];
            mapped[3] = 64;
            mapped[4] = p[4];
            mapped[5] = p[5];
            let time = self.delay_times.two_channel_scaled(
                &mapped,
                state.delay,
                edit.clock,
                state.delay.capacity,
                &self.chorus_milliseconds,
                10,
            )?;
            state.delay = time.state;
            let channel = match edit.parameter {
                4 => 0,
                5 => 1,
                _ => return None,
            };
            batch.push_direct(
                u32::from(edit.origin.wrapping_add(if channel == 0 { 7 } else { 9 })),
                encode_delay_frames(time.frames[channel], 7),
            )?;
            return Some(());
        }
        if edit.kind == 13 {
            let time = self.delay_times.lcr(&p, state.delay, edit.clock)?;
            state.delay = time.state;
            let all = p[1] != 0 || matches!(edit.parameter, 1 | 2);
            for (channel, offset) in [6, 7, 8].into_iter().enumerate() {
                let selected = match channel {
                    0 => matches!(edit.parameter, 3 | 6),
                    1 => matches!(edit.parameter, 4 | 7),
                    _ => matches!(edit.parameter, 5 | 8),
                };
                if !all && !selected {
                    continue;
                }
                let value = encode_delay_frames(time.frames[channel], 6);
                let target = u32::from(edit.origin) + offset;
                if channel == 1 {
                    batch.push_direct(target, value)?;
                } else {
                    publish(state, batch, control, target, value, 1)?;
                }
            }
        } else {
            let stereo = matches!(edit.kind, 14 | 16 | 18);
            let mut mapped = [0; 20];
            if edit.kind == 14 {
                mapped = p;
            } else {
                mapped[2..8].copy_from_slice(&p[1..7]);
            }
            let milliseconds = if edit.kind == 18 {
                &self.stereo_mod_milliseconds
            } else if matches!(edit.kind, 17 | 19) {
                &self.tape_milliseconds
            } else if stereo {
                &self.delay_times.stereo_milliseconds
            } else {
                &self.delay_times.lcr_milliseconds
            };
            // Master stereo uses its entire allocated buffer; the insert
            // stereo controller halves its allocation.
            let time = self.delay_times.two_channel(
                &mapped,
                state.delay,
                edit.clock,
                state.delay.capacity,
                milliseconds,
            )?;
            state.delay = time.state;
            if stereo {
                batch.push_direct(
                    u32::from(edit.origin)
                        + match edit.kind {
                            14 => 29,
                            18 => 28,
                            _ => 32,
                        },
                    self.delay_times.feedback_limit(
                        time.frames[0],
                        time.frames[1],
                        p[if edit.kind == 14 { 8 } else { 7 }],
                    )?,
                )?;
            }
            let sync_parameter = if edit.kind == 14 { 2 } else { 1 };
            let all = p[sync_parameter] != 0
                || usize::from(edit.parameter) == sync_parameter
                || usize::from(edit.parameter) == sync_parameter + 1;
            let offsets = match edit.kind {
                17 | 18 => [7, 9],
                19 => [7, 8],
                _ => [8, 10],
            };
            for (channel, offset) in offsets.into_iter().enumerate() {
                let relative_parameter = usize::from(edit.parameter).wrapping_sub(sync_parameter);
                let selected = if channel == 0 {
                    matches!(relative_parameter, 2 | 4)
                } else {
                    matches!(relative_parameter, 3 | 5)
                };
                if !all && !selected {
                    continue;
                }
                publish(
                    state,
                    batch,
                    control,
                    u32::from(edit.origin.wrapping_add(offset)),
                    encode_delay_frames(time.frames[channel], if stereo { 7 } else { 6 }),
                    1,
                )?;
            }
        }
        Some(())
    }
    fn equalizer_band(
        &self,
        state: &mut MasterControlState,
        batch: &mut EffectParameterBatch,
        edit: MasterEdit,
        index: usize,
    ) -> Option<()> {
        let base = if edit.kind == 7 {
            match edit.parameter {
                0..=4 => 2,
                5..=7 => 5,
                8..=10 => 8,
                _ => 11,
            }
        } else {
            match edit.parameter {
                3 | 13..=15 => 13,
                10..=12 => 10,
                7..=9 => 7,
                _ => 4,
            }
        };
        if edit.kind == 7
            && state.owner != u32::from(edit.stored_owner)
            && matches!(edit.stored_owner, 4 | 7 | 10 | 13)
        {
            state.owner = u32::from(edit.stored_owner);
            batch.push_direct(
                u32::from(edit.origin) + 46,
                (u32::from(edit.origin) + index as u32).wrapping_sub(20),
            )?;
        }
        let control = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            (base + 2) as u8,
            state.owner,
            0,
            true,
        );
        let shape = if edit.kind == 7 {
            2
        } else if base == 4 && edit.parameters[2] != 0 {
            0
        } else if base == 13 && edit.parameters[3] != 0 {
            1
        } else {
            2
        };
        let frequency = edit.parameters[base];
        let q = edit.parameters[base + 1];
        let gain = i32::from(edit.parameters[base + 2]) - 64;
        let band = (base - if edit.kind == 7 { 2 } else { 4 }) / 3;
        if gain == 0 {
            publish(
                state,
                batch,
                control,
                u32::from(edit.origin) + 36 + band as u32,
                self.equalizer_gain[36],
                0,
            )?;
            if edit.direct_switch == 0 {
                batch.push_command(0, 0x01000014)?;
            }
        }
        let selected_gain = if gain < 0 { -36 } else { 36 };
        let values = if shape == 2 {
            let coefficients = self.equalizer.peaking(frequency, q, selected_gain)?;
            let scale = match frequency {
                0..=34 => 1,
                35..=45 => 2,
                _ => 3,
            };
            let convert = |v: u32, exponent: u32| ((v as i32) >> (8 + scale - exponent)) as u32;
            [
                scale,
                convert(coefficients[0], coefficients[5]),
                convert(coefficients[1], coefficients[6]),
                convert(coefficients[3], 1),
                convert(coefficients[2], coefficients[7]),
                convert(coefficients[4], 0),
            ]
        } else {
            let coefficients = if shape == 0 {
                self.equalizer
                    .low_shelf(frequency, selected_gain)?
                    .map(|v| ((v as i32) >> 2) as u32)
            } else if gain == 0 {
                // SYS07859E's original zero-gain high-shelf path calls low-shelf.
                self.equalizer.low_shelf(frequency, 36)?
            } else {
                self.equalizer.high_shelf(frequency, selected_gain)?
            };
            [3, coefficients[0], coefficients[1], coefficients[2], 0, 0]
        };
        for (offset, value) in values.into_iter().enumerate() {
            let tag = if edit.direct_switch == 0 {
                (0x80 | (6 - offset as u32)) << 24
            } else {
                0
            };
            batch.push_command(
                (u32::from(edit.origin) + 12 + 6 * band as u32 + offset as u32) as u16,
                tag | (value & 0xffffff),
            )?;
        }
        Some(())
    }
    pub fn prepare(
        &self,
        state: &MasterControlState,
        edit: MasterEdit,
    ) -> Option<PreparedMasterEdit> {
        self.prepare_inner(state, edit, true)
    }
    /// Initial stored masks pass the raw stored byte independently of the
    /// constructor-clamped current parameter array (whole SYS07BBFA).
    pub fn prepare_stored_argument(
        &self,
        state: &MasterControlState,
        edit: MasterEdit,
    ) -> Option<PreparedMasterEdit> {
        self.prepare_inner(state, edit, false)
    }
    fn prepare_inner(
        &self,
        state: &MasterControlState,
        edit: MasterEdit,
        validate_argument: bool,
    ) -> Option<PreparedMasterEdit> {
        if !SUPPORTED_MASTER_TYPES.contains(&edit.kind) {
            return None;
        }
        let definition = self.definitions.get(usize::from(edit.kind))?;
        let parameter = usize::from(edit.parameter);
        if parameter >= definition.parameter_count {
            return None;
        }
        let valid = |v: u8, r: EffectParameterRange| {
            let x = i32::from(v) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&x)
        };
        if (validate_argument && !valid(edit.value, definition.ranges[parameter]))
            || edit.parameters[..definition.parameter_count]
                .iter()
                .zip(definition.ranges)
                .any(|(&v, r)| !valid(v, r))
        {
            return None;
        }
        let mut next = *state;
        next.owner = edit.owner;
        next.update_marker = edit.update_marker;
        let mut batch = EffectParameterBatch::from_lfo(None);
        let mut program_writes = [None; 2];
        let mut body_program = None;
        for (index, group) in definition.groups[..definition.group_count]
            .iter()
            .enumerate()
        {
            let mask = if index == 64 {
                0
            } else {
                0x80000000 >> (index % 32)
            };
            if definition.dependencies[parameter][usize::from(index >= 32)] & mask == 0 {
                continue;
            }
            // The original constructs a fresh context for every group. A
            // Distortion band can rebind the owner before its gain group.
            let control = EffectInterpolationControl::from_owners(
                edit.direct_switch,
                edit.parameter,
                next.owner,
                0,
                true,
            );
            match group.action {
                0 => {}
                38 | 54..=58 if edit.kind == 11 => self.prepare_reverb_action(
                    &mut next,
                    &mut batch,
                    &mut program_writes,
                    &mut body_program,
                    edit,
                    group.action,
                )?,
                23 | 24 | 59 if edit.kind == 12 => self.prepare_early_reflect_action(
                    &mut next,
                    &mut batch,
                    &mut program_writes,
                    edit,
                    group.action,
                    control,
                )?,
                73 => self.prepare_talking_mode(
                    &mut next,
                    &mut batch,
                    &mut program_writes,
                    &mut body_program,
                    edit,
                )?,
                74 | 75 => self.prepare_talking_coefficient(
                    &mut next,
                    &mut batch,
                    edit,
                    group.action,
                    control,
                )?,
                63..=65 | 68 => {
                    self.prepare_rotary_action(&mut next, &mut batch, edit, group.action)?
                }
                1 | 2 | 66 | 67 => {
                    let value = match group.action {
                        66 => {
                            if edit.value == 0 {
                                group.second as u32
                            } else {
                                i32::from(edit.value)
                                    .wrapping_add(24)
                                    .wrapping_mul(group.first.wrapping_sub(group.second))
                                    .wrapping_div(100) as u32
                            }
                        }
                        67 => 0x7fffffu32.wrapping_sub(group.range.compile(
                            EffectCurve::Linear,
                            i32::from(edit.value),
                            group.first,
                            group.second,
                        )? as u32),
                        action => {
                            let width =
                                i32::from(group.range.maximum) - i32::from(group.range.minimum);
                            if width <= 0 {
                                return None;
                            }
                            let position = i32::from(edit.value) - i32::from(group.range.minimum);
                            if (action == 1 && position <= width / 2)
                                || (action == 2 && position > width / 2)
                            {
                                group.first as u32
                            } else {
                                group
                                    .first
                                    .wrapping_div(width)
                                    .wrapping_mul(if action == 1 {
                                        width - position
                                    } else {
                                        position
                                    })
                                    .wrapping_mul(2) as u32
                            }
                        }
                    };
                    publish(
                        &mut next,
                        &mut batch,
                        control,
                        u32::from(edit.origin) + index as u32,
                        value,
                        0,
                    )?;
                }
                31 => {
                    let response = self.filter.prepare_response(
                        &next.assignments,
                        edit.origin,
                        edit.value,
                        control,
                    )?;
                    batch.extend(&response.batch)?;
                    next.assignments = response.next;
                }
                25 | 47..=53 => {
                    self.prepare_wah_action(&mut next, &mut batch, edit, index, group.action)?
                }
                41 if matches!(edit.kind, 4 | 5 | 30) => {
                    if next.midi_binding.source != u32::from(edit.value) {
                        let source = edit.parameters[match edit.kind {
                            4 => 15,
                            5 => 16,
                            _ => 19,
                        }];
                        next.midi_binding = MasterMidiBinding {
                            source: u32::from(source),
                            values: [edit.midi.value(4, source)?.abs(), 0],
                        };
                        if source != 0 {
                            let value = edit.midi.value(4, source)?;
                            if value != next.midi_binding.values[0] {
                                if edit.kind == 4 && edit.parameters[5] == 1 {
                                    self.filter_frequency(&mut next, &mut batch, edit, value)?;
                                } else if edit.kind == 5 && edit.parameters[4] == 2 {
                                    let level = effect_controller_level(value);
                                    let level = if level < 0
                                        && !edit.polarity.bipolar(edit.parameters[16])
                                    {
                                        level.wrapping_neg()
                                    } else {
                                        level
                                    };
                                    batch.push_direct(u32::from(edit.origin) + 5, level as u32)?;
                                } else if edit.kind == 30 && edit.parameters[7] == 2 {
                                    let level = effect_controller_level(value);
                                    let level = if edit.polarity.bipolar(edit.parameters[19]) {
                                        let v = level.wrapping_add(0x7fffff);
                                        v.wrapping_add(i32::from(v < 0)) >> 1
                                    } else {
                                        level
                                    };
                                    batch.push_direct(u32::from(edit.origin) + 5, level as u32)?;
                                }
                                next.midi_binding.values[0] = value;
                            }
                        }
                    }
                }
                43 => batch.extend(&self.filter.prepare_trim(
                    edit.origin,
                    edit.parameters[3],
                    edit.parameters[4],
                )?)?,
                44 => {
                    if edit.parameters[5] == 1 {
                        let value = edit
                            .midi
                            .value(4, next.midi_binding.source.try_into().ok()?)?;
                        self.filter_frequency(&mut next, &mut batch, edit, value)?;
                    }
                    // SYS07771C sets the slot-8 dirty flag after calculation.
                    next.filter_cache.dirty = 1;
                }
                45 => {
                    let instance = EffectRoutingInstance {
                        kind: 4,
                        origin: edit.origin,
                        parameters: edit.parameters,
                    };
                    let origin = u32::from(edit.origin);
                    if edit.direct_switch == 0 {
                        batch.push_direct(origin, 0x7fffff)?;
                        batch.push_direct(origin + 1, 0)?;
                        batch.extend(&self.flanger.routing.prepare_master(
                            edit.stored_effect_type,
                            &instance,
                            1,
                        )?)?;
                        batch.push_command(0, 0x01000014)?;
                    }
                    let selected = [2, 1, 0, 3, 4][usize::from(edit.value)];
                    for offset in 0..5 {
                        batch.push_direct(
                            origin + 10 + offset,
                            if offset == selected { 0x7fffff } else { 0 },
                        )?;
                    }
                    if edit.direct_switch == 0 && edit.stored_enabled {
                        batch.push_command(0, 0x01000028)?;
                        let mix = EffectMix::compile(
                            EffectKind::new(4)?,
                            edit.parameters[0],
                            Default::default(),
                        )?;
                        batch.push_direct(origin, mix.dry as u32)?;
                        batch.push_direct(origin + 1, mix.wet as u32)?;
                        batch.extend(&self.flanger.routing.prepare_master(
                            edit.stored_effect_type,
                            &instance,
                            0,
                        )?)?;
                    }
                }
                8 => self.cabinet.air(
                    &mut batch,
                    &EffectRoutingInstance {
                        kind: 8,
                        origin: edit.origin,
                        parameters: edit.parameters,
                    },
                )?,
                36 => {
                    let instance = EffectRoutingInstance {
                        kind: 8,
                        origin: edit.origin,
                        parameters: edit.parameters,
                    };
                    let origin = u32::from(edit.origin);
                    if edit.direct_switch == 0 {
                        batch.push_direct(origin, 0x7fffff)?;
                        batch.push_direct(origin + 1, 0)?;
                        batch.extend(&self.flanger.routing.prepare_master(
                            edit.stored_effect_type,
                            &instance,
                            1,
                        )?)?;
                        batch.push_command(0, 0x01000014)?;
                    }
                    for (i, value) in self.cabinet.coefficients[usize::from(edit.parameters[1])]
                        .iter()
                        .copied()
                        .enumerate()
                    {
                        batch.push_direct(origin + 11 + i as u32, value)?;
                    }
                    self.cabinet.air(&mut batch, &instance)?;
                    if edit.direct_switch == 0 && edit.stored_enabled {
                        batch.extend(
                            &self
                                .flanger
                                .routing
                                .prepare_master_input_mute(edit.stored_effect_type, &instance)?,
                        )?;
                        batch.push_command(0, 0x01000028)?;
                        let mix = EffectMix::compile(
                            EffectKind::new(8)?,
                            edit.parameters[0],
                            Default::default(),
                        )?;
                        batch.push_direct(origin, mix.dry as u32)?;
                        batch.push_direct(origin + 1, mix.wet as u32)?;
                        batch.extend(&self.flanger.routing.prepare_master(
                            edit.stored_effect_type,
                            &instance,
                            0,
                        )?)?;
                    }
                }
                37 => {
                    publish(
                        &mut next,
                        &mut batch,
                        control,
                        u32::from(edit.origin) + 10,
                        self.flanger.feedback(&edit.parameters)?,
                        0,
                    )?;
                }
                38 if edit.kind == 22 => {
                    let p = edit.parameters;
                    let feedback = self.flanger.feedback(&p)?;
                    let time = if p[1] == 0 {
                        encode_delay_frames(
                            u32::from(self.flanger.milliseconds[usize::from(p[2])]) * 48 / 10,
                            7,
                        )
                    } else {
                        self.flanger.cutoff[usize::from(p[3])]
                    };
                    batch.push_direct(u32::from(edit.origin) + 10, feedback)?;
                    let owner = EffectInterpolationControl::from_owners(
                        edit.direct_switch,
                        if p[1] == 1 { 3 } else { 2 },
                        next.owner,
                        0,
                        true,
                    );
                    for offset in [7, 9] {
                        publish(
                            &mut next,
                            &mut batch,
                            owner,
                            u32::from(edit.origin) + offset,
                            time,
                            1,
                        )?;
                    }
                }
                39 => {
                    let p = edit.parameters;
                    if index <= 1 {
                        let mix = EffectMix::compile(
                            EffectKind::new(edit.kind)?,
                            p[0],
                            MixContext {
                                byte1: p[1],
                                byte5: p[5],
                                byte6: p[6],
                            },
                        )?;
                        let owner = EffectInterpolationControl::from_owners(
                            edit.direct_switch,
                            0,
                            next.owner,
                            0,
                            true,
                        );
                        for (offset, value) in [mix.dry, mix.wet].into_iter().enumerate() {
                            publish(
                                &mut next,
                                &mut batch,
                                owner,
                                u32::from(edit.origin) + offset as u32,
                                value as u32,
                                0,
                            )?;
                        }
                    } else if edit.kind == 23 && index == 6 {
                        let value = self.flanger.feedback_range.compile(
                            EffectCurve::Linear,
                            i32::from(p[4]),
                            0x7fffff,
                            0,
                        )?;
                        let value = if p[5] == 1 {
                            value.wrapping_neg()
                        } else {
                            value
                        };
                        let owner = EffectInterpolationControl::from_owners(
                            edit.direct_switch,
                            4,
                            next.owner,
                            0,
                            true,
                        );
                        publish(
                            &mut next,
                            &mut batch,
                            owner,
                            u32::from(edit.origin) + 6,
                            value as u32,
                            0,
                        )?;
                    } else {
                        return None;
                    }
                }
                40 => {
                    let instance = EffectRoutingInstance {
                        kind: edit.kind,
                        origin: edit.origin,
                        parameters: edit.parameters,
                    };
                    if edit.direct_switch == 0 {
                        batch.extend(&self.flanger.routing.prepare_master(
                            edit.stored_effect_type,
                            &instance,
                            1,
                        )?)?;
                    }
                    for (i, value) in if edit.parameters[1] == 0 {
                        [0x7fffff, 0]
                    } else {
                        [0, 0x7fffff]
                    }
                    .into_iter()
                    .enumerate()
                    {
                        batch.push_direct(u32::from(edit.origin) + 18 + i as u32, value)?;
                    }
                    if edit.direct_switch == 0 {
                        batch.extend(&self.flanger.routing.prepare_master(
                            edit.stored_effect_type,
                            &instance,
                            0,
                        )?)?;
                    }
                }
                77 => {
                    let parameter = if edit.parameters[1] == 1 { 3 } else { 2 };
                    let owner = EffectInterpolationControl::from_owners(
                        edit.direct_switch,
                        parameter,
                        next.owner,
                        0,
                        true,
                    );
                    if owner.enabled_argument != 0 {
                        for record in &mut next.assignments.slots {
                            if record.target == UNASSIGNED_TARGET {
                                continue;
                            }
                            batch.push_direct(record.target, record.last_value)?;
                            record.target = UNASSIGNED_TARGET;
                            record.last_value = 0;
                            for (i, value) in [UNASSIGNED_TARGET, 0, 0x7a9765, 0x5689a]
                                .into_iter()
                                .enumerate()
                            {
                                batch.push_command(
                                    record.indices[i],
                                    (value & 0xffffff)
                                        | if edit.direct_switch == 0 {
                                            (0x84 - i as u32) << 24
                                        } else {
                                            0
                                        },
                                )?;
                            }
                        }
                        next.update_marker = 0;
                    }
                }
                26 => {
                    if edit.kind == 6 {
                        let old = i32::from(edit.previous_parameters[parameter]) - 64;
                        let current = i32::from(edit.parameters[parameter]) - 64;
                        if old * current < 0 {
                            batch.push_command(0, 0x01000014)?;
                        }
                    }
                    publish(
                        &mut next,
                        &mut batch,
                        control,
                        u32::from(edit.origin) + index as u32,
                        *self
                            .equalizer_gain
                            .get(usize::from(edit.value.checked_sub(28)?))?,
                        0,
                    )?;
                }
                35 if matches!(edit.kind, 11 | 12 | 20) => {
                    let low = if edit.kind == 12 { 5 } else { 7 };
                    let values = match edit.parameter {
                        p if p == low => self.equalizer.low_shelf(
                            27,
                            (i32::from(edit.parameters[usize::from(low)]) - 64) as i8,
                        )?,
                        p if p == low + 1 => self.equalizer.high_shelf(
                            36,
                            (i32::from(edit.parameters[usize::from(low + 1)]) - 64) as i8,
                        )?,
                        _ => return None,
                    };
                    for (i, value) in values.into_iter().enumerate() {
                        batch
                            .push_direct(u32::from(edit.origin) + index as u32 + i as u32, value)?;
                    }
                }
                35 => self.equalizer_band(&mut next, &mut batch, edit, index)?,
                38 if matches!(edit.kind, 13..=20) => {
                    self.delay_time(&mut next, &mut batch, edit)?;
                }
                20..=22 => {
                    let state = DecimatorEffectState {
                        pre_lpf: edit.parameters[1],
                        stored_sample_rate: edit.parameters[3],
                    };
                    let change = match group.action {
                        20 => DecimatorEffectChange::BitDepth {
                            value: edit.value,
                            coefficient_offset: index as u32,
                        },
                        21 => DecimatorEffectChange::SampleRate {
                            value: edit.value,
                            state,
                        },
                        _ => DecimatorEffectChange::PreLpf {
                            value: edit.value,
                            state,
                        },
                    };
                    let c = EffectInterpolationControl::for_decimator_parameter(
                        edit.direct_switch,
                        edit.parameter,
                        edit.owner,
                        0,
                        true,
                    );
                    let p = self
                        .decimator
                        .prepare(&next.assignments, edit.origin, change, c)?;
                    batch.extend(&p.batch)?;
                    next.assignments = p.next;
                }
                28 => {
                    let value = group.range.compile(
                        EffectCurve::Linear,
                        i32::from(edit.value),
                        group.first,
                        group.second,
                    )? as u32;
                    let dependent = *self
                        .dynamics
                        .raw_master_sensitivity
                        .get(usize::from(edit.value))?;
                    publish(
                        &mut next,
                        &mut batch,
                        control,
                        u32::from(edit.origin) + 11,
                        value,
                        0,
                    )?;
                    let value = if group.action == 16 {
                        value.wrapping_mul(2).wrapping_add(0xff800001)
                    } else if group.action == 72 {
                        0x7fffffu32.wrapping_sub(value)
                    } else {
                        value
                    };
                    publish(
                        &mut next,
                        &mut batch,
                        control,
                        u32::from(edit.origin) + 28,
                        dependent,
                        0,
                    )?;
                    batch.push_direct(u32::from(edit.origin) + 12, value)?;
                }
                38 => {
                    let p = edit.parameters;
                    let mut mapped = [0; 20];
                    let milliseconds = if edit.kind == 26 {
                        mapped[2] = p[3];
                        mapped[3] = p[4];
                        mapped[4] = p[5];
                        mapped[5] = p[5];
                        mapped[6] = p[6];
                        mapped[7] = p[6];
                        &self.pitch.time.stereo_milliseconds
                    } else if edit.kind == 27 {
                        mapped[2] = p[1];
                        mapped[3] = p[2];
                        mapped[4] = p[3];
                        mapped[5] = p[3];
                        mapped[6] = p[4];
                        mapped[7] = p[4];
                        &self.grain_milliseconds
                    } else {
                        return None;
                    };
                    let t = self.pitch.time.two_channel(
                        &mapped,
                        next.delay,
                        edit.clock,
                        next.delay.capacity,
                        milliseconds,
                    )?;
                    next.delay = t.state;
                    if edit.kind == 26 {
                        for (i, frame) in t.frames[..2].iter().enumerate() {
                            batch.push_direct(
                                u32::from(edit.origin.wrapping_add(11 + i as u16)),
                                encode_delay_frames(*frame, 7),
                            )?;
                        }
                    } else {
                        let tempo = if p[1] == 0 {
                            edit.clock.tempo
                        } else {
                            t.state.cached_tempo
                        };
                        if tempo == 0 {
                            return None;
                        }
                        let beat = 600000 / u32::from(tempo);
                        let period = if p[5] == 0 {
                            self.pitch.grain_period[usize::from(p[6])]
                        } else {
                            divide_192(
                                beat.wrapping_mul(self.pitch.clock_notes[usize::from(p[7])])
                                    .wrapping_mul(48),
                            )
                        };
                        let value = (encode_delay_frames(t.frames[0].min(period), 7).max(0x1700)
                            & 0xffffff)
                            | 0x80000000;
                        next.pending = [value; 2];
                        next.pending_control = 0;
                    }
                }
                42 => {
                    let first = matches!(index, 14 | 16);
                    let source = definition.groups[if first { 14 } else { 15 }];
                    let saturation = edit.parameters[if first { 5 } else { 11 }];
                    let bias = edit.parameters[if first { 4 } else { 10 }];
                    let coefficient = source.range.compile(
                        EffectCurve::Linear,
                        i32::from(saturation),
                        source.first,
                        source.second,
                    )?;
                    let product = coefficient.wrapping_mul(i32::from(bias));
                    let half = product.wrapping_add(i32::from(product < 0)) >> 1;
                    let divisor = i32::from(group.range.maximum) - i32::from(group.range.minimum);
                    if divisor == 0 {
                        return None;
                    }
                    batch.push_direct(
                        u32::from(edit.origin) + if first { 16 } else { 17 },
                        half.wrapping_div(divisor) as u32,
                    )?;
                }
                61 => {
                    let p = edit.parameters;
                    let value = self.ring.frequency_for_global_note(&p, edit.current_note)?;
                    let mut owner = EffectInterpolationControl::from_owners(
                        edit.direct_switch,
                        2,
                        edit.owner,
                        0,
                        true,
                    );
                    if owner.enabled_argument == 0 {
                        owner = EffectInterpolationControl::from_owners(
                            edit.direct_switch,
                            3,
                            edit.owner,
                            0,
                            true,
                        );
                    }
                    publish(
                        &mut next,
                        &mut batch,
                        owner,
                        u32::from(edit.origin) + 6,
                        value,
                        0,
                    )?;
                }
                62 => {
                    let w = EffectSelectorWrites::compile(
                        edit.origin,
                        index as u32,
                        u32::from(edit.value),
                    );
                    for w in &w.words[..usize::from(w.count)] {
                        batch.push_direct(u32::from(w.address), w.tagged_value)?;
                    }
                }
                69 => {
                    let owner = EffectInterpolationControl::from_owners(
                        edit.direct_switch,
                        1,
                        edit.owner,
                        0,
                        true,
                    );
                    publish(
                        &mut next,
                        &mut batch,
                        owner,
                        u32::from(edit.origin) + index as u32,
                        self.pitch.pitch_word(&edit.parameters)?,
                        0,
                    )?;
                }
                70 => {
                    let v = match edit.value {
                        0 => [0x1fff, 0xfffff],
                        1 => [0xfff, 0x1fffff],
                        2 => [0x3ff, 0x7fffff],
                        _ => return None,
                    };
                    for (i, v) in v.into_iter().enumerate() {
                        batch.push_direct(u32::from(edit.origin) + 9 + i as u32, v)?;
                    }
                }
                71 => {
                    let p = edit.parameters;
                    let v = self.pitch.feedback_range.compile(
                        EffectCurve::Quadratic,
                        i32::from(p[8]),
                        if p[7] == 0 { 0x4ccccc } else { 0x7fffff },
                        0,
                    )? as u32;
                    for (i, v) in if p[7] == 0 { [v, 0] } else { [0, v] }
                        .into_iter()
                        .enumerate()
                    {
                        publish(
                            &mut next,
                            &mut batch,
                            control,
                            u32::from(edit.origin) + 13 + i as u32,
                            v,
                            0,
                        )?;
                    }
                }
                34 => {
                    let p = next.lfo.prepare(
                        &edit.parameters,
                        definition.lfo_mapping,
                        EffectLfoSlot::new(8)?,
                        1,
                        edit.clock_rate,
                        &self.tempo,
                    )?;
                    if let Some(p) = p {
                        next.lfo = p.program;
                    }
                    batch.extend(&EffectParameterBatch::from_lfo(p))?;
                }
                60 => {
                    let speed = edit.parameters[2];
                    let lookup = usize::from(speed.checked_sub(1)?);
                    for (i, table) in self.ensemble.speed_words.iter().enumerate() {
                        publish(
                            &mut next,
                            &mut batch,
                            control,
                            u32::from(edit.origin) + 6 + i as u32,
                            *table.get(lookup)?,
                            0,
                        )?;
                    }
                    batch.push_direct(
                        u32::from(edit.origin) + 10,
                        self.ensemble.speed_shape_range.compile(
                            EffectCurve::EaseOut,
                            i32::from(speed),
                            0xaaaa,
                            0xe0000,
                        )? as u32,
                    )?;
                }
                4 | 6 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19 | 27 | 29 | 30 | 32
                | 33 | 72 | 76 => {
                    let value = match group.action {
                        4 | 6 => {
                            let mix = EffectMix::compile(
                                EffectKind::new(edit.kind)?,
                                edit.value,
                                Default::default(),
                            )?;
                            if group.action == 6 {
                                mix.dry as u32
                            } else {
                                mix.wet as u32
                            }
                        }
                        17 | 18 | 19 | 27 | 29 | 30 => self
                            .dynamics
                            .raw_word(group.action, i32::from(edit.value))?,
                        32 => *self.modulation_rate.get(usize::from(edit.value))?,
                        33 => *self.distortion_gain.get(usize::from(edit.value))?,
                        action => group.range.compile(
                            match action {
                                9 | 76 => EffectCurve::Quadratic,
                                10 => EffectCurve::OffsetQuadratic,
                                12 => EffectCurve::OffsetScale,
                                13 | 16 | 72 => EffectCurve::EaseOut,
                                14 => EffectCurve::InverseScale,
                                15 => EffectCurve::Select,
                                _ => EffectCurve::Linear,
                            },
                            i32::from(edit.value),
                            group.first,
                            group.second,
                        )? as u32,
                    };
                    let value = if group.action == 16 {
                        value.wrapping_mul(2).wrapping_add(0xff800001)
                    } else if group.action == 72 {
                        0x7fffffu32.wrapping_sub(value)
                    } else {
                        value
                    };
                    publish(
                        &mut next,
                        &mut batch,
                        control,
                        u32::from(edit.origin) + index as u32,
                        value,
                        u8::from(group.action == 76),
                    )?;
                }
                _ => return None,
            }
        }
        Some(PreparedMasterEdit {
            next,
            batch,
            program_writes,
            body_program,
        })
    }
}
