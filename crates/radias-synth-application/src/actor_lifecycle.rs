//! Native lifecycle caller work and ordered publication, without sample delays.
use crate::{
    dsp_transport::{ParameterSendRequest, SendQueueError},
    parameter_transport::OrderedParameterTransport,
};
use radias_synth_domain::{
    actor_lifecycle::ActorLifecycle,
    amplifier_delivery::AmplifierRateTable,
    dsp_control::{DspEndpoint, ParameterPacket},
};

fn request(origin: u64, slot: usize, sender: u8, address: u32, value: u32) -> ParameterSendRequest {
    ParameterSendRequest {
        endpoint: if slot < 12 {
            DspEndpoint::Master
        } else {
            DspEndpoint::Slave
        },
        sender,
        address,
        value,
        available_clock: origin,
    }
}

/// The controller mask is published before the sender can block, as in SYS.
pub fn activate<const N: usize>(
    queue: &mut OrderedParameterTransport<N>,
    lifecycle: &mut ActorLifecycle,
    origin: u64,
    slot: usize,
) -> Result<(), SendQueueError> {
    if slot >= 24 {
        return Err(SendQueueError::InvalidSlot);
    }
    queue.enqueue_with_spacing(
        request(origin, slot, 0, 0x2000 + 160 * (slot % 12) as u32, 1),
        29,
        6,
    )?;
    lifecycle.activate(slot);
    Ok(())
}

/// Opcode38 clears the actor flag and transfers its tail state to its frame.
pub fn detach<const N: usize>(
    queue: &mut OrderedParameterTransport<N>,
    lifecycle: &mut ActorLifecycle,
    origin: u64,
    slot: usize,
) -> Result<(), SendQueueError> {
    if slot >= 24 {
        return Err(SendQueueError::InvalidSlot);
    }
    if !lifecycle.contains(slot) {
        return queue.enqueue_work(origin, 20);
    }
    queue.enqueue_with_spacing(
        request(
            origin,
            slot,
            ParameterPacket::DETACH_ACTOR_SENDER,
            0x300c + 64 * (slot % 12) as u32,
            0x2000 + 160 * (slot % 12) as u32,
        ),
        30,
        6,
    )?;
    lifecycle.detach(slot);
    Ok(())
}

/// SYS002ae0 clears cached targetf0 and publishes the original reset rate.
pub fn reset_amplifier<const N: usize>(
    queue: &mut OrderedParameterTransport<N>,
    target: &mut i16,
    origin: u64,
    slot: usize,
    tables: &AmplifierRateTable,
) -> Result<(), SendQueueError> {
    if slot >= 24 {
        return Err(SendQueueError::InvalidSlot);
    }
    queue.enqueue_with_spacing(
        request(
            origin,
            slot,
            19,
            0x207c + 160 * (slot % 12) as u32,
            u32::from(tables.reset_rate) << 16,
        ),
        24,
        6,
    )?;
    *target = 0;
    Ok(())
}
