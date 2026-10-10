//! Whole idle-queue branch of SYS07C17E Master type changes.
//! The full rack context handles pressure via master_pressure_type_change.
use crate::{
    delay_time::DelayClock,
    effect_control::{EffectOrigins, PreparedEffect},
    effect_midi::{EffectMidiPolarity, EffectMidiSources},
    effect_parameters::EffectParameterBatch,
    effect_program_staging::EffectStagedProgramWrite,
    effect_routing::EffectRoutingInstance,
    effect_transition_queue::EffectTransitionQueueState,
    master_effect_construction::{MasterConstruction, MasterEffectInstance, MasterPatch},
    master_effect_control::{MasterControlTables, MasterEdit},
    master_effect_initialization::{MasterInitialization, MasterInitializationTables},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasterTypeStep {
    pub batch: EffectParameterBatch,
    pub program_writes: [Option<EffectStagedProgramWrite>; 2],
    pub body_program: Option<PreparedEffect>,
}
pub struct PreparedMasterTypeChange {
    pub next: MasterEffectInstance,
    pub patch: MasterPatch,
    pub steps: [Option<MasterTypeStep>; 24],
    pub step_count: usize,
    pub direct_switch: u32,
    pub pressure_marker: u32,
}
impl PreparedMasterTypeChange {
    pub fn steps(&self) -> impl Iterator<Item = &MasterTypeStep> {
        self.steps[..self.step_count].iter().flatten()
    }
    fn push(&mut self, step: MasterTypeStep) -> Option<()> {
        *self.steps.get_mut(self.step_count)? = Some(step);
        self.step_count += 1;
        Some(())
    }
    fn batch(&mut self, batch: EffectParameterBatch) -> Option<()> {
        self.push(MasterTypeStep {
            batch,
            program_writes: [None; 2],
            body_program: None,
        })
    }
}
#[derive(Clone, Copy)]
pub struct MasterTypeChange {
    pub kind: u8,
    pub origins: EffectOrigins,
    pub prefix_origin: u16,
    pub initial_direct_switch: u32,
    pub transition_marker: u32,
    pub pressure_marker: u32,
    pub queue: EffectTransitionQueueState,
    pub clock_rate: u32,
    pub clock: DelayClock,
    pub current_note: u8,
    pub midi: EffectMidiSources,
    pub polarity: EffectMidiPolarity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MasterTypeChangeError {
    InvalidInput,
    RackRebuildRequired,
}
impl MasterControlTables {
    pub fn prepare_master_type_change(
        &self,
        instance: &MasterEffectInstance,
        patch: MasterPatch,
        tables: &MasterInitializationTables,
        edit: MasterTypeChange,
    ) -> Result<PreparedMasterTypeChange, MasterTypeChangeError> {
        let active = usize::from(edit.queue.control & 1);
        let release_words = if instance.control.update_marker != 0 {
            instance
                .control
                .assignments
                .slots
                .iter()
                .filter(|s| s.target != crate::effect_updates::UNASSIGNED_TARGET)
                .count()
                * 5
        } else {
            0
        };
        if usize::from(edit.queue.rings[active].count) + release_words >= 1843 {
            return Err(MasterTypeChangeError::RackRebuildRequired);
        }
        self.prepare_idle_master_type_change(instance, patch, tables, edit)
            .ok_or(MasterTypeChangeError::InvalidInput)
    }
    fn prepare_idle_master_type_change(
        &self,
        instance: &MasterEffectInstance,
        patch: MasterPatch,
        tables: &MasterInitializationTables,
        edit: MasterTypeChange,
    ) -> Option<PreparedMasterTypeChange> {
        let slot = instance.control.work_slot;
        if slot >= 20 && slot != 28 {
            return None;
        }
        let mut plan = PreparedMasterTypeChange {
            next: *instance,
            patch,
            steps: [None; 24],
            step_count: 0,
            direct_switch: 0,
            pressure_marker: edit.pressure_marker,
        };
        if plan.next.control.update_marker != 0 {
            let batch = self
                .release_master_assignments(&mut plan.next.control, edit.initial_direct_switch)?;
            plan.batch(batch)?;
        }
        // The outer method distinguishes the busy branch before any acceptance.
        let active = usize::from(edit.queue.control & 1);
        let prior_words = plan.steps().map(|s| s.batch.words().len()).sum::<usize>();
        if usize::from(edit.queue.rings[active].count) + prior_words >= 1843 {
            return None;
        }
        let mut prefix = EffectParameterBatch::from_lfo(None);
        prefix.push_command(0, 0x01000014)?;
        let write = EffectStagedProgramWrite {
            selector: 3 * slot,
            word: self.early_reflect.transition_programs[4][0],
        };
        prefix.push_command(edit.prefix_origin, 0x02000000 | u32::from(write.selector))?;
        plan.push(MasterTypeStep {
            batch: prefix,
            program_writes: [Some(write), None],
            body_program: None,
        })?;
        let construction = self.construct_master(
            &plan.next,
            plan.patch,
            edit.kind,
            MasterConstruction::Defaults,
        )?;
        plan.next = construction.next;
        plan.patch = construction.patch;
        let initial = self.prepare_master_initialization(
            &plan.next.control,
            tables,
            MasterInitialization {
                kind: edit.kind,
                parameters: plan.next.parameters,
                origins: edit.origins,
                direct_switch: 1,
                clock_rate: edit.clock_rate,
            },
        )?;
        plan.next.control = initial.next;
        plan.push(MasterTypeStep {
            batch: initial.batch,
            program_writes: [None; 2],
            body_program: Some(initial.program),
        })?;
        let definition = self.definitions.get(usize::from(edit.kind))?;
        for parameter in 1..definition.parameter_count {
            if definition.initialization_mask & (0x80000000 >> parameter) == 0 {
                continue;
            }
            let prepared = self.prepare(
                &plan.next.control,
                type_parameter_edit(&plan.next, plan.patch, edit, parameter as u8, 1),
            )?;
            plan.next.control = prepared.next;
            plan.push(MasterTypeStep {
                batch: prepared.batch,
                program_writes: prepared.program_writes,
                body_program: prepared.body_program,
            })?;
        }
        plan.batch(self.flanger.routing.prepare_master_input_mute(
            plan.patch.header[0] & 127,
            &EffectRoutingInstance {
                kind: plan.next.kind,
                origin: edit.origins.data,
                parameters: plan.next.parameters,
            },
        )?)?;
        if plan.next.control.owner != 0 && plan.next.kind != 0 {
            let parameter = u8::try_from(plan.next.control.owner).ok()?;
            let prepared = self.prepare(
                &plan.next.control,
                type_parameter_edit(&plan.next, plan.patch, edit, parameter, 0),
            )?;
            plan.next.control = prepared.next;
            plan.push(MasterTypeStep {
                batch: prepared.batch,
                program_writes: prepared.program_writes,
                body_program: prepared.body_program,
            })?;
        }
        let words = plan.steps().map(|s| s.batch.words().len()).sum::<usize>();
        if usize::from(edit.queue.rings[active].count) + words >= 1843 {
            plan.pressure_marker = 1;
        }
        Some(plan)
    }
}
fn type_parameter_edit(
    instance: &MasterEffectInstance,
    patch: MasterPatch,
    edit: MasterTypeChange,
    parameter: u8,
    direct_switch: u32,
) -> MasterEdit {
    MasterEdit {
        kind: instance.kind,
        parameter,
        value: patch.parameters[usize::from(parameter)],
        parameters: instance.parameters,
        previous_parameters: instance.previous_parameters,
        stored_owner: patch.header[1] & 31,
        stored_effect_type: patch.header[0] & 127,
        stored_enabled: patch.header[0] & 128 != 0,
        update_marker: instance.control.update_marker,
        origin: edit.origins.data,
        owner: instance.control.owner,
        direct_switch,
        clock_rate: edit.clock_rate,
        clock: edit.clock,
        current_note: edit.current_note,
        midi: edit.midi,
        polarity: edit.polarity,
        prefix_origin: edit.prefix_origin,
        body_origin: edit.origins.program,
        relocation_origin: edit.origins.coefficients,
        transition_marker: edit.transition_marker,
    }
}
