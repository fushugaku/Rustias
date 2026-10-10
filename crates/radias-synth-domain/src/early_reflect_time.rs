//! Original sixteen Early Reflect tap addresses, complete SYS07A100.
use crate::{delay_time::divide_1000_unsigned, effect_parameters::EffectParameterBatch};
pub struct EarlyReflectTimeTables {
    pub pre_delay: [u8; 128],
    pub size: [u16; 128],
    pub tap_time: [u16; 16],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EarlyReflectTimeEdit {
    pub origin: u16,
    pub buffer_origin: u32,
    pub size: u8,
    pub pre_delay: u8,
}
impl EarlyReflectTimeTables {
    pub fn prepare(&self, edit: EarlyReflectTimeEdit) -> Option<EffectParameterBatch> {
        let size = u32::from(*self.size.get(usize::from(edit.size))?);
        let pre_delay = u32::from(*self.pre_delay.get(usize::from(edit.pre_delay))?);
        let base = edit.buffer_origin.wrapping_add(1);
        let start = u32::from(edit.origin.wrapping_add(34));
        let mut batch = EffectParameterBatch::from_lfo(None);
        for (i, &tap) in self.tap_time.iter().enumerate() {
            let time =
                divide_1000_unsigned(u32::from(tap).wrapping_mul(size)).wrapping_add(pre_delay);
            batch.push_direct(start + i as u32, time.wrapping_mul(48).wrapping_add(base))?;
        }
        Some(batch)
    }
}
