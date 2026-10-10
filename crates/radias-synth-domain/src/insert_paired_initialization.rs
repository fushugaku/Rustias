//! Complete coefficient and conditional peer initialization of SYS07AF0C.
use crate::{
    effect_control::EffectOrigins,
    effect_parameters::EffectParameterBatch,
    insert_effect_construction::InsertPatch,
    insert_effect_control::{
        InsertControlContext, InsertControlState, InsertControlStep, InsertControlTables,
    },
    insert_effect_initialization::{
        InsertInitialization, InsertInitializationState, InsertInitializationTables,
    },
    insert_program_initialization::InsertProgramInitializationTables,
};

pub struct PreparedInsertPairedInitialization {
    pub next: InsertControlState,
    pub peer_initialized: bool,
    steps: [Option<InsertControlStep>; 26],
    count: usize,
}
impl PreparedInsertPairedInitialization {
    pub fn steps(&self) -> impl Iterator<Item = &InsertControlStep> {
        self.steps[..self.count].iter().flatten()
    }
    fn push(&mut self, step: InsertControlStep) -> Option<()> {
        *self.steps.get_mut(self.count)? = Some(step);
        self.count += 1;
        Some(())
    }
    fn batch(&mut self, batch: EffectParameterBatch) -> Option<()> {
        self.push(InsertControlStep {
            batch,
            program_writes: [None; 2],
            body_program: None,
        })
    }
    fn coefficients(
        &mut self,
        tables: &InsertInitializationTables,
        programs: &InsertProgramInitializationTables,
        slot: u8,
        context: InsertControlContext<'_>,
    ) -> Option<()> {
        let index = usize::from(slot);
        let instance = self.next.midi.inserts[index];
        let program = programs.prepare_default(
            &instance,
            EffectOrigins {
                program: self.next.body_origins[index],
                data: instance.buffer.origin,
                coefficients: self.next.relocation_origins[index],
            },
            self.next.staging.cursor,
        )?;
        self.next.midi.inserts[index] = program.next;
        self.push(InsertControlStep {
            batch: program.batch,
            program_writes: [None; 2],
            body_program: Some(program.program),
        })?;
        let prepared = tables.prepare_with_pair_allocation(
            &InsertInitializationState {
                instances: self.next.midi.inserts,
                coefficient_scratch: self.next.scratch,
            },
            InsertInitialization {
                slot,
                clock_rate: context.clock_rate,
                clock: context.clock,
            },
        )?;
        self.next.midi.inserts = prepared.next.instances;
        self.next.scratch = prepared.next.coefficient_scratch;
        self.batch(prepared.batch)
    }
}
impl InsertControlTables {
    pub fn prepare_paired_insert_initialization(
        &self,
        state: &InsertControlState,
        initialization: &InsertInitializationTables,
        programs: &InsertProgramInitializationTables,
        slot: u8,
        context: InsertControlContext<'_>,
    ) -> Option<PreparedInsertPairedInitialization> {
        let index = usize::from(slot);
        let instance = *state.midi.inserts.get(index)?;
        let mut out = PreparedInsertPairedInitialization {
            next: *state,
            peer_initialized: false,
            steps: [None; 26],
            count: 0,
        };
        let release = self.common.release_master_assignments(
            &mut out.next.midi.master.control,
            context.midi.direct_switch,
        )?;
        out.batch(release)?;
        let mut temporary = EffectParameterBatch::from_lfo(None);
        for (target, value) in [(0x2fb, 0x2f7), (0x2fa, 0), (0x2fd, 0x2f7), (0x2fc, 0)] {
            temporary.push_direct(target, value)?;
        }
        out.batch(temporary)?;
        let part = index / 2;
        let first_kind = context.program.bytes()[168 + part * 228] & 127;
        if slot & 1 == 0 && first_kind != 29 && first_kind != 30 && instance.extended_program != 0 {
            out.coefficients(initialization, programs, slot, context)?;
            out.next.staging.advance();
            let peer = slot ^ 1;
            out.coefficients(initialization, programs, peer, context)?;
            let start = 168 + part * 228 + 24;
            let b = &context.program.bytes()[start..start + 24];
            let mask = self.prepare_initial_mask(
                &out.next,
                peer,
                InsertPatch {
                    header: b[..4].try_into().ok()?,
                    parameters: b[4..].try_into().ok()?,
                },
                context,
            )?;
            out.next = mask.next;
            for &step in mask.steps() {
                out.push(step)?;
            }
            out.peer_initialized = true;
        } else {
            out.coefficients(initialization, programs, slot, context)?;
        }
        Some(out)
    }
}
