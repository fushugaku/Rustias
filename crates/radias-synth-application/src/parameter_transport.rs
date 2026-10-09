//! Shared ordered sender/mailbox boundary for direct synthesis adapters.
//! Readiness and receiver scheduling are supplied explicitly by the owner.
use crate::dsp_transport::{HpiAction, ParameterSendQueue, ParameterSendRequest, SendQueueError};
use radias_synth_domain::dsp_control::DspEndpoint;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceivedParameterPacket {
    pub endpoint: DspEndpoint,
    words: [u16; 9],
    length: u8,
}
impl ReceivedParameterPacket {
    pub fn words(&self) -> &[u16] {
        &self.words[..usize::from(self.length)]
    }
}

pub struct OrderedParameterTransport<const N: usize> {
    queue: ParameterSendQueue<N>,
    mailboxes: [[u16; 9]; 2],
    cursors: [usize; 2],
}
impl<const N: usize> Default for OrderedParameterTransport<N> {
    fn default() -> Self {
        Self {
            queue: Default::default(),
            mailboxes: [[0; 9]; 2],
            cursors: [0; 2],
        }
    }
}
impl<const N: usize> OrderedParameterTransport<N> {
    pub fn enqueue(&mut self, request: ParameterSendRequest) -> Result<(), SendQueueError> {
        self.queue.push(request)
    }
    pub fn enqueue_with_spacing(
        &mut self,
        request: ParameterSendRequest,
        before: u16,
        after: u16,
    ) -> Result<(), SendQueueError> {
        self.queue.push_with_spacing(request, before, after)
    }
    pub fn remaining_capacity(&self) -> usize {
        self.queue.remaining_capacity()
    }
    pub fn enqueue_work(&mut self, origin: u64, clocks: u16) -> Result<(), SendQueueError> {
        self.queue.push_work(origin, clocks)
    }
    pub fn caller_available_clock(&self) -> u64 {
        self.queue.caller_available_clock()
    }
    pub fn pending(&self) -> usize {
        self.queue.pending()
    }
    /// Deliver a completed payload at the original HINT-ack write boundary.
    /// This does not imply the DSP has executed its receiver or a voice job.
    /// The owner supplies its ready port at every actual sender poll.
    pub fn advance_until(
        &mut self,
        end_clock: u64,
        ready_port: impl FnMut(u64) -> u8,
        mut receive: impl FnMut(u64, ReceivedParameterPacket),
    ) {
        let mailboxes = &mut self.mailboxes;
        let cursors = &mut self.cursors;
        self.queue
            .advance_until(end_clock, ready_port, |clock, action| {
                let endpoint = match action {
                    HpiAction::AddressByte { endpoint, .. }
                    | HpiAction::DataWord { endpoint, .. }
                    | HpiAction::AcknowledgeHint { endpoint, .. } => endpoint,
                };
                let chip = usize::from(endpoint == DspEndpoint::Slave);
                match action {
                    HpiAction::AddressByte { offset: 4, .. } => cursors[chip] = 0,
                    HpiAction::AddressByte { .. } => {}
                    HpiAction::DataWord { value, .. } => {
                        if cursors[chip] < 9 {
                            mailboxes[chip][cursors[chip]] = value;
                            cursors[chip] += 1;
                        }
                    }
                    HpiAction::AcknowledgeHint { .. } => {
                        receive(
                            clock,
                            ReceivedParameterPacket {
                                endpoint,
                                words: mailboxes[chip],
                                length: cursors[chip] as u8,
                            },
                        );
                    }
                }
            });
    }
}
