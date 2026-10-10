//! Original twenty-slot prefix/tail staging state used during FX transitions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectProgramStaging {
    pub cursor: u8,
    pub prefix: [u64; 20],
    pub tail: [u64; 20],
    pub counts: [[u16; 2]; 20],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectStagedProgramWrite {
    pub selector: u8,
    pub word: u64,
}
impl EffectProgramStaging {
    pub fn program_words(&self, selector: u8) -> Option<&[u64]> {
        let slot = usize::from(selector / 3);
        let (word, count) = match selector % 3 {
            0 => (self.prefix.get(slot)?, *self.counts.get(slot)?.first()?),
            2 => (self.tail.get(slot)?, self.counts.get(slot)?[1]),
            _ => return None,
        };
        match count {
            0 => Some(&[]),
            1 => Some(core::slice::from_ref(word)),
            _ => None,
        }
    }
    pub(crate) fn store(&mut self, word: u64, tail: bool) -> Option<EffectStagedProgramWrite> {
        let slot = usize::from(self.cursor);
        let target = if tail {
            self.tail.get_mut(slot)?
        } else {
            self.prefix.get_mut(slot)?
        };
        *target = word & 0xffffffffffff;
        self.counts[slot][usize::from(tail)] = 1;
        Some(EffectStagedProgramWrite {
            selector: 3 * self.cursor + if tail { 2 } else { 0 },
            word: *target,
        })
    }
    pub(crate) fn advance(&mut self) {
        self.cursor = (self.cursor + 1) % 20;
    }
}
