//! Shared effect parameter transport; domain compilers own all coefficients.
use crate::effects::EffectProgramPort;
use radias_synth_domain::{
    effect_lfo_program::EffectLfoPublication, effect_parameters::EffectParameterBatch,
    effect_updates::CoefficientQueueWord,
};
pub trait EffectParameterQueue {
    type Error;
    /// Accept all words atomically, or reject without modifying the queue.
    fn enqueue_parameter(&mut self, batch: &EffectParameterBatch) -> Result<(), Self::Error>;
}
pub trait EffectControlPort: EffectProgramPort {
    fn publish_effect_lfo(&mut self, publication: EffectLfoPublication) -> Result<(), Self::Error>;
}
pub fn dispatch_parameter_batch<P: EffectProgramPort>(
    port: &mut P,
    batch: &EffectParameterBatch,
) -> Result<(), P::Error> {
    assert!(
        batch.lfo_publication().is_none(),
        "LFO publication requires an EffectControlPort"
    );
    dispatch_words(port, batch.words())
}
pub fn dispatch_complete_parameter_batch<P: EffectControlPort>(
    port: &mut P,
    batch: &EffectParameterBatch,
) -> Result<(), P::Error> {
    dispatch_words(port, batch.words())?;
    if let Some(p) = batch.lfo_publication() {
        port.publish_effect_lfo(p)?;
    }
    Ok(())
}
pub(crate) fn dispatch_words<P: EffectProgramPort>(
    port: &mut P,
    words: &[CoefficientQueueWord],
) -> Result<(), P::Error> {
    let mut cursor = 0;
    while cursor < words.len() {
        let entry = words[cursor];
        assert!(
            entry.tagged_value >> 24 == 0 || entry.tagged_value & 0x80000000 != 0,
            "Transition commands require EffectCommandQueue service"
        );
        let count = (if entry.tagged_value & 0x80000000 != 0 {
            (entry.tagged_value >> 24) & 7
        } else {
            1
        })
        .max(1) as usize;
        let mut values = [0u32; 7];
        for (i, value) in values[..count].iter_mut().enumerate() {
            *value = words[cursor + i].tagged_value & 0xffffff;
        }
        port.write_coefficient_packet(entry.address, &values[..count], 1)?;
        cursor += count;
    }
    Ok(())
}
