use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::mod_delay::{ModDelayEdit, ModDelayRack, ModDelayTables};
#[derive(Debug, PartialEq, Eq)]
pub enum ModDelayError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_mod_delay<Q: EffectParameterQueue>(
    rack: &mut ModDelayRack,
    queue: &mut Q,
    edit: ModDelayEdit,
    tables: &ModDelayTables,
) -> Result<(), ModDelayError<Q::Error>> {
    let p = tables
        .prepare(rack, edit)
        .ok_or(ModDelayError::ParameterValue)?;
    queue
        .enqueue_parameter(&p.batch)
        .map_err(ModDelayError::Queue)?;
    *rack = p.next;
    Ok(())
}
