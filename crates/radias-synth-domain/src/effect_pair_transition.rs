//! Complete SYS07CCB2 pair program transition coefficients.
use crate::{effect_parameters::EffectParameterBatch, effect_routing::EffectRoutingInstance};

pub struct EffectPairTransitionTables {
    pub offsets: [[u8; 2]; 31],
    pub second_active: [bool; 31],
}
impl EffectPairTransitionTables {
    pub fn prepare(
        &self,
        instances: &[EffectRoutingInstance; 2],
        mute_argument: u32,
    ) -> Option<EffectParameterBatch> {
        let mut batch = EffectParameterBatch::from_lfo(None);
        let first = usize::from(instances[0].kind);
        let second = usize::from(instances[1].kind);
        let offsets = [*self.offsets.get(first)?, *self.offsets.get(second)?];
        let values = if mute_argument == 0 {
            [0x7f150f, 0xeaf0]
        } else {
            [0, 0x7fffff]
        };
        for (role, instance) in instances.iter().enumerate() {
            if role == 1 && !self.second_active[first] {
                break;
            }
            // The second offset is sent first. Keep source order when offsets
            // coincide or the narrowed host destination wraps.
            batch.push_direct(
                u32::from(instance.origin) + u32::from(offsets[role][1]),
                values[0],
            )?;
            batch.push_direct(
                u32::from(instance.origin) + u32::from(offsets[role][0]),
                values[1],
            )?;
        }
        Some(batch)
    }
}
