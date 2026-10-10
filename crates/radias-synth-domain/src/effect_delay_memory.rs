//! FXD03's populated delay-memory data path, from KOD-A30667 IC31.
//!
//! MA17..0 address 256K sixteen-bit cells; MD23..8 are connected and
//! MD7..0 are grounded. This is storage wiring, not an FXD instruction
//! decoder, address-generator model or a claim about ASIC access timing.
use crate::Sample;

pub const WORD_COUNT: usize = 0x40000;
const ADDRESS_MASK: u32 = (WORD_COUNT - 1) as u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemorySizeError {
    pub actual_words: usize,
}

/// Borrows retained cells. Construction does not clear or allocate storage.
/// ASIC RESET/RESPLL do not erase this separately powered memory.
pub struct EffectDelayMemory<'a> {
    cells: &'a mut [u16],
}

impl<'a> EffectDelayMemory<'a> {
    pub fn new(cells: &'a mut [u16]) -> Result<Self, MemorySizeError> {
        if cells.len() != WORD_COUNT {
            return Err(MemorySizeError {
                actual_words: cells.len(),
            });
        }
        Ok(Self { cells })
    }

    /// Raw, unsigned 24-bit bus word. Upper address lines do not select cells.
    pub fn read(&self, address: u32) -> u32 {
        u32::from(self.cells[(address & ADDRESS_MASK) as usize]) << 8
    }

    /// Only the sixteen connected data lines reach the stored cell.
    pub fn write(&mut self, address: u32, value: u32) {
        self.cells[(address & ADDRESS_MASK) as usize] = (value >> 8) as u16;
    }

    /// Explicit conversion of a left-aligned sample port to the 24-bit bus.
    pub fn write_left_aligned(&mut self, address: u32, value: Sample) {
        self.write(address, (value.0 as u32) >> 8);
    }

    pub fn read_left_aligned(&self, address: u32) -> Sample {
        Sample((self.read(address) << 8) as i32)
    }

    pub fn cells(&self) -> &[u16] {
        self.cells
    }
}
