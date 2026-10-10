//! Active FX queue's original scalar/packed/delay service, SYS01D7E8.
//! The external tick is the firmware timer value; its physical clock is separate.
use crate::effect_updates::CoefficientQueueWord;
pub const EFFECT_QUEUE_CAPACITY: usize = 2048;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectHostPacket {
    pub address: u16,
    pub values: [u32; 7],
    pub count: u8,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectHostBatch {
    pub packets: [EffectHostPacket; 4],
    pub count: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectQueueError {
    Full,
    UnsupportedCommand,
    IncompletePacket,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectCommandQueueState {
    pub write_index: u16,
    pub read_index: u16,
    pub count: u16,
    pub wait_ticks: u16,
    pub wait_started: u16,
}
pub struct EffectCommandQueue {
    words: [CoefficientQueueWord; EFFECT_QUEUE_CAPACITY],
    write_index: u16,
    read_index: u16,
    count: u16,
    wait_ticks: u16,
    wait_started: u16,
}
impl Default for EffectCommandQueue {
    fn default() -> Self {
        Self {
            words: [CoefficientQueueWord::default(); EFFECT_QUEUE_CAPACITY],
            write_index: 0,
            read_index: 0,
            count: 0,
            wait_ticks: 0,
            wait_started: 0,
        }
    }
}
impl EffectCommandQueue {
    pub fn state(&self) -> EffectCommandQueueState {
        EffectCommandQueueState {
            write_index: self.write_index,
            read_index: self.read_index,
            count: self.count,
            wait_ticks: self.wait_ticks,
            wait_started: self.wait_started,
        }
    }
    /// A complete generated parameter transaction is accepted or left untouched.
    pub fn enqueue_words(
        &mut self,
        words: &[CoefficientQueueWord],
    ) -> Result<(), EffectQueueError> {
        if words.len() > EFFECT_QUEUE_CAPACITY - usize::from(self.count) {
            return Err(EffectQueueError::Full);
        }
        let mut cursor = 0;
        while cursor < words.len() {
            let tag = words[cursor].tagged_value >> 24;
            if tag & 0x80 != 0 {
                let count = (tag & 7).max(1) as usize;
                if cursor + count > words.len() {
                    return Err(EffectQueueError::IncompletePacket);
                }
                cursor += count;
            } else {
                if tag > 1 {
                    return Err(EffectQueueError::UnsupportedCommand);
                }
                cursor += 1;
            }
        }
        for &word in words {
            self.words[usize::from(self.write_index)] = word;
            self.write_index = (self.write_index + 1) & 2047;
        }
        self.count += words.len() as u16;
        Ok(())
    }
    fn pop(&mut self) -> CoefficientQueueWord {
        let word = self.words[usize::from(self.read_index)];
        self.read_index = (self.read_index + 1) & 2047;
        self.count -= 1;
        word
    }
    /// A packed packet and a delay each end the service call. Up to four scalar
    /// writes can share one call. Timer expiration is checked before host busy.
    pub fn service(&mut self, tick: u16, host_blocked: bool) -> EffectHostBatch {
        let mut output = EffectHostBatch::default();
        if self.wait_ticks != 0 {
            if tick.wrapping_sub(self.wait_started) < self.wait_ticks {
                return output;
            }
            self.wait_ticks = 0;
        }
        if host_blocked {
            return output;
        }
        while self.count != 0 && output.count < 4 {
            let word = self.pop();
            let tag = word.tagged_value >> 24;
            if tag == 1 {
                self.wait_ticks = word.tagged_value as u16;
                self.wait_started = tick;
                return output;
            }
            let count = if tag & 0x80 != 0 {
                (tag & 7).max(1) as u8
            } else {
                1
            };
            let mut packet = EffectHostPacket {
                address: word.address,
                values: [0; 7],
                count,
            };
            packet.values[0] = word.tagged_value & 0xffffff;
            for i in 1..usize::from(count) {
                packet.values[i] = self.pop().tagged_value & 0xffffff;
            }
            output.packets[usize::from(output.count)] = packet;
            output.count += 1;
            if tag & 0x80 != 0 {
                return output;
            }
        }
        output
    }
}
