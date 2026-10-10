//! Whole SYS074B20 selected Insert program preparation and queued upload.
use crate::{
    effect_control::{
        EffectBank, EffectBufferLayout, EffectKind, EffectLoad, EffectOrigins, EffectRequest,
        PreparedEffect,
    },
    effect_parameters::EffectParameterBatch,
    insert_effect_construction::InsertEffectInstance,
};
pub struct InsertInitialProgram {
    pub bytes: [u8; 720],
    pub source_words: u8,
}
pub struct InsertProgramInitializationTables {
    pub programs: [InsertInitialProgram; 31],
    pub talking: [InsertInitialProgram; 2],
    pub layout: EffectBufferLayout,
}
pub struct PreparedInsertProgramInitialization {
    pub next: InsertEffectInstance,
    pub program: PreparedEffect,
    pub batch: EffectParameterBatch,
}
impl InsertProgramInitializationTables {
    pub fn prepare_default(
        &self,
        instance: &InsertEffectInstance,
        origins: EffectOrigins,
        work_slot: u8,
    ) -> Option<PreparedInsertProgramInitialization> {
        let kind = EffectKind::new(instance.buffer.kind)?;
        let template = self.programs.get(usize::from(kind.raw()))?;
        let program = PreparedEffect::compile(
            EffectRequest {
                kind,
                bank: EffectBank::Insert,
                load: EffectLoad::Default,
                work_slot: u16::from(work_slot),
                selector_byte: 0,
                origins,
            },
            template
                .bytes
                .get(..usize::from(template.source_words) * 6)?,
            self.layout,
        )
        .ok()?;
        let mut next = *instance;
        next.extended_program = u32::from(program.extended_insert);
        let mut batch = EffectParameterBatch::from_lfo(None);
        for block in &program.blocks[..usize::from(program.block_count)] {
            batch.push_command(block.destination, block.tag)?;
        }
        Some(PreparedInsertProgramInitialization {
            next,
            program,
            batch,
        })
    }
    pub fn prepare(
        &self,
        instance: &InsertEffectInstance,
        origins: EffectOrigins,
    ) -> Option<PreparedInsertProgramInitialization> {
        let kind = EffectKind::new(instance.buffer.kind)?;
        let template = if kind.raw() == 30 {
            &self.talking[usize::from(instance.buffer.parameters[7] != 0)]
        } else {
            self.programs.get(usize::from(kind.raw()))?
        };
        let program = PreparedEffect::compile(
            EffectRequest {
                kind,
                bank: EffectBank::Insert,
                load: EffectLoad::ParameterSelected,
                work_slot: u16::from(instance.slot),
                selector_byte: instance.buffer.parameters[7],
                origins,
            },
            template
                .bytes
                .get(..usize::from(template.source_words) * 6)?,
            self.layout,
        )
        .ok()?;
        let mut next = *instance;
        next.extended_program = u32::from(program.extended_insert);
        let mut batch = EffectParameterBatch::from_lfo(None);
        for block in &program.blocks[..usize::from(program.block_count)] {
            batch.push_command(block.destination, block.tag)?;
        }
        Some(PreparedInsertProgramInitialization {
            next,
            program,
            batch,
        })
    }
}
