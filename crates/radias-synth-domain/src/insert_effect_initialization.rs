//! Whole SYS07AD48 initial Insert coefficient/LFO publication.
use crate::{
    delay_time::DelayClock,
    effect_buffer_allocation::EffectBufferAllocationTables,
    effect_buffers::effect_uses_buffer,
    effect_lfo_program::{EffectLfoMapping, EffectLfoSlot},
    effect_parameters::EffectParameterBatch,
    insert_effect_construction::InsertEffectInstance,
    lfo_tempo::LfoTempoTables,
};

#[derive(Clone, Copy, Debug)]
pub struct InsertInitialCoefficients {
    pub words: [u32; 73],
    pub count: u8,
}
pub struct InsertInitializationTables {
    pub coefficients: [InsertInitialCoefficients; 31],
    pub reverb: [InsertInitialCoefficients; 3],
    pub talking: [InsertInitialCoefficients; 3],
    pub lfo_mappings: [EffectLfoMapping; 31],
    pub tempo: LfoTempoTables,
    pub allocation: EffectBufferAllocationTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertInitializationState {
    pub instances: [InsertEffectInstance; 8],
    pub coefficient_scratch: [u32; 73],
}
#[derive(Clone, Copy, Debug)]
pub struct InsertInitialization {
    pub slot: u8,
    pub clock_rate: u32,
    pub clock: DelayClock,
}
pub struct PreparedInsertInitialization {
    pub next: InsertInitializationState,
    pub batch: EffectParameterBatch,
}
impl InsertInitializationTables {
    pub fn prepare(
        &self,
        state: &InsertInitializationState,
        edit: InsertInitialization,
    ) -> Option<PreparedInsertInitialization> {
        self.prepare_inner(state, edit, false)
    }
    pub fn prepare_with_pair_allocation(
        &self,
        state: &InsertInitializationState,
        edit: InsertInitialization,
    ) -> Option<PreparedInsertInitialization> {
        self.prepare_inner(state, edit, true)
    }
    fn prepare_inner(
        &self,
        state: &InsertInitializationState,
        edit: InsertInitialization,
        allocate_pair: bool,
    ) -> Option<PreparedInsertInitialization> {
        let index = usize::from(edit.slot);
        let instance = state.instances.get(index)?;
        if instance.slot != edit.slot {
            return None;
        }
        let kind = instance.buffer.kind;
        let p = instance.buffer.parameters;
        let coefficients = match kind {
            11 => self.reverb.get(usize::from(p[1]))?,
            30 => self.talking.get(usize::from(p[7]))?,
            _ => self.coefficients.get(usize::from(kind))?,
        };
        let mut next = *state;
        let count = usize::from(coefficients.count);
        next.coefficient_scratch
            .get_mut(..count)?
            .copy_from_slice(coefficients.words.get(..count)?);
        next.coefficient_scratch[0] = 0x7fffff;
        next.coefficient_scratch[1] = 0;
        let lfo = instance.lfo.prepare(
            &p,
            *self.lfo_mappings.get(usize::from(kind))?,
            EffectLfoSlot::new(edit.slot)?,
            0,
            edit.clock_rate,
            &self.tempo,
        )?;
        if let Some(publication) = lfo {
            next.instances[index].lfo = publication.program;
        }
        let mut batch = EffectParameterBatch::from_lfo(lfo);
        if allocate_pair || effect_uses_buffer(u32::from(kind)) {
            let allocation = self.allocation.prepare(
                &next.instances.map(|i| i.buffer),
                edit.slot,
                &next.coefficient_scratch,
                edit.clock,
            )?;
            for (instance, buffer) in next.instances.iter_mut().zip(allocation.instances) {
                instance.buffer = buffer;
            }
            next.coefficient_scratch
                .copy_from_slice(&allocation.template.words[..73]);
            batch.extend(&allocation.batch)?;
        }
        let origin = instance.buffer.origin;
        // SYS07AC32 writes these pointers even though Distortion uploads 43 words.
        if kind == 7 {
            next.coefficient_scratch[46] = u32::from(origin) + 6;
            next.coefficient_scratch[47] = u32::from(origin) + 11;
        }
        for (i, &value) in next.coefficient_scratch[..count].iter().enumerate() {
            batch.push_direct(u32::from(origin.wrapping_add(i as u16)), value)?;
        }
        Some(PreparedInsertInitialization { next, batch })
    }
}
