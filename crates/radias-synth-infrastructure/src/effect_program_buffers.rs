//! Mutable native program buffers; selectors and offsets come from the domain.
use radias_synth_application::effect_transition_queue::EffectProgramSource;
use radias_synth_domain::effect_transition_queue::EffectProgramBufferLayout;

#[derive(Clone)]
pub struct EffectProgramBuffers {
    layout: EffectProgramBufferLayout,
    buffers: [Vec<u8>; 3],
    decoded: [Vec<u64>; 87],
}
impl EffectProgramBuffers {
    pub fn buffer_bytes(&self, index: usize) -> Option<&[u8]> {
        self.buffers.get(index).map(Vec::as_slice)
    }
    pub fn from_buffers(
        layout: EffectProgramBufferLayout,
        buffers: [Vec<u8>; 3],
    ) -> Result<Self, &'static str> {
        let mut result = Self {
            layout,
            buffers,
            decoded: core::array::from_fn(|_| Vec::new()),
        };
        result.refresh()?;
        Ok(result)
    }
    fn location(&self, selector: u8) -> Option<(usize, usize, usize)> {
        let location = self.layout.locate(selector)?;
        let index = if selector < 60 {
            0
        } else if selector < 84 {
            1
        } else {
            2
        };
        let base = [
            self.layout.normal,
            self.layout.selected_insert,
            self.layout.selected_master,
        ][index];
        Some((
            index,
            location.word_address.wrapping_sub(base) as usize,
            location.count_address.wrapping_sub(base) as usize,
        ))
    }
    fn refresh(&mut self) -> Result<(), &'static str> {
        for selector in 0..87 {
            let (index, offset, count_offset) = self.location(selector).unwrap();
            let buffer = &self.buffers[index];
            let count = buffer
                .get(count_offset..count_offset + 2)
                .ok_or("Truncated effect program count")?;
            let count = usize::from(u16::from_be_bytes(count.try_into().unwrap()));
            let raw = buffer
                .get(offset..offset + count * 6)
                .ok_or("Effect program outside supplied buffer")?;
            self.decoded[usize::from(selector)] = raw
                .chunks_exact(6)
                .map(|b| b.iter().fold(0u64, |v, x| (v << 8) | u64::from(*x)))
                .collect();
        }
        Ok(())
    }
    /// Copy prepared words and their count together. Rejection leaves every
    /// selector intact; aliased prefix/body/tail views refresh after the write.
    pub fn store_program(&mut self, selector: u8, words: &[u64]) -> Result<(), &'static str> {
        let (index, offset, count_offset) = self
            .location(selector)
            .ok_or("Undefined effect program selector")?;
        let count = u16::try_from(words.len()).map_err(|_| "Effect program too long")?;
        if offset + words.len() * 6 > self.buffers[index].len()
            || count_offset + 2 > self.buffers[index].len()
        {
            return Err("Effect program outside supplied buffer");
        }
        let mut buffers = self.buffers.clone();
        for (bytes, word) in buffers[index][offset..offset + words.len() * 6]
            .chunks_exact_mut(6)
            .zip(words)
        {
            bytes.copy_from_slice(&word.to_be_bytes()[2..]);
        }
        buffers[index][count_offset..count_offset + 2].copy_from_slice(&count.to_be_bytes());
        *self = Self::from_buffers(self.layout, buffers)?;
        Ok(())
    }
}
impl EffectProgramSource for EffectProgramBuffers {
    fn program_words(&self, selector: u8) -> Option<&[u64]> {
        self.decoded.get(usize::from(selector)).map(Vec::as_slice)
    }
}
