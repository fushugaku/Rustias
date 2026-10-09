//! A typed complete actor-copy caller using the instrument's shared FIFO.
use crate::{
    dsp_transport::{ParameterSendRequest, SendQueueError},
    parameter_transport::OrderedParameterTransport,
};
use radias_synth_domain::{
    actor_copy::{ActorTemplateBinding, ParameterTemplateAddresses},
    dsp_control::{DspEndpoint, ParameterPacket},
};

pub fn enqueue<const N: usize>(
    queue: &mut OrderedParameterTransport<N>,
    origin: u64,
    slot: usize,
    binding: ActorTemplateBinding,
    addresses: &ParameterTemplateAddresses,
) -> Result<(), SendQueueError> {
    if slot >= 24 {
        return Err(SendQueueError::InvalidSlot);
    }
    queue.enqueue_with_spacing(
        ParameterSendRequest {
            endpoint: if slot < 12 {
                DspEndpoint::Master
            } else {
                DspEndpoint::Slave
            },
            sender: ParameterPacket::ACTOR_COPY_SENDER,
            address: 0x2000 + 160 * (slot % 12) as u32,
            value: u32::from(binding.source(addresses)),
            available_clock: origin,
        },
        binding.sender_gap(),
        ActorTemplateBinding::RETURN_GAP,
    )
}
