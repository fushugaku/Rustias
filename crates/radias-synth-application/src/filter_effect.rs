//! Atomic acceptance of St.Filter coefficient and frequency-cache updates.
use crate::{effect_parameters::EffectParameterQueue, effects::EffectUpdateController};
use radias_synth_domain::{
    effect_parameters::EffectInterpolationControl,
    filter_effect::{FilterEffectCache, FilterEffectFrequency, FilterEffectTables},
};
pub struct FilterEffectController {
    pub updates: EffectUpdateController,
    pub caches: [FilterEffectCache; 9],
}
#[derive(Debug, PartialEq, Eq)]
pub enum FilterEffectError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_filter_parameter<Q: EffectParameterQueue>(
    rack: &mut radias_synth_domain::filter_effect_parameters::FilterEffectRack,
    queue: &mut Q,
    edit: radias_synth_domain::filter_effect_parameters::FilterParameterEdit,
    tables: &radias_synth_domain::filter_effect_parameters::FilterParameterTables<'_>,
    midi: &radias_synth_domain::effect_midi::EffectMidiSources,
    program: &radias_synth_domain::program::Program,
) -> Result<(), FilterEffectError<Q::Error>> {
    let prepared = rack
        .prepare(edit, tables, midi, program)
        .ok_or(FilterEffectError::ParameterValue)?;
    queue
        .enqueue_parameter(&prepared.batch)
        .map_err(FilterEffectError::Queue)?;
    *rack = prepared.next;
    Ok(())
}
impl FilterEffectController {
    pub fn update_frequency<Q: EffectParameterQueue>(
        &mut self,
        queue: &mut Q,
        tables: &FilterEffectTables,
        slot: u8,
        dirty: u32,
        change: FilterEffectFrequency,
    ) -> Result<(), FilterEffectError<Q::Error>> {
        let cache = self
            .caches
            .get(usize::from(slot))
            .ok_or(FilterEffectError::ParameterValue)?;
        let prepared = tables
            .prepare_frequency(
                FilterEffectCache {
                    frequency: cache.frequency,
                    dirty,
                },
                change,
            )
            .ok_or(FilterEffectError::ParameterValue)?;
        queue
            .enqueue_parameter(&prepared.batch)
            .map_err(FilterEffectError::Queue)?;
        self.caches[usize::from(slot)] = prepared.next;
        Ok(())
    }
}
impl EffectUpdateController {
    pub fn change_filter_trim<Q: EffectParameterQueue>(
        &mut self,
        queue: &mut Q,
        tables: &FilterEffectTables,
        origin: u16,
        resonance: u8,
        trim: u8,
    ) -> Result<(), FilterEffectError<Q::Error>> {
        let batch = tables
            .prepare_trim(origin, resonance, trim)
            .ok_or(FilterEffectError::ParameterValue)?;
        queue
            .enqueue_parameter(&batch)
            .map_err(FilterEffectError::Queue)
    }
    pub fn change_filter_response<Q: EffectParameterQueue>(
        &mut self,
        queue: &mut Q,
        tables: &FilterEffectTables,
        origin: u16,
        value: u8,
        interpolation: EffectInterpolationControl,
    ) -> Result<(), FilterEffectError<Q::Error>> {
        let prepared = tables
            .prepare_response(&self.assignments, origin, value, interpolation)
            .ok_or(FilterEffectError::ParameterValue)?;
        queue
            .enqueue_parameter(&prepared.batch)
            .map_err(FilterEffectError::Queue)?;
        self.assignments = prepared.next;
        Ok(())
    }
}
