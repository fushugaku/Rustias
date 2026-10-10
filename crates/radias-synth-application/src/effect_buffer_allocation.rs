use radias_synth_domain::{
    delay_time::DelayClock,
    effect_buffer_allocation::{
        EffectBufferAllocationTables, EffectBufferInstance, PreparedEffectBufferAllocation,
    },
    effect_buffers::PreparedEffectBufferTemplate,
};
pub trait EffectBufferAllocationPort {
    type Error;
    fn accept_allocation(
        &mut self,
        allocation: &PreparedEffectBufferAllocation,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum EffectBufferAllocationError<E> {
    Input,
    Port(E),
}
pub fn relocate_effect_buffers<P: EffectBufferAllocationPort>(
    instances: &mut [EffectBufferInstance; 8],
    template: &mut PreparedEffectBufferTemplate,
    port: &mut P,
    slot: u8,
    clock: DelayClock,
    tables: &EffectBufferAllocationTables,
) -> Result<(), EffectBufferAllocationError<P::Error>> {
    let next = tables
        .prepare(instances, slot, template.words(), clock)
        .ok_or(EffectBufferAllocationError::Input)?;
    port.accept_allocation(&next)
        .map_err(EffectBufferAllocationError::Port)?;
    *instances = next.instances;
    *template = next.template;
    Ok(())
}
