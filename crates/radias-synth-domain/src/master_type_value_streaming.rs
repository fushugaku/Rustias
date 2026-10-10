use crate::{
    insert_effect_control::InsertControlState,
    master_pressure_type_value::MasterPressureTypeValueOperation,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MasterTypeValuePath {
    Idle,
    Pressure,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasterTypeValueOperation {
    inner: MasterPressureTypeValueOperation,
    path: Option<MasterTypeValuePath>,
}
impl MasterTypeValueOperation {
    pub fn new(state: &InsertControlState, direct_switch: u32) -> Self {
        Self {
            inner: MasterPressureTypeValueOperation::new(state, direct_switch),
            path: None,
        }
    }
    pub fn value(&self) -> Option<i32> {
        self.inner.value()
    }
    pub fn path(&self) -> Option<MasterTypeValuePath> {
        self.path
    }
    pub fn is_complete(&self) -> bool {
        self.path.is_some() && self.inner.is_complete()
    }
    pub fn inner(&mut self) -> &mut MasterPressureTypeValueOperation {
        &mut self.inner
    }
    pub fn finish(&mut self, path: MasterTypeValuePath) {
        self.inner.pressure_operation().finish();
        self.path = Some(path);
    }
}
