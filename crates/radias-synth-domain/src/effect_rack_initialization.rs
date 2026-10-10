//! Whole SYS07E308 rack load, preserving program-buffer and publication order.
use crate::{
    effect_control::{
        EffectBank, EffectKind, EffectLoad, EffectOrigins, EffectRequest, PreparedEffect,
    },
    effect_parameters::EffectParameterBatch,
    effect_routing::{EffectRoutingContext, EffectRoutingInstance},
    insert_effect_construction::InsertPatch,
    insert_effect_control::{
        InsertControlContext, InsertControlState, InsertControlStep, InsertControlTables,
    },
    insert_effect_initialization::{
        InsertInitialization, InsertInitializationState, InsertInitializationTables,
    },
    insert_program_initialization::InsertProgramInitializationTables,
    master_effect_construction::MasterPatch,
    master_effect_initialization::{MasterInitialProgram, MasterInitializationTables},
    master_initial_mask::MasterInitialMaskContext,
    master_rack_coefficients::MasterRackCoefficientLoad,
};

pub struct EffectRackInitializationTables {
    pub control: InsertControlTables,
    pub insert_programs: InsertProgramInitializationTables,
    pub insert_coefficients: InsertInitializationTables,
    pub master_coefficients: MasterInitializationTables,
    pub master_talking_programs: [MasterInitialProgram; 2],
    pub master_reverb_programs: [MasterInitialProgram; 2],
    pub occupies_pair: [bool; 31],
}

#[derive(Clone, Copy)]
pub struct EffectRackInitializationContext<'a> {
    pub common: InsertControlContext<'a>,
    pub master_prefix: u16,
    pub master_body: u16,
    pub master_relocation: u16,
}

pub struct PreparedEffectRackInitialization {
    pub next: InsertControlState,
    // Nine effects * (program + coefficients + at most 19 parameters), five
    // input mutes, and the final wait. No heap or CPU interpreter in the domain.
    steps: [Option<InsertControlStep>; 195],
    step_count: usize,
}
impl PreparedEffectRackInitialization {
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
impl EffectRackInitializationTables {
    pub fn prepare(
        &self,
        state: &InsertControlState,
        context: EffectRackInitializationContext<'_>,
    ) -> Option<PreparedEffectRackInitialization> {
        let mut plan = PreparedEffectRackInitialization {
            next: *state,
            steps: [None; 195],
            step_count: 0,
        };
        for part in 0..4 {
            let first = plan.next.midi.inserts[part * 2].buffer.kind;
            let roles = if *self.occupies_pair.get(usize::from(first))? {
                1
            } else {
                2
            };
            for role in 0..roles {
                let slot = part * 2 + role;

                let instance = plan.next.midi.inserts[slot];
                let program = self.insert_programs.prepare(
                    &instance,
                    EffectOrigins {
                        program: plan.next.body_origins[slot],
                        data: instance.buffer.origin,
                        coefficients: plan.next.relocation_origins[slot],
                    },
                )?;
                plan.next.midi.inserts[slot] = program.next;
                plan.push(InsertControlStep {
                    batch: program.batch,
                    program_writes: [None; 2],
                    body_program: Some(program.program),
                })?;

                let coefficients = self.insert_coefficients.prepare(
                    &InsertInitializationState {
                        instances: plan.next.midi.inserts,
                        coefficient_scratch: plan.next.scratch,
                    },
                    InsertInitialization {
                        slot: slot as u8,
                        clock_rate: context.common.clock_rate,
                        clock: context.common.clock,
                    },
                )?;
                plan.next.midi.inserts = coefficients.next.instances;
                plan.next.scratch = coefficients.next.coefficient_scratch;
                plan.batch(coefficients.batch)?;
                let start = 168 + part * 228 + role * 24;
                let raw = &context.common.program.bytes()[start..start + 24];

                let mask = self.control.prepare_initial_mask(
                    &plan.next,
                    slot as u8,
                    InsertPatch {
                        header: raw[..4].try_into().ok()?,
                        parameters: raw[4..].try_into().ok()?,
                    },
                    context.common,
                )?;
                plan.next = mask.next;
                for &step in mask.steps() {
                    plan.push(step)?;
                }
            }
            plan.batch(self.input_mute(
                &plan.next,
                context.common,
                EffectRoutingContext::Insert(part as u8),
            )?)?;
        }

        let master = plan.next.midi.master;
        let template = match master.kind {
            30 => &self.master_talking_programs[usize::from(master.parameters[7] != 0)],
            11 => &self.master_reverb_programs[usize::from(matches!(master.parameters[1], 4 | 5))],
            _ => self
                .master_coefficients
                .programs
                .get(usize::from(master.kind))?,
        };
        let program = PreparedEffect::compile(
            EffectRequest {
                kind: EffectKind::new(master.kind)?,
                bank: EffectBank::Master,
                load: EffectLoad::ParameterSelected,
                work_slot: 28,
                selector_byte: 0,
                origins: EffectOrigins {
                    program: context.master_body,
                    data: context.common.midi.master_origin,
                    coefficients: context.master_relocation,
                },
            },
            template
                .bytes
                .get(..usize::from(template.source_words) * 6)?,
            self.master_coefficients.program_layout,
        )
        .ok()?;
        let mut batch = EffectParameterBatch::from_lfo(None);
        batch.push_command(program.blocks[0].destination, program.blocks[0].tag)?;
        plan.push(InsertControlStep {
            batch,
            program_writes: [None; 2],
            body_program: Some(program),
        })?;
        plan.next.midi.master.control.coefficient_scratch = plan.next.scratch;

        let coefficients = self.control.common.prepare_master_rack_coefficients(
            &plan.next.midi.master.control,
            &self.master_coefficients,
            MasterRackCoefficientLoad {
                kind: master.kind,
                parameters: master.parameters,
                origin: context.common.midi.master_origin,
                clock_rate: context.common.clock_rate,
            },
        )?;
        plan.next.midi.master.control = coefficients.next;
        plan.next.scratch = coefficients.next.coefficient_scratch;
        plan.batch(coefficients.batch)?;
        let raw = &context.common.program.bytes()[1038..1060];

        let mask = self.control.prepare_master_initial_mask(
            &plan.next,
            MasterPatch {
                header: raw[..2].try_into().ok()?,
                parameters: raw[2..].try_into().ok()?,
            },
            MasterInitialMaskContext {
                common: context.common,
                prefix_origin: context.master_prefix,
                body_origin: context.master_body,
                relocation_origin: context.master_relocation,
            },
        )?;
        plan.next = mask.next;
        for &step in mask.steps() {
            plan.push(step)?;
        }
        plan.batch(self.input_mute(&plan.next, context.common, EffectRoutingContext::Master)?)?;
        let mut wait = EffectParameterBatch::from_lfo(None);
        wait.push_command(0, 0x01000046)?;
        plan.batch(wait)?;
        Some(plan)
    }
    fn input_mute(
        &self,
        state: &InsertControlState,
        context: InsertControlContext<'_>,
        routing: EffectRoutingContext,
    ) -> Option<EffectParameterBatch> {
        let mut instances = [EffectRoutingInstance::default(); 9];
        for (out, instance) in instances[..8].iter_mut().zip(state.midi.inserts) {
            *out = EffectRoutingInstance {
                kind: instance.buffer.kind,
                origin: instance.buffer.origin,
                parameters: instance.buffer.parameters,
            };
        }
        instances[8] = EffectRoutingInstance {
            kind: state.midi.master.kind,
            origin: context.midi.master_origin,
            parameters: state.midi.master.parameters,
        };
        self.control
            .common
            .flanger
            .routing
            .prepare_input_mute(context.program, &instances, routing)
    }
}
