use radias_synth_domain::effect_buffers::{EffectBufferTables, PreparedEffectBufferTemplate};
pub trait EffectBufferTemplatePort {
    type Error;
    fn accept_template(
        &mut self,
        template: &PreparedEffectBufferTemplate,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum EffectBufferTemplateError<E> {
    Template,
    Port(E),
}
pub fn relocate_effect_buffer_template<P: EffectBufferTemplatePort>(
    current: &mut PreparedEffectBufferTemplate,
    port: &mut P,
    kind: u8,
    buffer_origin: u32,
    tables: &EffectBufferTables,
) -> Result<(), EffectBufferTemplateError<P::Error>> {
    let next = tables
        .relocate_template(kind, current.layout, buffer_origin, current.words())
        .ok_or(EffectBufferTemplateError::Template)?;
    port.accept_template(&next)
        .map_err(EffectBufferTemplateError::Port)?;
    *current = next;
    Ok(())
}
