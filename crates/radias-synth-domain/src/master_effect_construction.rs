//! Complete Master object constructors SYS07BE4C / 07BF22 / 07BFEE.
use crate::{
    effect_modulation::GrainModulationHistory,
    master_effect_control::{MasterControlState, MasterControlTables},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasterEffectInstance {
    pub kind: u8,
    pub parameters: [u8; 20],
    pub previous_parameters: [u8; 20],
    pub control: MasterControlState,
    pub controller_offset: u8,
    pub enabled_argument: u32,
    pub grain_history: GrainModulationHistory,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasterPatch {
    pub header: [u8; 2],
    pub parameters: [u8; 20],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MasterConstruction {
    Defaults,
    StoredWithDefaultHistory,
    StoredWithRawHistory,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedMasterConstruction {
    pub next: MasterEffectInstance,
    pub patch: MasterPatch,
}
impl MasterControlTables {
    pub fn construct_master(
        &self,
        prior: &MasterEffectInstance,
        patch: MasterPatch,
        kind: u8,
        path: MasterConstruction,
    ) -> Option<PreparedMasterConstruction> {
        let definition = self.definitions.get(usize::from(kind))?;
        let count = definition.parameter_count;
        if count > 20 {
            return None;
        }
        let mut next = *prior;
        let mut patch = patch;
        next.kind = kind;
        next.control.owner = u32::from(if path == MasterConstruction::StoredWithDefaultHistory {
            patch.header[1] & 15
        } else {
            definition.default_owner
        });
        next.control.delay.ratio = 0;
        next.control.delay.limited = 0;
        next.controller_offset = 64;
        next.enabled_argument = u32::from(patch.header[0] & 128 != 0);
        if path == MasterConstruction::Defaults {
            patch.header[1] = (patch.header[1] & 0xf0) | (definition.default_owner & 15);
            patch.parameters[..count].copy_from_slice(&definition.defaults[..count]);
        }
        next.parameters[..count].copy_from_slice(&patch.parameters[..count]);
        next.previous_parameters[..count].copy_from_slice(
            if path == MasterConstruction::StoredWithRawHistory {
                &patch.parameters[..count]
            } else {
                &definition.defaults[..count]
            },
        );
        if path != MasterConstruction::Defaults {
            for (value, range) in next.parameters[..count].iter_mut().zip(definition.ranges) {
                *value = range.clamp_encoded(*value);
            }
        }
        if kind == 29 {
            next.control.rotary_mode = u32::from(next.parameters[1]);
            next.control.rotary_speed = u32::from(next.parameters[5]);
        } else if kind == 27 {
            next.grain_history = GrainModulationHistory::default();
        } else {
            next.control.rotary_mode = 0;
            next.control.rotary_speed = 0;
        }
        Some(PreparedMasterConstruction { next, patch })
    }
}
