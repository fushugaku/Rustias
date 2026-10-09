//! Direct startup caller publications on the shared instrument FIFO.
use crate::{
    dsp_transport::{ParameterSendRequest, SendQueueError},
    parameter_transport::OrderedParameterTransport,
};
use radias_synth_domain::{
    actor_startup::{CoefficientPriming, PhysicalPhaseInitialization},
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
pub fn phases<const N: usize>(
    queue: &mut OrderedParameterTransport<N>,
    origin: u64,
    slot: usize,
    phase: &PhysicalPhaseInitialization,
) -> Result<(), SendQueueError> {
    if slot >= 24 {
        return Err(SendQueueError::InvalidSlot);
    }
    if phase.count > 2
        || phase.words[..usize::from(phase.count)]
            .iter()
            .any(|w| w.offset >= 63)
    {
        return Err(SendQueueError::UnsupportedSender);
    }
    let count = usize::from(phase.count);
    if queue.remaining_capacity() < count.max(1) {
        return Err(SendQueueError::Full);
    }
    if count == 0 {
        return queue.enqueue_work(origin, 31);
    }
    for (index, word) in phase.words[..count].iter().enumerate() {
        queue.enqueue_with_spacing(
            request(
                origin,
                slot,
                14,
                0x3000 + 64 * (slot % 12) as u32 + u32::from(word.offset),
                word.value,
            ),
            if index == 0 { 43 } else { 15 },
            if index + 1 == count { 17 } else { 0 },
        )?;
    }
    Ok(())
}
pub fn callback<const N: usize>(
    queue: &mut OrderedParameterTransport<N>,
    origin: u64,
    slot: usize,
    kind: radias_synth_domain::actor_startup::PhaseCallback,
    cached_word: i16,
    control: radias_synth_domain::controller_primary::PrimaryControl,
) -> Result<(), SendQueueError> {
    use radias_synth_domain::actor_startup::PhaseCallback;
    if slot >= 24 {
        return Err(SendQueueError::InvalidSlot);
    }
    let base = 0x2000 + 160 * (slot % 12) as u32;
    match kind {
        PhaseCallback::None => queue.enqueue_work(origin, 31),
        PhaseCallback::CachedWord => queue.enqueue_with_spacing(
            request(origin, slot, 0, base + 15, u32::from(cached_word as u16)),
            53,
            14,
        ),
        PhaseCallback::Unison | PhaseCallback::UnisonTriangle => {
            let physical = 0x3000 + 64 * (slot % 12) as u32;
            let value = (u32::from(control.phase_code()) << 16) | physical;
            queue.enqueue_with_spacing(
                request(
                    origin,
                    slot,
                    if kind == PhaseCallback::UnisonTriangle {
                        15
                    } else {
                        16
                    },
                    base + 22,
                    value,
                ),
                85,
                17,
            )
        }
    }
}
pub fn counter<const N: usize>(
    queue: &mut OrderedParameterTransport<N>,
    origin: u64,
    slot: usize,
    selected: bool,
    controller_slot: u8,
    seeds: &radias_synth_domain::controller_noise::FormantCounterSeeds,
) -> Result<(), SendQueueError> {
    if slot >= 24 {
        return Err(SendQueueError::InvalidSlot);
    }
    if !selected {
        return queue.enqueue_work(origin, 33);
    }
    queue.enqueue_with_spacing(
        request(
            origin,
            slot,
            0,
            0x2000 + 160 * (slot % 12) as u32 + 10,
            u32::from(seeds.for_slot(controller_slot) as u16),
        ),
        58,
        15,
    )
}
pub fn prime<const N: usize>(
    queue: &mut OrderedParameterTransport<N>,
    origin: u64,
    slot: usize,
    priming: CoefficientPriming,
) -> Result<(), SendQueueError> {
    if slot >= 24 {
        return Err(SendQueueError::InvalidSlot);
    }
    if queue.remaining_capacity() < if priming.pickup() { 2 } else { 1 } {
        return Err(SendQueueError::Full);
    }
    let base = 0x2000 + 160 * (slot % 12) as u32;
    if priming.pickup() {
        queue.enqueue_with_spacing(
            request(
                origin,
                slot,
                ParameterPacket::PICKUP_PRIME_SENDER,
                base + 2,
                0,
            ),
            priming.first_sender_gap(),
            0,
        )?;
    }
    queue.enqueue_with_spacing(
        request(origin, slot, 10, base + 6, 1),
        if priming.pickup() {
            17
        } else {
            priming.first_sender_gap()
        },
        6,
    )
}
