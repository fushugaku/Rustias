use radias_synth_domain::{
    master_effect_control::MasterControlTables,
    mixed_effect_midi::{MixedEffectMidiFrame, MixedEffectMidiState, PreparedMixedEffectMidi},
};
pub trait MixedEffectMidiPort {
    type Error;
    fn accept_mixed_effect_midi(
        &mut self,
        prepared: &PreparedMixedEffectMidi,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MixedEffectMidiError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn update_mixed_effect_midi<P: MixedEffectMidiPort>(
    state: &mut MixedEffectMidiState,
    port: &mut P,
    tables: &MasterControlTables,
    frame: MixedEffectMidiFrame,
) -> Result<(), MixedEffectMidiError<P::Error>> {
    let prepared = tables
        .prepare_mixed_midi(state, frame)
        .ok_or(MixedEffectMidiError::InvalidPreparation)?;
    port.accept_mixed_effect_midi(&prepared)
        .map_err(MixedEffectMidiError::Port)?;
    *state = prepared.next;
    Ok(())
}
