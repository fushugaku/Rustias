//! Lossless native drum-kit data and the SYS006A04 trigger-key rule.
pub const DRUM_KIT_BYTES: usize = 0x700;
pub const DRUM_INSTRUMENT_BYTES: usize = 104;
pub const DRUM_INSTRUMENT_COUNT: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrumKit {
    raw: [u8; DRUM_KIT_BYTES],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrumKitLength;

impl DrumKit {
    pub fn from_bytes(raw: &[u8]) -> Result<Self, DrumKitLength> {
        Ok(Self {
            raw: raw.try_into().map_err(|_| DrumKitLength)?,
        })
    }
    pub fn bytes(&self) -> &[u8; DRUM_KIT_BYTES] {
        &self.raw
    }
    pub fn name(&self) -> &[u8; 12] {
        self.raw[..12].try_into().unwrap()
    }
    pub fn instrument(&self, index: usize) -> Option<&[u8; DRUM_INSTRUMENT_BYTES]> {
        let start = 52usize.checked_add(index.checked_mul(DRUM_INSTRUMENT_BYTES)?)?;
        if index >= DRUM_INSTRUMENT_COUNT {
            return None;
        }
        Some(
            self.raw[start..start + DRUM_INSTRUMENT_BYTES]
                .try_into()
                .unwrap(),
        )
    }
    pub fn replace_instrument(
        &mut self,
        index: usize,
        body: &[u8; DRUM_INSTRUMENT_BYTES],
    ) -> Result<(), DrumKitLength> {
        if index >= DRUM_INSTRUMENT_COUNT {
            return Err(DrumKitLength);
        }
        let start = 52 + index * DRUM_INSTRUMENT_BYTES;
        self.raw[start..start + DRUM_INSTRUMENT_BYTES].copy_from_slice(body);
        Ok(())
    }
    pub fn exclusive_group(&self, index: usize) -> Option<u8> {
        (index < DRUM_INSTRUMENT_COUNT).then(|| self.raw[18 + index])
    }
    /// Preserve the librarian control word without assigning an unqualified
    /// note-off meaning. The original direct-note scene never reads this word.
    pub fn control_word(&self) -> u16 {
        u16::from_le_bytes([self.raw[34], self.raw[35]])
    }
    /// The original signed key plus masked transpose is compared before any
    /// folding. Duplicate assignments all dispatch, in instrument order.
    pub fn trigger_mask(&self, note: u8, transpose: u8) -> u16 {
        let transpose = (transpose & 127) as i32 - 64;
        let mut mask = 0;
        for index in 0..DRUM_INSTRUMENT_COUNT {
            if self.raw[36 + index] as i8 as i32 + transpose == (note & 127) as i32 {
                mask |= 1 << index;
            }
        }
        mask
    }
}

/// SYS006928 selects one of four timbres with the high three program bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrumProgram {
    pub selection: u8,
    pub kit: u8,
    pub timbre: Option<u8>,
    pub level: u8,
    pub pan: u8,
    pub transpose: u8,
}
impl DrumProgram {
    pub fn from_raw(selection: u8, level: u8, pan: u8, transpose: u8) -> Self {
        let selected = selection >> 5;
        Self {
            selection,
            kit: selection & 31,
            timbre: (1..=4).contains(&selected).then(|| selected - 1),
            level,
            pan,
            transpose,
        }
    }
}
