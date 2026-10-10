//! Whole idle high Master type setter, retaining mixed rack callbacks.
use crate::{
    effect_control::EffectOrigins,
    effect_parameters::EffectParameterBatch,
    effect_program_staging::EffectStagedProgramWrite,
    effect_property::{EffectProperty, EffectPropertyChange, EffectPropertyTables},
    effect_routing::EffectRoutingInstance,
    effect_transition_queue::EffectTransitionQueueState,
    insert_effect_control::{
        InsertControlContext, InsertControlState, InsertControlStep, InsertControlTables,
    },
    master_effect_construction::{MasterConstruction, MasterPatch},
    master_effect_initialization::{MasterInitialization, MasterInitializationTables},
    master_initial_mask::MasterInitialMaskContext,
    mixed_effect_midi::MixedEffectMidiFrame,
    mixed_effect_parameter::EffectParameterTarget,
    program::Program,
};
#[derive(Clone, Copy)]
pub struct MasterTypeValueContext<'a> {
    pub parameters: MasterInitialMaskContext<'a>,
    pub queue: EffectTransitionQueueState,
    pub pressure_marker: u32,
}
pub struct PreparedMasterTypeValueChange {
    pub next: InsertControlState,
    pub program: Program,
    pub value: i32,
    pub steps: [Option<InsertControlStep>; 24],
    pub step_count: usize,
    pub direct_switch: u32,
    pub pressure_marker: u32,
}
impl PreparedMasterTypeValueChange {
    pub fn steps(&self) -> impl Iterator<Item = &InsertControlStep> {
        self.steps[..self.step_count].iter().flatten()
    }
    fn push(&mut self, step: InsertControlStep) -> Option<()> {
        *self.steps.get_mut(self.step_count)? = Some(step);
        self.step_count += 1;
        Some(())
    }
    fn batch(&mut self, batch: EffectParameterBatch) -> Option<()> {
        self.push(InsertControlStep {
            batch,
            program_writes: [None; 2],
            body_program: None,
        })
    }
}
impl InsertControlTables {
    pub fn prepare_master_type_value_change(
        &self,
        state: &InsertControlState,
        properties: &EffectPropertyTables,
        initialization: &MasterInitializationTables,
        value: i32,
        context: MasterTypeValueContext<'_>,
    ) -> Option<PreparedMasterTypeValueChange> {
        let original = context.parameters;
        let stored = properties.prepare(
            state,
            original.common.program,
            EffectPropertyChange {
                target: EffectParameterTarget::Master,
                property: EffectProperty::Kind,
                value,
            },
        )?;
        let mut out = PreparedMasterTypeValueChange {
            next: stored.next,
            program: stored.program,
            value: stored.value,
            steps: [None; 24],
            step_count: 0,
            direct_switch: 0,
            pressure_marker: context.pressure_marker,
        };
        out.next.midi.master.control.coefficient_scratch = out.next.scratch;
        out.next.midi.master.control.work_slot = out.next.staging.cursor;
        if out.next.midi.master.control.update_marker != 0 {
            let batch = self.common.release_master_assignments(
                &mut out.next.midi.master.control,
                original.common.midi.direct_switch,
            )?;
            out.batch(batch)?;
        }
        let active = usize::from(context.queue.control & 1);
        if usize::from(context.queue.rings[active].count)
            + out.steps().map(|s| s.batch.words().len()).sum::<usize>()
            >= 1843
        {
            return None;
        }
        let mut prefix = EffectParameterBatch::from_lfo(None);
        prefix.push_command(0, 0x01000014)?;
        let write = EffectStagedProgramWrite {
            selector: 3 * out.next.staging.cursor,
            word: self.common.early_reflect.transition_programs[4][0],
        };
        prefix.push_command(
            original.prefix_origin,
            0x02000000 | u32::from(write.selector),
        )?;
        let slot = usize::from(out.next.staging.cursor);
        *out.next.staging.prefix.get_mut(slot)? = write.word;
        out.next.staging.counts[slot][0] = 1;
        out.push(InsertControlStep {
            batch: prefix,
            program_writes: [Some(write), None],
            body_program: None,
        })?;
        let b = out.program.bytes();
        let patch = MasterPatch {
            header: b[1038..1040].try_into().ok()?,
            parameters: b[1040..1060].try_into().ok()?,
        };
        let built = self.common.construct_master(
            &out.next.midi.master,
            patch,
            stored.value as u8,
            MasterConstruction::Defaults,
        )?;
        out.next.midi.master = built.next;
        let mut raw = *out.program.bytes();
        raw[1038..1040].copy_from_slice(&built.patch.header);
        raw[1040..1060].copy_from_slice(&built.patch.parameters);
        out.program = Program::from_bytes(&raw).ok()?;
        let initialized = self.common.prepare_master_initialization(
            &out.next.midi.master.control,
            initialization,
            MasterInitialization {
                kind: stored.value as u8,
                parameters: out.next.midi.master.parameters,
                origins: EffectOrigins {
                    program: original.body_origin,
                    data: original.common.midi.master_origin,
                    coefficients: original.relocation_origin,
                },
                direct_switch: 1,
                clock_rate: original.common.clock_rate,
            },
        )?;
        out.next.midi.master.control = initialized.next;
        out.next.scratch = out.next.midi.master.control.coefficient_scratch;
        out.next.staging.cursor = out.next.midi.master.control.work_slot;
        out.push(InsertControlStep {
            batch: initialized.batch,
            program_writes: [None; 2],
            body_program: Some(initialized.program),
        })?;
        let program = out.program.clone();
        let direct = MasterInitialMaskContext {
            common: InsertControlContext {
                program: &program,
                midi: MixedEffectMidiFrame {
                    direct_switch: 1,
                    ..original.common.midi
                },
                ..original.common
            },
            ..original
        };
        let mask = self.prepare_master_initial_mask(&out.next, built.patch, direct)?;
        out.next = mask.next;
        for &step in mask.steps() {
            out.push(step)?;
        }
        out.batch(self.common.flanger.routing.prepare_master_input_mute(
            raw[1038] & 127,
            &EffectRoutingInstance {
                kind: out.next.midi.master.kind,
                origin: original.common.midi.master_origin,
                parameters: out.next.midi.master.parameters,
            },
        )?)?;
        if out.next.midi.master.control.owner != 0 && out.next.midi.master.kind != 0 {
            let p = u8::try_from(out.next.midi.master.control.owner).ok()?;
            let (next, step) = self.prepare_master_stored_parameter(
                &out.next,
                p,
                out.program.bytes()[1040 + usize::from(p)],
                MasterInitialMaskContext {
                    common: InsertControlContext {
                        program: &program,
                        midi: MixedEffectMidiFrame {
                            direct_switch: 0,
                            ..original.common.midi
                        },
                        ..original.common
                    },
                    ..original
                },
            )?;
            out.next = next;
            out.push(step)?;
        }
        if usize::from(context.queue.rings[active].count)
            + out.steps().map(|s| s.batch.words().len()).sum::<usize>()
            >= 1843
        {
            out.pressure_marker = 1;
        }
        Some(out)
    }
}
