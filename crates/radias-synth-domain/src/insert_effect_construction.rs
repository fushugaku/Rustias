//! Whole SYS07B318 stored insert construction used by FX rack rebuilding.
use crate::{
    effect_buffer_allocation::EffectBufferInstance, effect_curves::EffectParameterRange,
    effect_lfo_program::EffectLfoProgram, effect_modulation::GrainModulationHistory,
};
pub struct InsertConstructionDefinition {
    pub parameter_count: usize,
    pub ranges: [EffectParameterRange; 20],
}
pub struct InsertConstructionTables {
    pub definitions: [InsertConstructionDefinition; 31],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertEffectInstance {
    pub slot: u8,
    pub buffer: EffectBufferInstance,
    pub previous_parameters: [u8; 20],
    pub owners: [u32; 2],
    pub lfo: EffectLfoProgram,
    pub controller_source: u32,
    pub controller_values: [i8; 2],
    pub controller_offset: u8,
    pub extended_program: u32,
    pub enabled_argument: u32,
    pub rotary_mode: u32,
    pub rotary_speed: u32,
    pub grain_history: GrainModulationHistory,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertPatch {
    pub header: [u8; 4],
    pub parameters: [u8; 20],
}
impl InsertConstructionTables {
    pub fn construct_stored(
        &self,
        prior: &InsertEffectInstance,
        patch: InsertPatch,
        kind: u8,
    ) -> Option<InsertEffectInstance> {
        if prior.slot >= 8 {
            return None;
        }
        let definition = self.definitions.get(usize::from(kind))?;
        let count = definition.parameter_count;
        if count > 20 {
            return None;
        }
        let mut next = *prior;
        next.buffer.kind = kind;
        next.owners = [
            u32::from(patch.header[2] & 31),
            u32::from(patch.header[3] & 31),
        ];
        next.buffer.ratio = 0;
        next.buffer.limited = 0;
        next.controller_offset = 64;
        next.enabled_argument = u32::from(patch.header[0] & 128 != 0);
        next.buffer.parameters[..count].copy_from_slice(&patch.parameters[..count]);
        next.previous_parameters[..count].copy_from_slice(&patch.parameters[..count]);
        for (value, range) in next.buffer.parameters[..count]
            .iter_mut()
            .zip(definition.ranges)
        {
            *value = range.clamp_encoded(*value);
        }
        if kind == 29 {
            next.rotary_mode = u32::from(next.buffer.parameters[1]);
            next.rotary_speed = u32::from(next.buffer.parameters[5]);
        } else if kind == 27 {
            next.grain_history = GrainModulationHistory::default();
        } else {
            next.rotary_mode = 0;
            next.rotary_speed = 0;
        }
        Some(next)
    }
}
