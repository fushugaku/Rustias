use radias_synth_domain::{
    effect_rack_rebuild::{
        EffectRackRebuildContext, EffectRackRebuildTables, PreparedEffectRackRebuild,
    },
    insert_effect_control::InsertControlState,
};
/// Accept all FX and cross-context timbre-output steps before committing state.
pub trait EffectRackRebuildPort {
    type Error;
    fn accept_effect_rack_rebuild(
        &mut self,
        prepared: &PreparedEffectRackRebuild,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum EffectRackRebuildError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn rebuild_effect_rack<P: EffectRackRebuildPort>(
    state: &mut InsertControlState,
    port: &mut P,
    tables: &EffectRackRebuildTables,
    context: EffectRackRebuildContext<'_>,
) -> Result<(), EffectRackRebuildError<P::Error>> {
    let prepared = tables
        .prepare(state, context)
        .ok_or(EffectRackRebuildError::InvalidPreparation)?;
    port.accept_effect_rack_rebuild(&prepared)
        .map_err(EffectRackRebuildError::Port)?;
    *state = prepared.next;
    Ok(())
}
