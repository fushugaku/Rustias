//! Whole SYS07BDA6 initial Master parameter mask with mixed MIDI callbacks.
use crate::{
    effect_parameters::EffectParameterBatch,
    insert_effect_control::{InsertControlContext, InsertControlState, InsertControlStep},
    master_effect_construction::MasterPatch,
    master_effect_control::MasterEdit,
    mixed_effect_midi::MixedEffectMidiFrame,
};
#[derive(Clone, Copy)]
pub struct MasterInitialMaskContext<'a> {
    pub common: InsertControlContext<'a>,
    pub prefix_origin: u16,
    pub body_origin: u16,
    pub relocation_origin: u16,
}
pub struct PreparedMasterInitialMask {
    pub next: InsertControlState,
    pub steps: [Option<InsertControlStep>; 20],
    pub step_count: usize,
}
impl PreparedMasterInitialMask {
    pub fn steps(&self) -> impl Iterator<Item = &InsertControlStep> {
        self.steps[..self.step_count].iter().flatten()
    }
}
impl crate::insert_effect_control::InsertControlTables {
    pub fn prepare_master_parameter(
        &self,
        state: &InsertControlState,
        parameter: u8,
        value: u8,
        context: MasterInitialMaskContext<'_>,
    ) -> Option<(InsertControlState, InsertControlStep)> {
        self.prepare_master_parameter_inner(state, parameter, value, context, true)
    }
    pub fn prepare_master_stored_parameter(
        &self,
        state: &InsertControlState,
        parameter: u8,
        value: u8,
        context: MasterInitialMaskContext<'_>,
    ) -> Option<(InsertControlState, InsertControlStep)> {
        self.prepare_master_parameter_inner(state, parameter, value, context, false)
    }
    fn prepare_master_parameter_inner(
        &self,
        state: &InsertControlState,
        parameter: u8,
        value: u8,
        context: MasterInitialMaskContext<'_>,
        validate_argument: bool,
    ) -> Option<(InsertControlState, InsertControlStep)> {
        let definition = self
            .common
            .definitions
            .get(usize::from(state.midi.master.kind))?;
        if usize::from(parameter) >= definition.parameter_count {
            return None;
        }
        let range = definition.ranges[usize::from(parameter)];
        let decoded = i32::from(value) - i32::from(range.encoded_zero);
        if validate_argument
            && !(i32::from(range.minimum)..=i32::from(range.maximum)).contains(&decoded)
        {
            return None;
        }
        let mut next = *state;
        let parameter = usize::from(parameter);
        let master = next.midi.master;
        let p = master.parameters;
        next.midi.master.control.coefficient_scratch = next.scratch;
        next.midi.master.control.work_slot = next.staging.cursor;
        let mut step = InsertControlStep {
            batch: EffectParameterBatch::from_lfo(None),
            program_writes: [None; 2],
            body_program: None,
        };
        if matches!(
            (master.kind, parameter),
            (4, 15) | (5, 16) | (29, 2 | 3 | 6 | 7 | 9) | (30, 19)
        ) {
            let tested = if matches!(master.kind, 29 | 30) {
                value.min(12)
            } else {
                value
            };
            if master.control.midi_binding.source != u32::from(tested) {
                let source = p[if master.kind == 29 { 2 } else { parameter }];
                let control = &mut next.midi.master.control;
                control.midi_binding.source = u32::from(source);
                control.midi_binding.values = [context.common.midi.midi.value(4, source)?.abs(), 0];
                if master.kind == 29 {
                    let source = if p[4] == 0 { p[6] } else { p[9] };
                    control.midi_binding.values[1] =
                        context.common.midi.midi.value(4, source)?.abs();
                }
                let prepared = self.common.prepare_mixed_midi(
                    &next.midi,
                    MixedEffectMidiFrame {
                        force_refresh: false,
                        ..context.common.midi
                    },
                )?;
                next.midi = prepared.next;
                step.batch = prepared.batch;
            }
            if master.kind == 5 {
                step.batch.push_direct(
                    u32::from(context.common.midi.master_origin) + 55,
                    crate::wah_effect::WahEffectTables::controller_polarity_word(
                        &p,
                        context.common.midi.polarity,
                    ),
                )?;
            }
        } else {
            let stored = context.common.program.master_effect();
            let edit = MasterEdit {
                kind: master.kind,
                parameter: parameter as u8,
                value,
                parameters: p,
                previous_parameters: master.previous_parameters,
                stored_owner: context.common.program.bytes()[1039] & 31,
                stored_effect_type: stored.kind()?.raw(),
                stored_enabled: stored.enabled(),
                update_marker: master.control.update_marker,
                origin: context.common.midi.master_origin,
                owner: master.control.owner,
                direct_switch: context.common.midi.direct_switch,
                clock_rate: context.common.clock_rate,
                clock: context.common.clock,
                current_note: context.common.midi.current_notes[4],
                midi: context.common.midi.midi,
                polarity: context.common.midi.polarity,
                prefix_origin: context.prefix_origin,
                body_origin: context.body_origin,
                relocation_origin: context.relocation_origin,
                transition_marker: context.common.secondary_switch,
            };
            let prepared = if validate_argument {
                self.common.prepare(&next.midi.master.control, edit)?
            } else {
                self.common
                    .prepare_stored_argument(&next.midi.master.control, edit)?
            };
            next.midi.master.control = prepared.next;
            step = InsertControlStep {
                batch: prepared.batch,
                program_writes: prepared.program_writes,
                body_program: prepared.body_program,
            };
        }
        next.scratch = next.midi.master.control.coefficient_scratch;
        next.staging.cursor = next.midi.master.control.work_slot;
        for w in step.program_writes.iter().flatten() {
            let slot = usize::from(w.selector / 3);
            if w.selector % 3 == 0 {
                next.staging.prefix[slot] = w.word;
                next.staging.counts[slot][0] = 1;
            } else if w.selector % 3 == 2 {
                next.staging.tail[slot] = w.word;
                next.staging.counts[slot][1] = 1;
            }
        }
        Some((next, step))
    }
    pub fn prepare_master_initial_mask(
        &self,
        state: &InsertControlState,
        patch: MasterPatch,
        context: MasterInitialMaskContext<'_>,
    ) -> Option<PreparedMasterInitialMask> {
        let definition = self
            .common
            .definitions
            .get(usize::from(state.midi.master.kind))?;
        let mut plan = PreparedMasterInitialMask {
            next: *state,
            steps: [None; 20],
            step_count: 0,
        };
        for parameter in 1..definition.parameter_count {
            if definition.initialization_mask & (0x80000000 >> parameter) == 0 {
                continue;
            }
            let (next, step) = self.prepare_master_stored_parameter(
                &plan.next,
                parameter as u8,
                patch.parameters[parameter],
                context,
            )?;
            plan.next = next;
            *plan.steps.get_mut(plan.step_count)? = Some(step);
            plan.step_count += 1;
        }
        Some(plan)
    }
}
