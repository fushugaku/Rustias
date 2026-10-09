//! Complete initial Filter1 publication on the common synthesis FIFO.
use crate::{dsp_transport::SendQueueError, synthesis_transport::SynthesisParameterTransport};
use radias_synth_domain::actor_control_state::ActorControlState;
pub fn publish_initial_filter1_frequency(
    clock: u64,
    slot: usize,
    controller: &ActorControlState,
    body: &[u8; 104],
    transport: &mut SynthesisParameterTransport,
) -> Result<(), SendQueueError> {
    transport.publish_actor_descriptors(
        clock,
        slot,
        &controller.compile_initial_filter1_frequency(body),
    )
}
