//! Submit a whole St.Decimator parameter change before committing assignment
//! state. DSP compilation belongs to the domain; transport belongs to the port.
use crate::effects::{EffectProgramPort, EffectUpdateController};
use radias_synth_domain::{
    decimator_effect::{
        DecimatorEffectChange, DecimatorEffectTables, EffectInterpolationControl,
        EffectParameterBatch,
    },
    effect_updates::CoefficientQueueWord,
};
pub trait EffectParameterQueue {
    type Error;
    /// Accept all words atomically, or reject without modifying the queue.
    fn enqueue_parameter(&mut self, batch: &EffectParameterBatch) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum DecimatorEffectError<E> {
    ParameterValue,
    Queue(E),
}
impl EffectUpdateController {
    pub fn change_decimator<Q: EffectParameterQueue>(
        &mut self,
        queue: &mut Q,
        tables: &DecimatorEffectTables,
        origin: u16,
        change: DecimatorEffectChange,
        interpolation: EffectInterpolationControl,
    ) -> Result<(), DecimatorEffectError<Q::Error>> {
        let prepared = tables
            .prepare(&self.assignments, origin, change, interpolation)
            .ok_or(DecimatorEffectError::ParameterValue)?;
        queue
            .enqueue_parameter(&prepared.batch)
            .map_err(DecimatorEffectError::Queue)?;
        self.assignments = prepared.next;
        Ok(())
    }
}
pub fn dispatch_parameter_batch<P: EffectProgramPort>(
    port: &mut P,
    batch: &EffectParameterBatch,
) -> Result<(), P::Error> {
    dispatch_words(port, batch.words())
}
pub(crate) fn dispatch_words<P: EffectProgramPort>(
    port: &mut P,
    words: &[CoefficientQueueWord],
) -> Result<(), P::Error> {
    let mut cursor = 0;
    while cursor < words.len() {
        let entry = words[cursor];
        let count = (if entry.tagged_value & 0x80000000 != 0 {
            (entry.tagged_value >> 24) & 7
        } else {
            1
        })
        .max(1) as usize;
        let mut values = [0u32; 4];
        for (i, value) in values[..count].iter_mut().enumerate() {
            *value = words[cursor + i].tagged_value & 0xffffff;
        }
        port.write_coefficient_packet(entry.address, &values[..count], 1)?;
        cursor += count;
    }
    Ok(())
}
