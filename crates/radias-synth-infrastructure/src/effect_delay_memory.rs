//! Owns FX delay cells; allocation belongs to instrument preparation.
use radias_synth_domain::effect_delay_memory::{EffectDelayMemory, MemorySizeError, WORD_COUNT};

pub struct EffectDelayStorage {
    cells: Box<[u16]>,
}

impl EffectDelayStorage {
    /// Deterministic power-on profile. Actual SRAM power-on data is unknown.
    pub fn zeroed() -> Self {
        Self {
            cells: vec![0; WORD_COUNT].into_boxed_slice(),
        }
    }

    /// Imports retained words without reallocating, normalizing or clearing.
    pub fn from_cells(cells: Box<[u16]>) -> Result<Self, MemorySizeError> {
        if cells.len() != WORD_COUNT {
            return Err(MemorySizeError {
                actual_words: cells.len(),
            });
        }
        Ok(Self { cells })
    }

    /// The borrowed domain object performs sample access without allocation.
    pub fn samples(&mut self) -> EffectDelayMemory<'_> {
        EffectDelayMemory::new(&mut self.cells).expect("fixed FX delay storage geometry")
    }

    pub fn cells(&self) -> &[u16] {
        &self.cells
    }
}
