//! Unified native Insert parameter and whole SYS07ADE4 initial-mask control.
use crate::{
    auto_pan_delay::{AutoPanDelayEdit, AutoPanDelayRack, AutoPanDelayTables},
    cabinet_effect::{CabinetEffectRack, CabinetParameterEdit},
    chorus_effect::{ChorusEffectRack, ChorusEffectTables, ChorusParameterEdit},
    decimator_effect::{DecimatorEffectChange, DecimatorEffectState},
    delay_effect::{DelayEffectKind, DelayEffectRack, DelayEffectTables, DelayParameterEdit},
    delay_time::{DelayClock, DelayTimeState},
    dynamics_effect::{DynamicsEffectKind, DynamicsParameterChange},
    early_reflect_effect::{EarlyReflectEffectRack, EarlyReflectParameterEdit},
    effect_control::PreparedEffect,
    effect_curves::EffectParameterRange,
    effect_lfo_program::{EffectLfoMapping, EffectLfoSlot},
    effect_parameters::{EffectInterpolationControl, EffectParameterBatch},
    effect_program_staging::{EffectProgramStaging, EffectStagedProgramWrite},
    effect_routing::EffectRoutingInstance,
    ensemble_effect::EnsembleParameterEdit,
    equalizer_effect::{
        EqualizerEffectInstance, EqualizerEffectKind, EqualizerEffectRack, EqualizerEffectTables,
        EqualizerParameterEdit,
    },
    filter_effect_parameters::{
        FilterEffectInstance, FilterEffectRack, FilterParameterEdit, FilterParameterTables,
    },
    flanger_phaser_effect::{FlangerPhaserEdit, FlangerPhaserKind, FlangerPhaserRack},
    insert_effect_construction::InsertPatch,
    master_effect_control::MasterControlTables,
    mixed_effect_midi::{MixedEffectMidiFrame, MixedEffectMidiState},
    mod_delay::{ModDelayEdit, ModDelayKind, ModDelayRack, ModDelayTables},
    pitch_grain_shifter::{GrainPendingTime, PitchGrainEdit, PitchGrainKind, PitchGrainRack},
    program::Program,
    reverb_effect::{ReverbEffectRack, ReverbEffectTables, ReverbParameterEdit},
    rotary_effect::{RotaryEdit, RotaryInstance, RotaryRack},
    talking_effect::{TalkingEdit, TalkingInstance, TalkingRack},
    tremolo_ring_mod_effect::{TremoloRingModEdit, TremoloRingModKind, TremoloRingModRack},
    tube_effect::{TubeEffectTables, TubeParameterEdit},
    vibrato_effect::{VibratoEdit, VibratoRack, VibratoTables},
    wah_effect::{WahEffectInstance, WahEffectRack, WahParameterEdit, WahParameterTables},
};
pub struct InsertControlDefinition {
    pub parameter_count: usize,
    pub initialization_mask: u32,
    pub ranges: [EffectParameterRange; 20],
    pub lfo_mapping: EffectLfoMapping,
}
pub struct InsertControlTables {
    pub definitions: [InsertControlDefinition; 31],
    pub common: MasterControlTables,
    pub tube: TubeEffectTables,
    pub equalizer: EqualizerEffectTables,
    pub reverb: ReverbEffectTables,
    pub delay: DelayEffectTables,
    pub auto_pan: AutoPanDelayTables,
    pub mod_delay: ModDelayTables,
    pub chorus: ChorusEffectTables,
    pub vibrato: VibratoTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertControlState {
    pub midi: MixedEffectMidiState,
    pub scratch: [u32; 73],
    pub staging: EffectProgramStaging,
    pub prefix_origins: [u16; 8],
    pub body_origins: [u16; 8],
    pub relocation_origins: [u16; 8],
}
#[derive(Clone, Copy)]
pub struct InsertControlContext<'a> {
    pub program: &'a Program,
    pub midi: MixedEffectMidiFrame,
    pub secondary_switch: u32,
    pub clock_rate: u32,
    pub clock: DelayClock,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertControlStep {
    pub batch: EffectParameterBatch,
    pub program_writes: [Option<EffectStagedProgramWrite>; 2],
    pub body_program: Option<PreparedEffect>,
}
impl InsertControlStep {
    fn batch(batch: EffectParameterBatch) -> Self {
        Self {
            batch,
            program_writes: [None; 2],
            body_program: None,
        }
    }
}
pub struct PreparedInsertInitialMask {
    pub next: InsertControlState,
    pub steps: [Option<InsertControlStep>; 20],
    pub step_count: usize,
}
impl PreparedInsertInitialMask {
    pub fn steps(&self) -> impl Iterator<Item = &InsertControlStep> {
        self.steps[..self.step_count].iter().flatten()
    }
}
fn times(state: &InsertControlState) -> [DelayTimeState; 8] {
    state.midi.inserts.map(|i| DelayTimeState {
        cached_tempo: i.buffer.cached_tempo,
        capacity: i.buffer.layout.frames,
        ratio: i.buffer.ratio,
        limited: i.buffer.limited,
    })
}
fn store_times(state: &mut InsertControlState, values: [DelayTimeState; 8]) {
    for (i, t) in state.midi.inserts.iter_mut().zip(values) {
        i.buffer.cached_tempo = t.cached_tempo;
        i.buffer.layout.frames = t.capacity;
        i.buffer.ratio = t.ratio;
        i.buffer.limited = t.limited;
    }
}
fn routing(state: &InsertControlState, master_origin: u16) -> [EffectRoutingInstance; 9] {
    core::array::from_fn(|slot| {
        if slot < 8 {
            let i = state.midi.inserts[slot];
            EffectRoutingInstance {
                kind: i.buffer.kind,
                origin: i.buffer.origin,
                parameters: i.buffer.parameters,
            }
        } else {
            EffectRoutingInstance {
                kind: state.midi.master.kind,
                origin: master_origin,
                parameters: state.midi.master.parameters,
            }
        }
    })
}
impl InsertControlTables {
    fn common_filter_mapping(&self) -> EffectLfoMapping {
        self.definitions[4].lfo_mapping
    }
    fn decimator_mapping(&self) -> EffectLfoMapping {
        self.definitions[10].lfo_mapping
    }
    pub fn prepare_initial_mask(
        &self,
        state: &InsertControlState,
        slot: u8,
        patch: InsertPatch,
        context: InsertControlContext<'_>,
    ) -> Option<PreparedInsertInitialMask> {
        let i = state.midi.inserts.get(usize::from(slot))?;
        let d = self.definitions.get(usize::from(i.buffer.kind))?;
        let mut out = PreparedInsertInitialMask {
            next: *state,
            steps: [None; 20],
            step_count: 0,
        };
        for parameter in 1..d.parameter_count {
            if d.initialization_mask & (0x80000000 >> parameter) == 0 {
                continue;
            }
            let (next, step) = self.prepare_stored_parameter(
                &out.next,
                slot,
                parameter as u8,
                patch.parameters[parameter],
                context,
            )?;
            out.next = next;
            *out.steps.get_mut(out.step_count)? = Some(step);
            out.step_count += 1;
        }
        Some(out)
    }
    pub fn prepare_parameter(
        &self,
        state: &InsertControlState,
        slot: u8,
        parameter: u8,
        value: u8,
        context: InsertControlContext<'_>,
    ) -> Option<(InsertControlState, InsertControlStep)> {
        self.prepare_parameter_inner(state, slot, parameter, value, context, false)
    }
    /// Initial stored dynamics masks retain the signed MOV.B argument from
    /// SYS07AC52, separately from the already clamped instance parameters.
    /// Other effect families still require their legal argument ranges.
    pub fn prepare_stored_parameter(
        &self,
        state: &InsertControlState,
        slot: u8,
        parameter: u8,
        value: u8,
        context: InsertControlContext<'_>,
    ) -> Option<(InsertControlState, InsertControlStep)> {
        self.prepare_parameter_inner(state, slot, parameter, value, context, true)
    }
    fn prepare_parameter_inner(
        &self,
        state: &InsertControlState,
        slot: u8,
        parameter: u8,
        value: u8,
        context: InsertControlContext<'_>,
        stored_argument: bool,
    ) -> Option<(InsertControlState, InsertControlStep)> {
        let index = usize::from(slot);
        let i = *state.midi.inserts.get(index)?;
        let kind = i.buffer.kind;
        let d = self.definitions.get(usize::from(kind))?;
        let p = usize::from(parameter);
        if p >= d.parameter_count {
            return None;
        }
        let range = d.ranges[p];
        let v = i32::from(value) - i32::from(range.encoded_zero);
        if (!stored_argument || !matches!(kind, 1..=3))
            && !(i32::from(range.minimum)..=i32::from(range.maximum)).contains(&v)
        {
            return None;
        }
        let mut next = *state;
        let parameters = i.buffer.parameters;
        let origin = i.buffer.origin;
        let owners = i.owners;
        let direct = context.midi.direct_switch;
        let assignments = state.midi.master.control.assignments;
        let control =
            EffectInterpolationControl::from_owners(direct, parameter, owners[0], owners[1], false);
        // These dependency records contain only the source-bind action. Their
        // subsequent whole-rack sweep must visit mixed kinds and active Master.
        if matches!(
            (kind, parameter),
            (4, 15) | (5, 16) | (29, 2 | 3 | 6 | 7 | 9) | (30, 19)
        ) {
            let tested = if matches!(kind, 29 | 30) {
                value.min(12)
            } else {
                value
            };
            let mut batch = EffectParameterBatch::from_lfo(None);
            if i.controller_source != u32::from(tested) {
                let source = parameters[if kind == 29 { 2 } else { p }];
                next.midi.inserts[index].controller_source = u32::from(source);
                next.midi.inserts[index].controller_values =
                    [context.midi.midi.value(slot / 2, source)?.abs(), 0];
                if kind == 29 {
                    let source = if parameters[4] == 0 {
                        parameters[6]
                    } else {
                        parameters[9]
                    };
                    next.midi.inserts[index].controller_values[1] =
                        context.midi.midi.value(slot / 2, source)?.abs();
                }
                let prepared = self.common.prepare_mixed_midi(
                    &next.midi,
                    MixedEffectMidiFrame {
                        force_refresh: false,
                        ..context.midi
                    },
                )?;
                next.midi = prepared.next;
                batch = prepared.batch;
            }
            // Wah source rebinding also recomputes group55/action53 after
            // the whole mixed MIDI sweep, even when the binding is unchanged.
            if kind == 5 {
                let word = crate::wah_effect::WahEffectTables::controller_polarity_word(
                    &parameters,
                    context.midi.polarity,
                );
                batch.push_direct(u32::from(origin) + 55, word)?;
            }
            return Some((next, InsertControlStep::batch(batch)));
        }
        let lfos = state.midi.inserts.map(|i| i.lfo);
        let step = match kind {
            0 => InsertControlStep::batch(EffectParameterBatch::from_lfo(None)),
            1..=3 => {
                let change = DynamicsParameterChange {
                    kind: DynamicsEffectKind::from_effect_type(kind)?,
                    origin,
                    parameter,
                    value,
                    interpolation: control,
                };
                let a = if stored_argument {
                    self.common
                        .dynamics
                        .prepare_stored_insert_argument(&assignments, change)?
                } else {
                    self.common.dynamics.prepare(&assignments, change)?
                };
                next.midi.master.control.assignments = a.next;
                InsertControlStep::batch(a.batch)
            }
            4 => {
                let rack = FilterEffectRack {
                    instances: core::array::from_fn(|n| {
                        let a = state.midi.inserts[n];
                        FilterEffectInstance {
                            kind: a.buffer.kind,
                            parameters: a.buffer.parameters,
                            origin: a.buffer.origin,
                            owners: a.owners,
                            lfo: a.lfo,
                            controller_source: if n == index { a.controller_source } else { 0 },
                            controller_value: a.controller_values[0],
                            secondary_value: a.controller_values[1],
                        }
                    }),
                    caches: core::array::from_fn(|n| {
                        if n < 8 {
                            state.midi.insert_filter_caches[n]
                        } else {
                            state.midi.master.control.filter_cache
                        }
                    }),
                    assignments,
                };
                let tables = FilterParameterTables {
                    coefficients: &self.common.filter,
                    routing: &self.common.flanger.routing,
                    tempo: &self.common.tempo,
                    lfo_mapping: self.common_filter_mapping(),
                };
                let a = rack.prepare(
                    FilterParameterEdit {
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                        clock_rate: context.clock_rate,
                    },
                    &tables,
                    &context.midi.midi,
                    context.program,
                )?;
                next.midi.inserts[index].lfo = a.next.instances[index].lfo;
                next.midi
                    .insert_filter_caches
                    .copy_from_slice(&a.next.caches[..8]);
                next.midi.master.control.filter_cache = a.next.caches[8];
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            5 => {
                let rack = WahEffectRack {
                    instances: core::array::from_fn(|n| {
                        let a = state.midi.inserts[n];
                        WahEffectInstance {
                            kind: a.buffer.kind,
                            parameters: a.buffer.parameters,
                            previous_parameters: a.previous_parameters,
                            origin: a.buffer.origin,
                            owners: a.owners,
                            lfo: a.lfo,
                            controller_source: if n == index { a.controller_source } else { 0 },
                            controller_value: a.controller_values[0],
                            secondary_value: a.controller_values[1],
                        }
                    }),
                    assignments,
                };
                let tables = WahParameterTables {
                    coefficients: &self.common.wah,
                    routing: &self.common.flanger.routing,
                    tempo: &self.common.tempo,
                    midi: &context.midi.midi,
                    polarity: context.midi.polarity,
                };
                let a = self.common.wah.prepare(
                    &rack,
                    WahParameterEdit {
                        slot,
                        parameter,
                        value,
                        parameters,
                        previous_parameters: i.previous_parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                        clock_rate: context.clock_rate,
                        tempo: context.clock.tempo,
                    },
                    &tables,
                    context.program,
                )?;
                next.midi.inserts[index].lfo = a.next.instances[index].lfo;
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            6 | 7 => {
                let rack = EqualizerEffectRack {
                    instances: state.midi.inserts.map(|a| EqualizerEffectInstance {
                        parameters: a.buffer.parameters,
                        previous_parameters: a.previous_parameters,
                        origin: a.buffer.origin,
                        owners: a.owners,
                    }),
                    assignments,
                };
                let a = self.equalizer.prepare(
                    &rack,
                    EqualizerParameterEdit {
                        kind: EqualizerEffectKind::from_type(kind)?,
                        slot,
                        parameter,
                        value,
                        parameters,
                        previous_parameters: i.previous_parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                    },
                    &self.common.equalizer,
                    context.program,
                )?;
                next.midi.inserts[index].owners = a.next.instances[index].owners;
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            8 => {
                let rack = CabinetEffectRack {
                    instances: routing(state, context.midi.master_origin),
                    assignments,
                };
                let a = self.common.cabinet.prepare(
                    &rack,
                    CabinetParameterEdit {
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                    },
                    &self.common.flanger.routing,
                    context.program,
                )?;
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            9 => {
                let a = self.tube.prepare(
                    &assignments,
                    TubeParameterEdit {
                        origin,
                        parameter,
                        value,
                        parameters,
                        interpolation: control,
                    },
                )?;
                next.midi.master.control.assignments = a.next;
                InsertControlStep::batch(a.batch)
            }
            10 => {
                if parameter < 7 {
                    let change = DecimatorEffectChange::from_parameter(
                        parameter,
                        value,
                        DecimatorEffectState {
                            pre_lpf: parameters[1],
                            stored_sample_rate: parameters[3],
                        },
                    )?;
                    let a = self.common.decimator.prepare(
                        &assignments,
                        origin,
                        change,
                        EffectInterpolationControl::for_decimator_parameter(
                            direct, parameter, owners[0], owners[1], false,
                        ),
                    )?;
                    next.midi.master.control.assignments = a.next;
                    InsertControlStep::batch(a.batch)
                } else {
                    let publication = i.lfo.prepare(
                        &parameters,
                        self.decimator_mapping(),
                        EffectLfoSlot::new(slot)?,
                        0,
                        context.clock_rate,
                        &self.common.tempo,
                    )?;
                    if let Some(p) = publication {
                        next.midi.inserts[index].lfo = p.program;
                    }
                    InsertControlStep::batch(EffectParameterBatch::from_lfo(publication))
                }
            }
            11 => {
                let rack = ReverbEffectRack {
                    instances: state.midi.inserts.map(|i| i.buffer),
                    assignments,
                    scratch: state.scratch[..49].try_into().ok()?,
                };
                let a = self.reverb.prepare(
                    &rack,
                    ReverbParameterEdit {
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                        secondary_switch: context.secondary_switch,
                        clock: context.clock,
                    },
                )?;
                for (i, b) in next.midi.inserts.iter_mut().zip(a.next.instances) {
                    i.buffer = b;
                }
                next.scratch[..49].copy_from_slice(&a.next.scratch);
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            12 => {
                let rack = EarlyReflectEffectRack {
                    instances: state.midi.inserts.map(|i| i.buffer),
                    program_origins: state.prefix_origins,
                    assignments,
                    staging: state.staging,
                };
                let a = self.common.early_reflect.prepare(
                    &rack,
                    context.program,
                    EarlyReflectParameterEdit {
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                    },
                )?;
                for (i, b) in next.midi.inserts.iter_mut().zip(a.next.instances) {
                    i.buffer = b;
                }
                next.midi.master.control.assignments = a.next.assignments;
                next.staging = a.next.staging;
                InsertControlStep {
                    batch: a.batch,
                    program_writes: a.program_writes,
                    body_program: None,
                }
            }
            13 | 14 => {
                let rack = DelayEffectRack {
                    states: times(state),
                    assignments,
                };
                let a = self.delay.prepare(
                    &rack,
                    DelayParameterEdit {
                        kind: if kind == 13 {
                            DelayEffectKind::Lcr
                        } else {
                            DelayEffectKind::Stereo
                        },
                        slot,
                        origin,
                        parameter,
                        value,
                        parameters,
                        owners,
                        direct_switch: direct,
                        clock: context.clock,
                    },
                )?;
                store_times(&mut next, a.next.states);
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            15 | 16 => {
                let rack = AutoPanDelayRack {
                    times: times(state),
                    lfos,
                    assignments,
                };
                let a = self.auto_pan.prepare(
                    &rack,
                    AutoPanDelayEdit {
                        stereo: kind == 16,
                        slot,
                        origin,
                        parameter,
                        value,
                        parameters,
                        owners,
                        direct_switch: direct,
                        clock: context.clock,
                        clock_rate: context.clock_rate,
                    },
                )?;
                store_times(&mut next, a.next.times);
                for (i, l) in next.midi.inserts.iter_mut().zip(a.next.lfos) {
                    i.lfo = l;
                }
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            17..=19 => {
                let rack = ModDelayRack {
                    times: times(state),
                    lfos,
                    assignments,
                };
                let a = self.mod_delay.prepare(
                    &rack,
                    ModDelayEdit {
                        kind: match kind {
                            17 => ModDelayKind::Mod,
                            18 => ModDelayKind::StereoMod,
                            _ => ModDelayKind::TapeEcho,
                        },
                        slot,
                        origin,
                        parameter,
                        value,
                        parameters,
                        owners,
                        direct_switch: direct,
                        clock: context.clock,
                        clock_rate: context.clock_rate,
                    },
                )?;
                store_times(&mut next, a.next.times);
                for (i, l) in next.midi.inserts.iter_mut().zip(a.next.lfos) {
                    i.lfo = l;
                }
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            20 => {
                let rack = ChorusEffectRack {
                    times: times(state),
                    assignments,
                };
                let a = self.chorus.prepare(
                    &rack,
                    ChorusParameterEdit {
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                        clock: context.clock,
                    },
                    &self.common.equalizer,
                )?;
                store_times(&mut next, a.next.times);
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            21 => {
                let a = self.common.ensemble.prepare(
                    &assignments,
                    EnsembleParameterEdit {
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                    },
                )?;
                next.midi.master.control.assignments = a.next;
                InsertControlStep::batch(a.batch)
            }
            22 | 23 => {
                let rack = FlangerPhaserRack {
                    instances: routing(state, context.midi.master_origin),
                    lfos,
                    assignments,
                    update_marker: state.midi.master.control.update_marker,
                };
                let a = self.common.flanger.prepare(
                    &rack,
                    context.program,
                    FlangerPhaserEdit {
                        kind: if kind == 22 {
                            FlangerPhaserKind::Flanger
                        } else {
                            FlangerPhaserKind::Phaser
                        },
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                        clock_rate: context.clock_rate,
                    },
                )?;
                for (i, l) in next.midi.inserts.iter_mut().zip(a.next.lfos) {
                    i.lfo = l;
                }
                next.midi.master.control.assignments = a.next.assignments;
                next.midi.master.control.update_marker = a.next.update_marker;
                InsertControlStep::batch(a.batch)
            }
            24 | 25 => {
                let rack = TremoloRingModRack { lfos, assignments };
                let a = self.common.ring.prepare(
                    &rack,
                    TremoloRingModEdit {
                        kind: if kind == 24 {
                            TremoloRingModKind::Tremolo
                        } else {
                            TremoloRingModKind::RingMod
                        },
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                        clock_rate: context.clock_rate,
                        current_note: context.midi.current_notes[usize::from(slot / 2)],
                    },
                )?;
                for (i, l) in next.midi.inserts.iter_mut().zip(a.next.lfos) {
                    i.lfo = l;
                }
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            26 | 27 => {
                let rack = PitchGrainRack {
                    times: times(state),
                    pending: state.midi.inserts.map(|i| GrainPendingTime {
                        coefficients: i.buffer.pending_coefficients,
                        control_argument: i.buffer.pending_argument,
                    }),
                    lfos,
                    assignments,
                };
                let a = self.common.pitch.prepare(
                    &rack,
                    PitchGrainEdit {
                        kind: if kind == 26 {
                            PitchGrainKind::Pitch
                        } else {
                            PitchGrainKind::Grain
                        },
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                        clock: context.clock,
                        clock_rate: context.clock_rate,
                    },
                )?;
                store_times(&mut next, a.next.times);
                for ((i, l), p) in next
                    .midi
                    .inserts
                    .iter_mut()
                    .zip(a.next.lfos)
                    .zip(a.next.pending)
                {
                    i.lfo = l;
                    i.buffer.pending_coefficients = p.coefficients;
                    i.buffer.pending_argument = p.control_argument;
                }
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            28 => {
                let rack = VibratoRack { lfos, assignments };
                let a = self.vibrato.prepare(
                    &rack,
                    VibratoEdit {
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                        clock_rate: context.clock_rate,
                    },
                )?;
                for (i, l) in next.midi.inserts.iter_mut().zip(a.next.lfos) {
                    i.lfo = l;
                }
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            29 => {
                let rack = RotaryRack {
                    instances: state.midi.inserts.map(|a| RotaryInstance {
                        kind: a.buffer.kind,
                        parameters: a.buffer.parameters,
                        origin: a.buffer.origin,
                        controller_source: a.controller_source,
                        primary: a.controller_values[0],
                        secondary: a.controller_values[1],
                        mode: a.rotary_mode,
                        speed: a.rotary_speed,
                    }),
                    assignments,
                };
                let a = self.common.rotary.prepare(
                    &rack,
                    RotaryEdit {
                        slot,
                        parameter,
                        value,
                        parameters,
                        origin,
                        owners,
                        direct_switch: direct,
                    },
                    &context.midi.midi,
                )?;
                let b = a.next.instances[index];
                next.midi.inserts[index].controller_values = [b.primary, b.secondary];
                next.midi.inserts[index].rotary_mode = b.mode;
                next.midi.inserts[index].rotary_speed = b.speed;
                next.midi.master.control.assignments = a.next.assignments;
                InsertControlStep::batch(a.batch)
            }
            30 => {
                let rack = TalkingRack {
                    instances: core::array::from_fn(|n| {
                        let a = state.midi.inserts[n];
                        TalkingInstance {
                            kind: a.buffer.kind,
                            parameters: a.buffer.parameters,
                            previous_parameters: a.previous_parameters,
                            owners: a.owners,
                            origin: a.buffer.origin,
                            relocation_origin: state.relocation_origins[n],
                            prefix_origin: state.prefix_origins[n],
                            body_origin: state.body_origins[n],
                            lfo: a.lfo,
                            controller_source: a.controller_source,
                            controller_value: a.controller_values[0],
                            secondary_value: a.controller_values[1],
                        }
                    }),
                    assignments,
                    staging: state.staging,
                };
                let a = self.common.talking.prepare(
                    &rack,
                    context.program,
                    &context.midi.midi,
                    context.midi.polarity,
                    TalkingEdit {
                        slot,
                        parameter,
                        value,
                        parameters,
                        previous_parameters: i.previous_parameters,
                        origin,
                        relocation_origin: state.relocation_origins[index],
                        prefix_origin: state.prefix_origins[index],
                        body_origin: state.body_origins[index],
                        owners,
                        direct_switch: direct,
                        update_marker: context.secondary_switch,
                        clock_rate: context.clock_rate,
                    },
                )?;
                next.midi.inserts[index].lfo = a.next.instances[index].lfo;
                next.midi.master.control.assignments = a.next.assignments;
                next.staging = a.next.staging;
                InsertControlStep {
                    batch: a.batch,
                    program_writes: a.program_writes,
                    body_program: a.body_program,
                }
            }
            _ => return None,
        };
        Some((next, step))
    }
}
