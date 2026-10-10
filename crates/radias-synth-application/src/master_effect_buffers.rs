use radias_synth_domain::master_effect_buffers::{
    MasterBufferState, PreparedMasterBufferTemplate, relocate_master_template,
};
pub trait MasterBufferTemplatePort {
    type Error;
    fn accept_master_template(
        &mut self,
        prepared: &PreparedMasterBufferTemplate,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterBufferError<E> {
    InvalidTemplate,
    Port(E),
}
pub fn relocate_master_effect_buffer_template<P: MasterBufferTemplatePort>(
    state: &mut MasterBufferState,
    port: &mut P,
    kind: u8,
    input: &[u32],
) -> Result<(), MasterBufferError<P::Error>> {
    let prepared =
        relocate_master_template(kind, *state, input).ok_or(MasterBufferError::InvalidTemplate)?;
    port.accept_master_template(&prepared)
        .map_err(MasterBufferError::Port)?;
    *state = prepared.next;
    Ok(())
}
