//! Bounded, transactional parameter publications shared by effect controllers.
use crate::effect_lfo_program::EffectLfoPublication;
use crate::effect_updates::{
    CoefficientChangePlan, CoefficientQueueWord, EffectCoefficientAssignments,
};
#[derive(Clone, Copy)]
pub struct EffectCoefficientGroup {
    /// Original R5 is the endpoint stored second in the twelve-byte record.
    pub first: i32,
    pub second: i32,
    pub action: u8,
    pub range: crate::effect_curves::EffectParameterRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectParameterBatch {
    words: [CoefficientQueueWord; 128],
    count: u8,
    lfo: Option<EffectLfoPublication>,
}
impl EffectParameterBatch {
    pub fn words(&self) -> &[CoefficientQueueWord] {
        &self.words[..usize::from(self.count)]
    }
    pub fn lfo_publication(&self) -> Option<EffectLfoPublication> {
        self.lfo
    }
    pub fn from_lfo(publication: Option<EffectLfoPublication>) -> Self {
        Self {
            words: [CoefficientQueueWord::default(); 128],
            count: 0,
            lfo: publication,
        }
    }
    pub(crate) fn push_direct(&mut self, target: u32, value: u32) -> Option<()> {
        *self.words.get_mut(usize::from(self.count))? = CoefficientQueueWord {
            address: target as u16,
            tagged_value: value & 0xffffff,
        };
        self.count += 1;
        Some(())
    }
    pub(crate) fn append(&mut self, plan: &CoefficientChangePlan) -> Option<()> {
        let start = usize::from(self.count);
        let count = usize::from(plan.count);
        self.words
            .get_mut(start..start + count)?
            .copy_from_slice(&plan.entries[..count]);
        self.count += plan.count;
        Some(())
    }
    /// Keep original non-coefficient queue tags, including delay commands.
    pub(crate) fn push_command(&mut self, address: u16, tagged_value: u32) -> Option<()> {
        *self.words.get_mut(usize::from(self.count))? = CoefficientQueueWord {
            address,
            tagged_value,
        };
        self.count += 1;
        Some(())
    }
    pub(crate) fn extend(&mut self, other: &Self) -> Option<()> {
        if self.lfo.is_some() && other.lfo.is_some() {
            return None;
        }
        let start = usize::from(self.count);
        self.words
            .get_mut(start..start + other.words().len())?
            .copy_from_slice(other.words());
        self.count += other.count;
        self.lfo = self.lfo.or(other.lfo);
        Some(())
    }
}
pub struct PreparedEffectParameterChange {
    pub next: EffectCoefficientAssignments,
    pub batch: EffectParameterBatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectInterpolationControl {
    pub direct_switch: u32,
    pub enabled_argument: u32,
}
impl EffectInterpolationControl {
    /// Original SYS0755DC owner selection; parameter zero uses direct updates.
    pub fn from_owners(
        direct_switch: u32,
        parameter: u8,
        first: u32,
        second: u32,
        master: bool,
    ) -> Self {
        let target = u32::from(parameter);
        let enabled_argument = if target == 0 {
            0
        } else if master {
            if first == target { 3 } else { 0 }
        } else if first == target {
            1
        } else if second == target {
            2
        } else {
            0
        };
        Self {
            direct_switch,
            enabled_argument,
        }
    }
}
