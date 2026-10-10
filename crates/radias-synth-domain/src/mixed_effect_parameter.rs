//! Unified original Insert/Master parameter dispatch with mixed MIDI callbacks.
use crate::{
    insert_effect_control::{InsertControlState, InsertControlStep, InsertControlTables},
    master_initial_mask::MasterInitialMaskContext,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectParameterTarget {
    Insert(u8),
    Master,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixedEffectParameterEdit {
    pub target: EffectParameterTarget,
    pub parameter: u8,
    pub value: u8,
}
pub struct PreparedMixedEffectParameter {
    pub next: InsertControlState,
    pub step: InsertControlStep,
}
impl InsertControlTables {
    /// Current/previous parameter arrays are explicit caller state. The
    /// requested byte remains a separate input, as in SYS07AC52/SYS07BBFA.
    pub fn prepare_mixed_effect_parameter(
        &self,
        state: &InsertControlState,
        edit: MixedEffectParameterEdit,
        context: MasterInitialMaskContext<'_>,
    ) -> Option<PreparedMixedEffectParameter> {
        self.prepare_mixed_parameter_inner(state, edit, context, false)
    }
    /// Stored bytes reach the original callbacks independently of the current
    /// constructor-clamped parameter arrays. Insert retains MOV.B signedness;
    /// Master retains the unsigned EXTU.B argument. All stored dynamics bytes
    /// are supported; other families retain their individual range contracts.
    pub fn prepare_stored_mixed_effect_parameter(
        &self,
        state: &InsertControlState,
        edit: MixedEffectParameterEdit,
        context: MasterInitialMaskContext<'_>,
    ) -> Option<PreparedMixedEffectParameter> {
        self.prepare_mixed_parameter_inner(state, edit, context, true)
    }
    fn prepare_mixed_parameter_inner(
        &self,
        state: &InsertControlState,
        edit: MixedEffectParameterEdit,
        context: MasterInitialMaskContext<'_>,
        stored: bool,
    ) -> Option<PreparedMixedEffectParameter> {
        let (next, step) = match edit.target {
            EffectParameterTarget::Insert(slot) if stored => self.prepare_stored_parameter(
                state,
                slot,
                edit.parameter,
                edit.value,
                context.common,
            )?,
            EffectParameterTarget::Insert(slot) => {
                self.prepare_parameter(state, slot, edit.parameter, edit.value, context.common)?
            }
            EffectParameterTarget::Master if stored => {
                self.prepare_master_stored_parameter(state, edit.parameter, edit.value, context)?
            }
            EffectParameterTarget::Master => {
                self.prepare_master_parameter(state, edit.parameter, edit.value, context)?
            }
        };
        Some(PreparedMixedEffectParameter { next, step })
    }
}
