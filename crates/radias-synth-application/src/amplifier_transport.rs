//! AMP delivery through the ordered native HPI sender. Other synthesis packet
//! families and receiver hot-loop/DMA scheduling remain separate ports.
use crate::dsp_transport::{ParameterSendRequest, SendQueueError};
use crate::parameter_transport::OrderedParameterTransport;
use radias_synth_domain::{amplifier_delivery::AmplifierPacket, dsp_control::DspEndpoint};

#[derive(Default)]
pub struct AmplifierTransport {
    queue: OrderedParameterTransport<48>,
}
impl AmplifierTransport {
    pub fn enqueue(
        &mut self,
        clock: u64,
        slot: usize,
        packet: AmplifierPacket,
    ) -> Result<(), SendQueueError> {
        if slot >= 24 {
            return Err(SendQueueError::InvalidSlot);
        }
        let endpoint = if slot < 12 {
            DspEndpoint::Master
        } else {
            DspEndpoint::Slave
        };
        let base = 0x2000 + 160 * (slot % 12) as u32;
        let (sender, address, value) = match packet {
            AmplifierPacket::RateAndTarget { rate, target } => (
                19,
                base + 124,
                (u32::from(rate) << 16) | u32::from(target as u16),
            ),
            AmplifierPacket::Target(target) => (0, base + 125, u32::from(target as u16)),
        };
        self.queue.enqueue(ParameterSendRequest {
            endpoint,
            sender,
            address,
            value,
            available_clock: clock,
        })
    }
    pub fn pending(&self) -> usize {
        self.queue.pending()
    }
    /// Direct arithmetic makes the narrow AMP receive handler atomic here.
    /// This preserves sender clock ordering but does not qualify receiver
    /// hot-loop/voice/DMA preemption or complete instrument audio timing.
    pub fn advance_until(
        &mut self,
        end_clock: u64,
        mut commit: impl FnMut(u64, usize, AmplifierPacket),
    ) {
        self.queue.advance_until(
            end_clock,
            |_| 0,
            |clock, received| {
                let words = received.words();
                if words.len() < 5 || words[0] != 6 || words[2] != 0 {
                    return;
                }
                let chip = usize::from(received.endpoint == DspEndpoint::Slave);
                let address = usize::from(words[3]);
                if address < 0x2000 {
                    return;
                }
                let slot = (address - 0x2000) / 160;
                if slot >= 12 {
                    return;
                }
                let offset = (address - 0x2000) % 160;
                let packet = match (words[1], offset, words.len()) {
                    (22, 124, 6) => AmplifierPacket::RateAndTarget {
                        rate: words[4],
                        target: words[5] as i16,
                    },
                    (0, 125, 5) => AmplifierPacket::Target(words[4] as i16),
                    _ => return,
                };
                commit(clock, slot + chip * 12, packet);
            },
        );
    }
}
