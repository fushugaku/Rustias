//! Instrument Unison groups, distinct from OSC1's five-phase Unison mode.
use crate::lfo::LfoState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceGroupProgram {
    /// Original common byte8: bit7 enabled, low nibble encodes count minus2.
    pub raw: u8,
    pub detune: u8,
    pub spread: u8,
}
impl Default for VoiceGroupProgram {
    fn default() -> Self {
        Self {
            raw: 4,
            detune: 0,
            spread: 0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupLayout {
    pub count: u8,
    pub bank: u8,
    pub stereo_pair: bool,
}
impl VoiceGroupProgram {
    pub fn layout(self, primary: u8) -> GroupLayout {
        if self.raw & 128 == 0 {
            return GroupLayout {
                count: 1,
                bank: 0,
                stereo_pair: false,
            };
        }
        let stereo_pair = primary & 63 == 8;
        GroupLayout {
            count: if stereo_pair {
                2
            } else {
                ((self.raw & 15) + 2).min(8)
            },
            bank: if stereo_pair { 1 } else { (self.raw & 15) + 1 },
            stereo_pair,
        }
    }
}
#[derive(Clone, Copy)]
pub struct VoiceGroupTables {
    pub detune: [[i16; 8]; 8],
    pub pan: [[i16; 8]; 8],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupOffsets {
    pub tuning_q16: i32,
    pub pan: i8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceGroupSlots {
    pub timbres: [u8; 24],
    pub indices: [u8; 24],
    pub stereo: [u8; 24],
}
impl Default for VoiceGroupSlots {
    fn default() -> Self {
        Self {
            timbres: [255; 24],
            indices: [0; 24],
            stereo: [0; 24],
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupAssignment {
    pub selected: u32,
    pub displaced: u32,
    pub bank: u8,
}
impl VoiceGroupTables {
    /// Original01ED70/01EDD4: zero coefficients consume no random word.
    /// Detune reads a signed byte; spread masks bit7 before multiplying.
    pub fn offsets(
        &self,
        program: VoiceGroupProgram,
        bank: u8,
        index: u8,
        seed: &mut u16,
    ) -> Option<GroupOffsets> {
        Some(GroupOffsets {
            tuning_q16: self.detune_offset(program.detune, bank, index, seed)?,
            pan: self.pan_offset(program.spread, bank, index)?,
        })
    }
    pub fn detune_offset(&self, amount: u8, bank: u8, index: u8, seed: &mut u16) -> Option<i32> {
        let detune = *self.detune.get(bank as usize)?.get(index as usize)?;
        Some(if detune == 0 {
            0
        } else {
            let random = LfoState::next_random(seed) as i32;
            let jitter = (random.wrapping_mul(65).wrapping_shl(1) >> 16) as i16;
            (detune as i32)
                .wrapping_add(jitter as i32)
                .wrapping_mul(amount as i8 as i32)
        })
    }
    pub fn pan_offset(&self, amount: u8, bank: u8, index: u8) -> Option<i8> {
        let pan = *self.pan.get(bank as usize)?.get(index as usize)?;
        Some((((pan as i32) * (amount & 127) as i32) >> 8) as i8)
    }
}
