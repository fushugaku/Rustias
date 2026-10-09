//! Bounded constructor publication, retaining one shared FIFO under24-voice load.
use crate::{dsp_transport::SendQueueError, synthesis_transport::SynthesisParameterTransport};
use radias_synth_domain::actor_construction::{
    ActorConstructionError, ActorConstructionRequest, ActorConstructionState,
    ActorConstructionTables, ConstructedActors,
};

pub struct ActorConstructionRun {
    constructed: ConstructedActors,
    next_step: usize,
    origin: u64,
    validated: bool,
}
impl ActorConstructionRun {
    pub fn prepare(
        origin: u64,
        state: &ActorConstructionState,
        request: ActorConstructionRequest<'_>,
        tables: &ActorConstructionTables<'_>,
    ) -> Result<Self, ActorConstructionError> {
        Ok(Self {
            constructed: state.construct(request, tables)?,
            next_step: 0,
            origin,
            validated: false,
        })
    }
    /// Stop at FIFO capacity; a later call resumes the next complete publication.
    pub fn enqueue_available(
        &mut self,
        transport: &mut SynthesisParameterTransport,
    ) -> Result<(), SendQueueError> {
        if !self.validated {
            for step in self.constructed.steps() {
                transport.validate_actor_descriptors(step.slot, &step.publication)?;
            }
            self.validated = true;
        }
        while let Some(step) = self.constructed.steps().get(self.next_step) {
            if transport.descriptor_capacity() < step.publication.operations().len() {
                break;
            }
            transport.publish_actor_descriptors(self.origin, step.slot, &step.publication)?;
            self.next_step += 1;
        }
        Ok(())
    }
    pub fn staged_state(&self) -> &ActorConstructionState {
        &self.constructed.state
    }
    /// Controller ownership changes only once the entire prepared call has returned.
    pub fn finish(
        &self,
        clock: u64,
        state: &mut ActorConstructionState,
        transport: &mut SynthesisParameterTransport,
    ) -> bool {
        if self.next_step != self.constructed.steps().len()
            || transport.pending() != 0
            || transport.caller_available_clock() > clock
        {
            return false;
        }
        *state = self.constructed.state;
        transport.set_actor_lifecycle(state.lifecycle);
        true
    }
}
