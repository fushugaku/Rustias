//! Complete SYS07991C Reverb Time publications for insert and master banks.
use crate::{effect_control::EffectBank, effect_parameters::EffectParameterBatch};
pub struct ReverbTimeTables {
    pub large_indices: [u8; 128],
    pub small_indices: [u8; 128],
    /// Three insert profiles, then master types 0/2, 1, 3, 4, 5.
    pub coefficients: [[[u32; 4]; 100]; 8],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReverbTimeEdit {
    pub bank: EffectBank,
    pub origin: u16,
    pub effect_type: u8,
    pub time: u8,
}
impl ReverbTimeTables {
    pub fn prepare(&self, edit: ReverbTimeEdit) -> Option<EffectParameterBatch> {
        let (profile, count) = match (edit.bank, edit.effect_type) {
            (EffectBank::Insert, 0..=2) => (usize::from(edit.effect_type), 3),
            (EffectBank::Master, 0 | 2) => (3, 4),
            (EffectBank::Master, 1) => (4, 4),
            (EffectBank::Master, 3) => (5, 4),
            (EffectBank::Master, 4) => (6, 4),
            (EffectBank::Master, 5) => (7, 4),
            _ => return None,
        };
        let mapping = if matches!(profile, 2 | 6 | 7) {
            &self.small_indices
        } else {
            &self.large_indices
        };
        let index = usize::from(*mapping.get(usize::from(edit.time))?);
        let coefficients = self.coefficients[profile].get(index)?;
        let mut batch = EffectParameterBatch::from_lfo(None);
        for (offset, &value) in coefficients[..count].iter().enumerate() {
            batch.push_direct(u32::from(edit.origin) + 36 + offset as u32, value)?;
        }
        Some(batch)
    }
}
