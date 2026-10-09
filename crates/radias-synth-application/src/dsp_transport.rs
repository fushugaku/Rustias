//! Original ready->HPIA->HPID->HINT-ack parameter send use case.
//! The adapter advances bus actions on its own clock; no fixed audio delay,
//! recorded coefficient or firmware interpreter is introduced here.
use radias_synth_domain::dsp_control::{DspEndpoint, ParameterPacket};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HpiAction {
    AddressByte {
        endpoint: DspEndpoint,
        offset: u8,
        value: u8,
    },
    DataWord {
        endpoint: DspEndpoint,
        offset: u8,
        value: u16,
    },
    AcknowledgeHint {
        endpoint: DspEndpoint,
        value: u16,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterTransfer {
    endpoint: DspEndpoint,
    packet: ParameterPacket,
    stage: u8,
}
impl ParameterTransfer {
    pub fn new(endpoint: DspEndpoint, packet: ParameterPacket) -> Self {
        Self {
            endpoint,
            packet,
            stage: 0,
        }
    }
    pub fn completed(&self) -> bool {
        self.stage as usize == self.packet.words().len() + 3
    }
    /// `None` means either busy before transfer or an already completed transfer.
    /// Once HPIA is written the original sender completes the packet without
    /// re-reading readiness between its word writes.
    pub fn advance(&mut self, sampled_port: u8) -> Option<HpiAction> {
        if self.completed() || self.stage == 0 && self.endpoint.busy(sampled_port) {
            return None;
        }
        let action = match self.stage {
            0 => HpiAction::AddressByte {
                endpoint: self.endpoint,
                offset: 4,
                value: 1,
            },
            1 => HpiAction::AddressByte {
                endpoint: self.endpoint,
                offset: 5,
                value: 0,
            },
            stage if (stage as usize) < self.packet.words().len() + 2 => HpiAction::DataWord {
                endpoint: self.endpoint,
                offset: 2,
                value: self.packet.words()[stage as usize - 2],
            },
            _ => HpiAction::AcknowledgeHint {
                endpoint: self.endpoint,
                value: ParameterPacket::COMMIT,
            },
        };
        self.stage += 1;
        Some(action)
    }
}

/// Clocked implementation of SYS00F330/00F3BC and their complete HPI subcalls.
/// These are clocks of the existing functional SH3 reference, not a claim of
/// electrical bus or hardware pipeline timing. The adapter supplies absolute
/// clock origins, readiness at each poll and interrupt preemption separately.
#[derive(Clone, Copy)]
pub struct TimedParameterTransfer {
    transfer: ParameterTransfer,
    endpoint: DspEndpoint,
    action_clocks: [u8; 12],
    action_count: u8,
    action_index: u8,
    next_clock: u64,
    busy_delay: u64,
    return_clock: u8,
    polling: bool,
    done: bool,
    origin: u64,
}
impl TimedParameterTransfer {
    pub fn from_sender(
        endpoint: DspEndpoint,
        entry: u8,
        address: u32,
        value: u32,
        origin: u64,
    ) -> Option<Self> {
        let packet = ParameterPacket::from_sender(entry, address, value)?;
        if matches!(
            entry,
            ParameterPacket::INITIAL_FILTER1_IMMEDIATE_SENDER
                | ParameterPacket::INITIAL_FILTER1_TIMED_SENDER
        ) {
            // SYS00f23a copies four payload words before its ready poll.
            return Some(Self {
                transfer: ParameterTransfer::new(endpoint, packet),
                endpoint,
                action_clocks: [74, 83, 92, 97, 100, 105, 108, 112, 116, 129, 0, 0],
                action_count: 10,
                action_index: 0,
                next_clock: origin + 56,
                busy_delay: 0,
                return_clock: 142,
                polling: true,
                done: false,
                origin,
            });
        }
        if entry == ParameterPacket::COMB_POINTERS_SENDER {
            // SYS00f21c constructs three header and six payload words before
            // polling. EA24 writes all nine words; EAFC commits afterwards.
            return Some(Self {
                transfer: ParameterTransfer::new(endpoint, packet),
                endpoint,
                action_clocks: [88, 97, 106, 111, 114, 119, 122, 127, 130, 134, 138, 151],
                action_count: 12,
                action_index: 0,
                next_clock: origin + 70,
                busy_delay: 0,
                return_clock: 164,
                polling: true,
                done: false,
                origin,
            });
        }
        if matches!(
            entry,
            ParameterPacket::ACTOR_COPY_SENDER
                | ParameterPacket::PICKUP_PRIME_SENDER
                | ParameterPacket::DETACH_ACTOR_SENDER
        ) {
            let pickup = u8::from(entry == ParameterPacket::PICKUP_PRIME_SENDER);
            let mut clocks = [42, 51, 60, 65, 68, 72, 88, 0, 0, 0, 0, 0];
            for clock in &mut clocks[..7] {
                *clock += pickup;
            }
            return Some(Self {
                transfer: ParameterTransfer::new(endpoint, packet),
                endpoint,
                action_clocks: clocks,
                action_count: 7,
                action_index: 0,
                next_clock: origin + 25 + u64::from(pickup),
                busy_delay: 0,
                return_clock: 98 + pickup,
                polling: true,
                done: false,
                origin,
            });
        }
        let long = entry >= 14;
        let compact = entry == 13 || entry == 20;
        // Count the original wrapper, ready reader and EA24/EAFC paths. Each
        // taken wrapper BRA with its slot adds three SH reference clocks.
        let reduction = if compact { 3 } else { 0 };
        let mut action_clocks = if long {
            [52, 61, 70, 75, 78, 83, 86, 90, 106, 0, 0, 0]
        } else {
            [49, 58, 67, 72, 75, 79, 83, 95, 0, 0, 0, 0]
        };
        let action_count = if long { 9 } else { 8 };
        for clock in &mut action_clocks[..action_count] {
            *clock -= reduction;
        }
        Some(Self {
            transfer: ParameterTransfer::new(endpoint, packet),
            endpoint,
            action_clocks,
            action_count: action_count as u8,
            action_index: 0,
            next_clock: origin + (if long { 35 } else { 32 }) - u64::from(reduction),
            busy_delay: 0,
            return_clock: (if long { 117 } else { 106 }) - reduction,
            polling: true,
            done: false,
            origin,
        })
    }
    pub fn next_clock(&self) -> Option<u64> {
        (!self.done).then_some(self.next_clock)
    }
    pub fn completed(&self) -> bool {
        self.done
    }
    pub fn polling(&self) -> bool {
        self.polling && !self.done
    }
    /// Execute exactly the bus/poll/return boundary exposed by `next_clock`.
    /// A busy poll returns no write and schedules the original21-clock loop.
    /// After HPIA begins, readiness is not reread between packet writes.
    pub fn advance(&mut self, sampled_port: u8) -> Option<HpiAction> {
        if self.done {
            return None;
        }
        if self.polling {
            if self.endpoint.busy(sampled_port) {
                self.busy_delay += 21;
                self.next_clock += 21;
            } else {
                self.polling = false;
                self.next_clock = self.origin + self.busy_delay + u64::from(self.action_clocks[0]);
            }
            return None;
        }
        if self.action_index == self.action_count {
            self.done = true;
            return None;
        }
        // The ready poll has already accepted the transfer. A subsequent port
        // sample cannot cancel its first HPIA byte or the remaining writes.
        let action = self.transfer.advance(0);
        self.action_index += 1;
        self.next_clock = self.origin
            + self.busy_delay
            + u64::from(if self.action_index == self.action_count {
                self.return_clock
            } else {
                self.action_clocks[self.action_index as usize]
            });
        action
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterSendRequest {
    pub endpoint: DspEndpoint,
    pub sender: u8,
    pub address: u32,
    pub value: u32,
    pub available_clock: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendQueueError {
    Full,
    UnsupportedSender,
    InvalidSlot,
    MissingFilterContext,
    MissingPitchContext,
    MissingParameterTemplate,
}

#[derive(Clone, Copy)]
struct SpacedSendRequest {
    request: ParameterSendRequest,
    before: u16,
    after: u16,
}
#[derive(Clone, Copy)]
enum CallerOperation {
    Send(SpacedSendRequest),
    Work { origin: u64, clocks: u16 },
}

/// Controller calls execute in one FIFO, even when they address different
/// processors. A busy endpoint blocks the original caller; it must not let a
/// later Slave/Master packet pass. Capacity is selected by the owning adapter.
pub struct ParameterSendQueue<const N: usize> {
    requests: [Option<CallerOperation>; N],
    head: usize,
    length: usize,
    transfer: Option<TimedParameterTransfer>,
    available_clock: u64,
}
impl<const N: usize> Default for ParameterSendQueue<N> {
    fn default() -> Self {
        Self {
            requests: [None; N],
            head: 0,
            length: 0,
            transfer: None,
            available_clock: 0,
        }
    }
}
impl<const N: usize> ParameterSendQueue<N> {
    pub fn push(&mut self, request: ParameterSendRequest) -> Result<(), SendQueueError> {
        self.push_with_spacing(request, 0, 0)
    }
    /// Direct caller work surrounding a sender. Relative gaps retain elapsed
    /// busy time across later sends, unlike precomputed absolute entry times.
    pub fn push_with_spacing(
        &mut self,
        request: ParameterSendRequest,
        before: u16,
        after: u16,
    ) -> Result<(), SendQueueError> {
        if ParameterPacket::from_sender(request.sender, request.address, request.value).is_none() {
            return Err(SendQueueError::UnsupportedSender);
        }
        if self.length == N {
            return Err(SendQueueError::Full);
        }
        self.requests[(self.head + self.length) % N] =
            Some(CallerOperation::Send(SpacedSendRequest {
                request,
                before,
                after,
            }));
        self.length += 1;
        Ok(())
    }
    /// A caller can consume instruction work without sending a packet, e.g.
    /// a null phase table. It must still hold its FIFO position and delay later
    /// work after any preceding busy sender has actually returned.
    pub fn push_work(&mut self, origin: u64, clocks: u16) -> Result<(), SendQueueError> {
        if self.length == N {
            return Err(SendQueueError::Full);
        }
        self.requests[(self.head + self.length) % N] =
            Some(CallerOperation::Work { origin, clocks });
        self.length += 1;
        Ok(())
    }
    pub fn pending(&self) -> usize {
        self.length
    }
    pub fn remaining_capacity(&self) -> usize {
        N - self.length
    }
    pub fn caller_available_clock(&self) -> u64 {
        self.available_clock
    }
    /// The readiness callback is evaluated only on original ready-poll
    /// boundaries. Bus writes and elapsed caller clocks remain ordered across
    /// arbitrary audio callback partitions.
    pub fn advance_until(
        &mut self,
        end_clock: u64,
        mut ready_port: impl FnMut(u64) -> u8,
        mut write: impl FnMut(u64, HpiAction),
    ) {
        while self.length != 0 {
            if let Some(CallerOperation::Work { origin, clocks }) = self.requests[self.head] {
                let end = self.available_clock.max(origin) + u64::from(clocks);
                if end > end_clock {
                    break;
                }
                self.available_clock = end;
                self.requests[self.head] = None;
                self.head = (self.head + 1) % N;
                self.length -= 1;
                continue;
            }
            let Some(CallerOperation::Send(spaced)) = self.requests[self.head] else {
                unreachable!()
            };
            if self.transfer.is_none() {
                let request = spaced.request;
                let origin =
                    self.available_clock.max(request.available_clock) + u64::from(spaced.before);
                self.transfer = TimedParameterTransfer::from_sender(
                    request.endpoint,
                    request.sender,
                    request.address,
                    request.value,
                    origin,
                );
            }
            let transfer = self.transfer.as_mut().unwrap();
            let clock = transfer.next_clock().unwrap();
            if clock > end_clock {
                break;
            }
            let port = if transfer.polling() {
                ready_port(clock)
            } else {
                0
            };
            if let Some(action) = transfer.advance(port) {
                write(clock, action);
            }
            if transfer.completed() {
                self.available_clock = clock + u64::from(spaced.after);
                self.transfer = None;
                self.requests[self.head] = None;
                self.head = (self.head + 1) % N;
                self.length -= 1;
            }
        }
    }
}
