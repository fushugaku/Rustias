//! Original SYS07D2B2 gains and complete SYS07D31A route publications.
use crate::{
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_parameters::EffectParameterBatch,
    program::Program,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectRoutingProfile {
    pub parameter: u8,
    pub offset: u8,
    pub constant: u32,
    pub first: i32,
    pub second: i32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectRoutingInstance {
    /// Object type can differ from the stored program during transitions.
    pub kind: u8,
    pub origin: u16,
    pub parameters: [u8; 20],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectRoutingContext {
    Insert(u8),
    Master,
}
pub struct EffectRoutingTables {
    pub insert: [EffectRoutingProfile; 31],
    pub master: [EffectRoutingProfile; 31],
    pub resonance: [u16; 128],
    pub range: EffectParameterRange,
}
impl EffectRoutingTables {
    pub fn prepare_master_input_mute(
        &self,
        stored_kind: u8,
        instance: &EffectRoutingInstance,
    ) -> Option<EffectParameterBatch> {
        let mut batch = EffectParameterBatch::from_lfo(None);
        if stored_kind != 0 {
            batch.push_direct(
                u32::from(instance.origin)
                    + u32::from(self.master.get(usize::from(stored_kind))?.offset),
                0,
            )?;
        }
        Some(batch)
    }
    /// Master routing has no dependency on a timbre or the insert instances.
    pub fn prepare_master(
        &self,
        stored_kind: u8,
        instance: &EffectRoutingInstance,
        reset_argument: u32,
    ) -> Option<EffectParameterBatch> {
        let mut batch = EffectParameterBatch::from_lfo(None);
        if reset_argument == 0 {
            let profile = *self.master.get(usize::from(stored_kind))?;
            let base = if stored_kind == 0 {
                0x2f7
            } else {
                u32::from(instance.origin)
            };
            batch.push_direct(0x2fb, base + u32::from(profile.offset))?;
            batch.push_direct(0x2fa, self.gain(instance, profile)?)?;
            batch.push_command(0, 0x01000014)?;
        }
        for (address, value) in [(0x2fb, 0x2f7), (0x2fa, 0), (0x2fd, 0x2f7), (0x2fc, 0)] {
            batch.push_direct(address, value)?;
        }
        Some(batch)
    }
    /// SYS07D56E clears the selected effects' input-gain destinations.
    pub fn prepare_input_mute(
        &self,
        program: &Program,
        instances: &[EffectRoutingInstance; 9],
        context: EffectRoutingContext,
    ) -> Option<EffectParameterBatch> {
        if context == EffectRoutingContext::Master {
            return self
                .prepare_master_input_mute(program.master_effect().kind()?.raw(), &instances[8]);
        }
        let mut batch = EffectParameterBatch::from_lfo(None);
        match context {
            EffectRoutingContext::Insert(part) => {
                let timbre = program.timbre(usize::from(part))?;
                let first = timbre.effect(0)?.kind()?.raw();
                for role in 0..2 {
                    if role == 1 && matches!(first, 29 | 30) {
                        break;
                    }
                    let kind = if role == 0 {
                        first
                    } else {
                        timbre.effect(1)?.kind()?.raw()
                    };
                    if kind != 0 {
                        batch.push_direct(
                            u32::from(instances[usize::from(part) * 2 + role].origin)
                                + u32::from(self.insert[usize::from(kind)].offset),
                            0,
                        )?;
                    }
                }
            }
            EffectRoutingContext::Master => {
                return None;
            }
        }
        Some(batch)
    }
    pub fn gain(
        &self,
        instance: &EffectRoutingInstance,
        profile: EffectRoutingProfile,
    ) -> Option<u32> {
        if instance.kind >= 31 {
            return None;
        }
        if instance.kind == 4 {
            let coefficient =
                (u32::from(*self.resonance.get(usize::from(instance.parameters[3]))?) << 8) | 255;
            let value = u32::from(*instance.parameters.get(usize::from(profile.parameter))?);
            let product = coefficient.wrapping_mul(value);
            let high = ((u64::from(product) * 0x02040811) >> 32) as u32;
            // Original unsigned reciprocal division, including its narrowing.
            return Some(((product.wrapping_sub(high) >> 1).wrapping_add(high)) >> 6);
        }
        if profile.parameter == 0 {
            return Some(profile.constant);
        }
        self.range
            .compile(
                EffectCurve::Linear,
                i32::from(*instance.parameters.get(usize::from(profile.parameter))?),
                profile.first,
                profile.second,
            )
            .map(|value| value as u32)
    }

    pub fn prepare(
        &self,
        program: &Program,
        instances: &[EffectRoutingInstance; 9],
        context: EffectRoutingContext,
        reset_argument: u32,
    ) -> Option<EffectParameterBatch> {
        if context == EffectRoutingContext::Master {
            return self.prepare_master(
                program.master_effect().kind()?.raw(),
                &instances[8],
                reset_argument,
            );
        }
        let mut batch = EffectParameterBatch::from_lfo(None);
        if reset_argument == 0 {
            match context {
                EffectRoutingContext::Insert(part) => {
                    let timbre = program.timbre(usize::from(part))?;
                    let first_kind = timbre.effect(0)?.kind()?.raw();
                    let second_kind = if matches!(first_kind, 29 | 30) {
                        0
                    } else {
                        timbre.effect(1)?.kind()?.raw()
                    };
                    for (role, kind) in [first_kind, second_kind].into_iter().enumerate() {
                        let instance = &instances[usize::from(part) * 2 + role];
                        let profile = self.insert[usize::from(kind)];
                        let base = if kind == 0 {
                            0x2f7
                        } else {
                            u32::from(instance.origin)
                        };
                        batch.push_direct(
                            0x2fb + 2 * role as u32,
                            base + u32::from(profile.offset),
                        )?;
                        batch
                            .push_direct(0x2fa + 2 * role as u32, self.gain(instance, profile)?)?;
                    }
                }
                EffectRoutingContext::Master => {
                    return None;
                }
            }
            // Original normal publication falls through the delay/reset tail.
            batch.push_command(0, 0x01000014)?;
        }
        for (address, value) in [(0x2fb, 0x2f7), (0x2fa, 0), (0x2fd, 0x2f7), (0x2fc, 0)] {
            batch.push_direct(address, value)?;
        }
        Some(batch)
    }
}
