use crate::{
    effect_transition_queue::{
        EffectProgramSource, EffectPublicationError, EffectQueueServiceInputs,
        service_effect_transition_queue_with_context,
    },
    effects::EffectProgramPort,
};
use radias_synth_domain::{
    effect_transition_queue::EffectTransitionQueue,
    master_assignment_release::MasterAssignmentRelease, master_effect_control::MasterControlState,
};
/// Resume the original ordered release with its partially updated pool state.
pub fn release_master_assignments_with_service<
    P: EffectProgramPort,
    S: EffectProgramSource,
    C: EffectQueueServiceInputs,
>(
    state: &mut MasterControlState,
    release: &mut MasterAssignmentRelease,
    queue: &mut EffectTransitionQueue,
    port: &mut P,
    source: &S,
    inputs: &mut C,
) -> Result<(), EffectPublicationError<P::Error>> {
    while !release.publish_available(state, queue) {
        let (tick, status) = inputs
            .next_service_inputs()
            .ok_or(EffectPublicationError::ServiceInputRequired)?;
        service_effect_transition_queue_with_context(
            queue,
            port,
            source,
            tick,
            status,
            release.host_context(),
        )
        .map_err(EffectPublicationError::Delivery)?;
    }
    Ok(())
}
