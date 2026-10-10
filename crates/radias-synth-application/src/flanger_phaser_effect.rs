use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::{
    flanger_phaser_effect::{FlangerPhaserEdit, FlangerPhaserRack, FlangerPhaserTables},
    program::Program,
};
#[derive(Debug, PartialEq, Eq)]
pub enum FlangerPhaserError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_flanger_phaser_parameter<Q: EffectParameterQueue>(
    rack: &mut FlangerPhaserRack,
    queue: &mut Q,
    tables: &FlangerPhaserTables,
    program: &Program,
    edit: FlangerPhaserEdit,
) -> Result<(), FlangerPhaserError<Q::Error>> {
    let p = tables
        .prepare(rack, program, edit)
        .ok_or(FlangerPhaserError::ParameterValue)?;
    queue
        .enqueue_parameter(&p.batch)
        .map_err(FlangerPhaserError::Queue)?;
    *rack = p.next;
    Ok(())
}
