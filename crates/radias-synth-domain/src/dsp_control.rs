//! Original SYS00F2A0..00F3B8 parameter packets and HPI ready/commit boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DspEndpoint {
    Master,
    Slave,
}
impl DspEndpoint {
    /// SYS00E6F4 samples PTFDR; low means the selected DSP is ready.
    pub fn busy(self, port: u8) -> bool {
        port & match self {
            Self::Master => 1,
            Self::Slave => 2,
        } != 0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterPacket {
    words: [u16; 9],
    length: u8,
}
impl ParameterPacket {
    /// SYS00ed3c copies160 DSP words from a stored timbre/drum parameter
    /// template to the assigned actor bank. This is a four-word payload.
    pub const ACTOR_COPY_SENDER: u8 = 21;
    /// SYS00f450 publishes the Pickup pitch shadow before Parallel priming.
    pub const PICKUP_PRIME_SENDER: u8 = 22;
    /// SYS00f40c transfers the retiring actor's working state to its frame.
    pub const DETACH_ACTOR_SENDER: u8 = 23;
    /// Whole SYS00f21c with count1, used by SYS01c8d8 to publish both
    /// Comb delay pointers in one original opcode1 packet.
    pub const COMB_POINTERS_SENDER: u8 = 24;
    /// SYS00f23a count0, with the initial Filter1 normalization word.
    pub const INITIAL_FILTER1_IMMEDIATE_SENDER: u8 = 25;
    pub const INITIAL_FILTER1_TIMED_SENDER: u8 = 26;
    pub fn copy_actor(source: u16, destination: u16) -> Self {
        Self {
            words: [6, 9, source, destination, 0, 0, 0, 0, 0],
            length: 4,
        }
    }
    pub fn word(opcode: u16, address: u32, value: u32) -> Self {
        Self {
            words: [6, opcode, 0, address as u16, value as u16, 0, 0, 0, 0],
            length: 5,
        }
    }
    pub fn long(opcode: u16, address: u32, value: u32) -> Self {
        Self {
            words: [
                6,
                opcode,
                0,
                address as u16,
                (value >> 16) as u16,
                value as u16,
                0,
                0,
                0,
            ],
            length: 6,
        }
    }
    pub fn words(&self) -> &[u16] {
        &self.words[..self.length as usize]
    }
    /// Original descriptor entry ordinal, used for comparison and typed adapters.
    pub fn from_sender(entry: u8, address: u32, value: u32) -> Option<Self> {
        if matches!(
            entry,
            Self::INITIAL_FILTER1_IMMEDIATE_SENDER | Self::INITIAL_FILTER1_TIMED_SENDER
        ) {
            return Some(Self {
                words: [
                    6,
                    21,
                    0,
                    address as u16,
                    (value >> 16) as u16,
                    value as u16,
                    if entry == Self::INITIAL_FILTER1_IMMEDIATE_SENDER {
                        0x5785
                    } else {
                        0x216
                    },
                    0,
                    0,
                ],
                length: 7,
            });
        }
        if entry == Self::ACTOR_COPY_SENDER {
            return Some(Self::copy_actor(value as u16, address as u16));
        }
        if entry == Self::PICKUP_PRIME_SENDER {
            return Some(Self {
                words: [6, 39, 0, address as u16, 0, 0, 0, 0, 0],
                length: 4,
            });
        }
        if entry == Self::DETACH_ACTOR_SENDER {
            return Some(Self {
                words: [6, 38, value as u16, address as u16, 0, 0, 0, 0, 0],
                length: 4,
            });
        }
        if entry == Self::COMB_POINTERS_SENDER {
            let controller_slot = (value >> 8) as u8;
            if controller_slot >= 24 {
                return None;
            }
            let local = if controller_slot >= 12 {
                controller_slot - 12
            } else {
                controller_slot
            };
            let pair = (u32::from(local) << 1) | (value & 1);
            let delay = 0x8000 + (pair << 12);
            return Some(Self {
                words: [
                    6,
                    1,
                    1,
                    address as u16,
                    ((pair ^ 1) << 2) as u16,
                    (pair << 2) as u16,
                    (address as u16).wrapping_add(5),
                    (delay >> 16) as u16,
                    delay as u16,
                ],
                length: 9,
            });
        }
        const WORD: [u16; 14] = [0, 16, 24, 25, 29, 30, 24, 26, 17, 27, 31, 34, 41, 18];
        const LONG: [u16; 7] = [1, 32, 28, 19, 20, 22, 23];
        if let Some(opcode) = WORD.get(entry as usize) {
            Some(Self::word(*opcode, address, value))
        } else {
            LONG.get((entry as usize).checked_sub(14)?)
                .map(|opcode| Self::long(*opcode, address, value))
        }
    }
    /// SYS00EAFC acknowledges HINT after the complete packet has been written.
    /// The ordinary software receive loop observes this handshake; this write
    /// does not raise DSPINT(the separate HPIC bit2 command).
    pub const COMMIT: u16 = 4;
}
