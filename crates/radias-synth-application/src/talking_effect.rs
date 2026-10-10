use radias_synth_domain::{
    effect_midi::{EffectMidiPolarity, EffectMidiSources},
    program::Program,
    talking_effect::{PreparedTalkingEdit, TalkingEdit, TalkingRack, TalkingTables},
};
pub trait TalkingEffectPort {
    type Error;
    fn accept_talking_edit(&mut self, edit: &PreparedTalkingEdit) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum TalkingError<E> {
    ParameterValue,
    Port(E),
}
#[derive(Clone, Copy)]
pub struct TalkingContext<'a> {
    pub tables: &'a TalkingTables,
    pub program: &'a Program,
    pub midi: &'a EffectMidiSources,
    pub polarity: EffectMidiPolarity,
}
pub fn change_talking_parameter<P: TalkingEffectPort>(
    port: &mut P,
    rack: &mut TalkingRack,
    context: TalkingContext<'_>,
    edit: TalkingEdit,
) -> Result<(), TalkingError<P::Error>> {
    let p = context
        .tables
        .prepare(rack, context.program, context.midi, context.polarity, edit)
        .ok_or(TalkingError::ParameterValue)?;
    port.accept_talking_edit(&p).map_err(TalkingError::Port)?;
    *rack = p.next;
    Ok(())
}

pub fn update_talking_midi<P: TalkingEffectPort>(
    port: &mut P,
    rack: &mut TalkingRack,
    tables: &TalkingTables,
    midi: &EffectMidiSources,
    polarity: EffectMidiPolarity,
) -> Result<(), TalkingError<P::Error>> {
    let p = tables
        .prepare_midi(rack, midi, polarity)
        .ok_or(TalkingError::ParameterValue)?;
    port.accept_talking_edit(&p).map_err(TalkingError::Port)?;
    *rack = p.next;
    Ok(())
}
