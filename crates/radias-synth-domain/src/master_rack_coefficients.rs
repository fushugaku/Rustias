//! Whole SYS07BCFC coefficient-only Master stage of rack loading.
use crate::{
    effect_buffers::effect_uses_buffer,
    effect_lfo_program::EffectLfoSlot,
    effect_parameters::EffectParameterBatch,
    master_effect_buffers::{MasterBufferState, relocate_master_template},
    master_effect_control::{MasterControlState, MasterControlTables},
    master_effect_initialization::MasterInitializationTables,
};
#[derive(Clone, Copy)]
pub struct MasterRackCoefficientLoad {
    pub kind: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub clock_rate: u32,
}
pub struct PreparedMasterRackCoefficients {
    pub next: MasterControlState,
    pub batch: EffectParameterBatch,
}
impl MasterControlTables {
    pub fn prepare_master_rack_coefficients(
        &self,
        state: &MasterControlState,
        tables: &MasterInitializationTables,
        edit: MasterRackCoefficientLoad,
    ) -> Option<PreparedMasterRackCoefficients> {
        let definition = self.definitions.get(usize::from(edit.kind))?;
        let coefficients = match edit.kind {
            11 => tables.reverb.get(usize::from(edit.parameters[1]))?,
            30 => tables.talking.get(usize::from(edit.parameters[7]))?,
            _ => tables.coefficients.get(usize::from(edit.kind))?,
        };
        let count = usize::from(coefficients.count);
        let mut next = *state;
        next.coefficient_scratch
            .get_mut(..count)?
            .copy_from_slice(coefficients.words.get(..count)?);
        next.coefficient_scratch[0] = 0x7fffff;
        next.coefficient_scratch[1] = 0;
        // The rack-loader entry relocates before publishing LFO configuration.
        if effect_uses_buffer(u32::from(edit.kind)) {
            let relocated = relocate_master_template(
                edit.kind,
                MasterBufferState {
                    capacity: next.delay.capacity,
                },
                &next.coefficient_scratch[..count],
            )?;
            next.delay.capacity = relocated.next.capacity;
            next.coefficient_scratch[..count].copy_from_slice(&relocated.words[..count]);
        }
        let lfo = next.lfo.prepare(
            &edit.parameters,
            definition.lfo_mapping,
            EffectLfoSlot::new(8)?,
            1,
            edit.clock_rate,
            &self.tempo,
        )?;
        if let Some(p) = lfo {
            next.lfo = p.program;
        }
        let mut batch = EffectParameterBatch::from_lfo(lfo);
        if edit.kind == 7 {
            next.coefficient_scratch[46] = u32::from(edit.origin) + 6;
            next.coefficient_scratch[47] = u32::from(edit.origin) + 11;
        }
        for (i, &word) in next.coefficient_scratch[..count].iter().enumerate() {
            batch.push_direct(u32::from(edit.origin.wrapping_add(i as u16)), word)?;
        }
        Some(PreparedMasterRackCoefficients { next, batch })
    }
}
