//! High Master type write retained across synchronous queue service.
use crate::{
    effect_rack_rebuild::EffectRackRebuildContext, insert_effect_control::InsertControlState,
    master_pressure_type_change::MasterPressureTypeChangeOperation,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasterPressureTypeValueOperation {
    value: Option<i32>,
    pressure: MasterPressureTypeChangeOperation,
}
impl MasterPressureTypeValueOperation {
    pub fn new(state: &InsertControlState, direct_switch: u32) -> Self {
        Self {
            value: None,
            pressure: MasterPressureTypeChangeOperation::new(state, direct_switch),
        }
    }
    pub fn value(&self) -> Option<i32> {
        self.value
    }
    pub fn begin(&mut self, value: i32) {
        if self.value.is_none() {
            self.value = Some(value);
        }
    }
    pub fn pressure_operation(&mut self) -> &mut MasterPressureTypeChangeOperation {
        &mut self.pressure
    }
    pub fn is_complete(&self) -> bool {
        self.pressure.is_complete()
    }
}

#[derive(Clone, Copy)]
pub struct MasterPressureTypeValueRequest<'a> {
    pub value: i32,
    pub rebuild: EffectRackRebuildContext<'a>,
}
