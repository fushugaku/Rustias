//! Native coefficient writes from the original SYS076770 selector setter.
//! No arithmetic or signal-source meaning is inferred for the ASIC operands.
use crate::effect_updates::CoefficientQueueWord;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectSelectorWrites {
    pub words: [CoefficientQueueWord; 3],
    pub count: u8,
}
impl EffectSelectorWrites {
    pub fn compile(origin: u16, offset: u32, selector: u32) -> Self {
        let base = origin.wrapping_add(offset as u16);
        let mut result = Self {
            words: [CoefficientQueueWord::default(); 3],
            count: 0,
        };
        let values = match selector {
            0 => [0x7fffff, 0, 0],
            1 => [0, 0x7fffff, 0],
            2 => [0, 0, 0x7fffff],
            _ => return result,
        };
        for (i, offset) in [0u16, 2, 1].into_iter().enumerate() {
            result.words[i] = CoefficientQueueWord {
                address: base.wrapping_add(offset),
                tagged_value: values[i],
            };
        }
        result.count = 3;
        result
    }
}
