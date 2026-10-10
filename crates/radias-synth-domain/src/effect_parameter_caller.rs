//! Shared original effect mode mappings and prepared parameter calls.
use crate::{
    insert_effect_control::{InsertControlState, InsertControlStep},
    program::Program,
};
pub struct EffectParameterCallerTables {
    pub time_parameters: [[u8; 2]; 31],
    pub owner_modes: [[u8; 6]; 31],
}
pub struct PreparedEffectParameterCaller {
    pub next: InsertControlState,
    pub program: Program,
    pub steps: [Option<InsertControlStep>; 2],
    pub step_count: usize,
}
impl PreparedEffectParameterCaller {
    pub fn steps(&self) -> impl Iterator<Item = &InsertControlStep> {
        self.steps[..self.step_count].iter().flatten()
    }
}
pub(crate) fn mode_owner_change(record: [u8; 6], parameter: u8, value: u8) -> Option<[u8; 3]> {
    for triple in record.chunks_exact(3) {
        let [mode, zero, nonzero] = <[u8; 3]>::try_from(triple).ok()?;
        if parameter == mode {
            let selected = if value == 0 { zero } else { nonzero };
            return (selected != 0).then_some([zero, nonzero, selected]);
        }
    }
    None
}
