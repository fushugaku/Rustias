use radias_synth_domain::{
    effect_property::EffectPropertyTables,
    insert_effect_control::{InsertControlState, InsertControlTables},
    master_effect_initialization::MasterInitializationTables,
    master_type_value_change::{MasterTypeValueContext, PreparedMasterTypeValueChange},
    program::Program,
};
pub trait MasterTypeValuePort {
    type Error;
    fn accept_master_type_value(
        &mut self,
        prepared: &PreparedMasterTypeValueChange,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterTypeValueError<E> {
    InvalidOrPressurePreparation,
    Port(E),
}
pub struct MasterTypeValueTables<'a> {
    pub control: &'a InsertControlTables,
    pub properties: &'a EffectPropertyTables,
    pub initialization: &'a MasterInitializationTables,
}
pub fn change_master_type_value<P: MasterTypeValuePort>(
    state: &mut InsertControlState,
    program: &mut Program,
    port: &mut P,
    tables: MasterTypeValueTables<'_>,
    value: i32,
    context: MasterTypeValueContext<'_>,
) -> Result<(i32, u32, u32), MasterTypeValueError<P::Error>> {
    let p = tables
        .control
        .prepare_master_type_value_change(
            state,
            tables.properties,
            tables.initialization,
            value,
            context,
        )
        .ok_or(MasterTypeValueError::InvalidOrPressurePreparation)?;
    port.accept_master_type_value(&p)
        .map_err(MasterTypeValueError::Port)?;
    *state = p.next;
    *program = p.program;
    Ok((p.value, p.direct_switch, p.pressure_marker))
}
