//! FX queue scheduling and host delivery; the domain owns switching and tags.
use crate::effects::EffectProgramPort;
use radias_synth_domain::effect_transition_queue::{
    EffectQueueHostContext, EffectQueuePublication, EffectTransitionBatch, EffectTransitionQueue,
};

pub trait EffectQueueServiceInputs {
    /// Each invocation supplies the timer and readiness observed by the next
    /// synchronous SYS01D7E8 call. None suspends publication without retrying
    /// or changing the pinned producer call.
    fn next_service_inputs(&mut self) -> Option<(u16, u16)>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum EffectPublicationError<E> {
    ServiceInputRequired,
    Delivery(EffectQueueDeliveryError<E>),
}
pub fn publish_effect_transition_words<
    P: EffectProgramPort,
    S: EffectProgramSource,
    C: EffectQueueServiceInputs,
>(
    queue: &mut EffectTransitionQueue,
    publication: &mut EffectQueuePublication<'_>,
    port: &mut P,
    source: &S,
    inputs: &mut C,
) -> Result<bool, EffectPublicationError<P::Error>> {
    while !publication.publish_available(queue) {
        let (tick, status) = inputs
            .next_service_inputs()
            .ok_or(EffectPublicationError::ServiceInputRequired)?;
        service_effect_transition_queue_with_context(
            queue,
            port,
            source,
            tick,
            status,
            publication.host_context(),
        )
        .map_err(EffectPublicationError::Delivery)?;
    }
    Ok(publication.pending_switch())
}

impl crate::effect_parameters::EffectParameterQueue for EffectTransitionQueue {
    type Error = radias_synth_domain::effect_queue::EffectQueueError;
    fn enqueue_parameter(
        &mut self,
        batch: &radias_synth_domain::effect_parameters::EffectParameterBatch,
    ) -> Result<(), Self::Error> {
        if batch.lfo_publication().is_some() {
            return Err(Self::Error::UnsupportedCommand);
        }
        self.enqueue_words(batch.words()).map(|_| ())
    }
}

pub trait EffectProgramSource {
    fn program_words(&self, selector: u8) -> Option<&[u64]>;
}
impl EffectProgramSource for radias_synth_domain::effect_program_staging::EffectProgramStaging {
    fn program_words(&self, selector: u8) -> Option<&[u64]> {
        self.program_words(selector)
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum EffectQueueDeliveryError<E> {
    MissingProgram(u8),
    Port(E),
}
pub fn dispatch_effect_transition_batch<P: EffectProgramPort, S: EffectProgramSource>(
    port: &mut P,
    source: &S,
    batch: &EffectTransitionBatch,
) -> Result<(), EffectQueueDeliveryError<P::Error>> {
    let program = if let Some(request) = batch.program {
        Some((
            request,
            source
                .program_words(request.selector)
                .ok_or(EffectQueueDeliveryError::MissingProgram(request.selector))?,
        ))
    } else {
        None
    };
    for (index, packet) in batch.coefficients.packets[..usize::from(batch.coefficients.count)]
        .iter()
        .enumerate()
    {
        port.write_coefficient_packet(
            packet.address,
            &packet.values[..usize::from(packet.count)],
            batch.coefficient_controls[index],
        )
        .map_err(EffectQueueDeliveryError::Port)?;
    }
    if let Some((request, words)) = program {
        for (index, chunk) in words.chunks(16).enumerate() {
            port.upload_program(
                request.destination.wrapping_add((index * 16) as u16),
                chunk,
                1,
            )
            .map_err(EffectQueueDeliveryError::Port)?;
        }
    }
    Ok(())
}
pub fn service_effect_transition_queue<P: EffectProgramPort, S: EffectProgramSource>(
    queue: &mut EffectTransitionQueue,
    port: &mut P,
    source: &S,
    tick: u16,
    host_status: u16,
) -> Result<(), EffectQueueDeliveryError<P::Error>> {
    dispatch_effect_transition_batch(port, source, &queue.service(tick, host_status))
}
pub fn service_effect_transition_queue_with_context<
    P: EffectProgramPort,
    S: EffectProgramSource,
>(
    queue: &mut EffectTransitionQueue,
    port: &mut P,
    source: &S,
    tick: u16,
    host_status: u16,
    context: &mut EffectQueueHostContext,
) -> Result<(), EffectQueueDeliveryError<P::Error>> {
    dispatch_effect_transition_batch(
        port,
        source,
        &queue.service_with_context(tick, host_status, context),
    )
}
