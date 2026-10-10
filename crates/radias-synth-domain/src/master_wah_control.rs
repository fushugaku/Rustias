//! Master Wah coefficient actions using the shared original Wah arithmetic.
use crate::{
    effect_control::{EffectKind, EffectMix},
    effect_curves::EffectCurve,
    effect_parameters::{EffectInterpolationControl, EffectParameterBatch},
    effect_routing::EffectRoutingInstance,
    effect_setters::EffectSelectorWrites,
    master_effect_control::{MasterControlState, MasterControlTables, MasterEdit, publish},
};
impl MasterControlTables {
    pub(crate) fn prepare_wah_action(
        &self,
        state: &mut MasterControlState,
        batch: &mut EffectParameterBatch,
        edit: MasterEdit,
        index: usize,
        action: u8,
    ) -> Option<()> {
        let p = edit.parameters;
        let origin = u32::from(edit.origin);
        let control = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            edit.parameter,
            state.owner,
            0,
            true,
        );
        match action {
            25 => {
                let instance = EffectRoutingInstance {
                    kind: 5,
                    origin: edit.origin,
                    parameters: p,
                };
                if edit.direct_switch == 0 {
                    batch.push_direct(origin, 0x7fffff)?;
                    batch.push_direct(origin + 1, 0)?;
                    batch.extend(&self.flanger.routing.prepare_master(
                        edit.stored_effect_type,
                        &instance,
                        1,
                    )?)?;
                    batch.push_command(0, 0x01000014)?;
                }
                let mode = usize::from(edit.value);
                batch.push_direct(origin + 33, *self.wah.mode.get(mode)?)?;
                batch.push_direct(origin + 34, self.wah.frequency[usize::from(p[2] - 1)][mode])?;
                batch.push_direct(origin + 35, self.wah.bound(&p, edit.value, true)?)?;
                batch.push_direct(origin + 36, self.wah.resonance[usize::from(p[3] - 1)][mode])?;
                if edit.direct_switch == 0 && edit.stored_enabled {
                    batch.push_command(0, 0x01000028)?;
                    let mix = EffectMix::compile(EffectKind::new(5)?, p[0], Default::default())?;
                    batch.push_direct(origin, mix.dry as u32)?;
                    batch.push_direct(origin + 1, mix.wet as u32)?;
                    batch.extend(&self.flanger.routing.prepare_master(
                        edit.stored_effect_type,
                        &instance,
                        0,
                    )?)?;
                }
            }
            47 => {
                let writes =
                    EffectSelectorWrites::compile(edit.origin, index as u32, u32::from(edit.value));
                for word in &writes.words[..usize::from(writes.count)] {
                    batch.push_command(word.address, word.tagged_value)?;
                }
            }
            48 => {
                let value = if p[4] == 0 {
                    self.wah.ranges[6].compile(
                        EffectCurve::Linear,
                        i32::from(p[6]),
                        0x3200,
                        0x100,
                    )? as u32
                } else {
                    self.wah.response[usize::from(p[6])]
                };
                publish(state, batch, control, origin + 20, value, 0)?;
                publish(
                    state,
                    batch,
                    control,
                    origin + 24,
                    self.wah.response_complement[usize::from(p[6])],
                    0,
                )?;
            }
            49 => {
                publish(
                    state,
                    batch,
                    control,
                    origin + 51,
                    self.wah.frequency_mod(&p)?,
                    0,
                )?;
                batch.push_direct(
                    origin + 34,
                    self.wah.frequency[usize::from(p[2] - 1)][usize::from(p[1])],
                )?;
            }
            50 => {
                publish(
                    state,
                    batch,
                    control,
                    origin + 46,
                    self.wah.resonance_mod(&p)?,
                    0,
                )?;
                batch.push_direct(origin + 35, self.wah.bound(&p, p[1], false)?)?;
                batch.push_direct(
                    origin + 36,
                    self.wah.resonance[usize::from(p[3] - 1)][usize::from(p[1])],
                )?;
            }
            51 => {
                let owner = |parameter| {
                    if control.enabled_argument != 0 {
                        control
                    } else {
                        EffectInterpolationControl::from_owners(
                            edit.direct_switch,
                            parameter,
                            state.owner,
                            0,
                            true,
                        )
                    }
                };
                let first = owner(2);
                let second = owner(3);
                publish(
                    state,
                    batch,
                    first,
                    origin + 51,
                    self.wah.frequency_mod(&p)?,
                    0,
                )?;
                publish(
                    state,
                    batch,
                    second,
                    origin + 46,
                    self.wah.resonance_mod(&p)?,
                    0,
                )?;
                if (i32::from(edit.previous_parameters[5]) - 64) * (i32::from(p[5]) - 64) <= 0 {
                    batch.push_direct(origin + 35, self.wah.bound(&p, p[1], false)?)?;
                }
            }
            52 => {
                let bucket = if p[4] != 1 {
                    0
                } else if p[9] == 0 {
                    usize::from(self.wah.free_rate[usize::from(p[10])])
                } else {
                    let rate = u32::from(edit.clock.tempo).wrapping_mul(192)
                        / self.wah.sync_divisors[usize::from(p[11])];
                    (((u64::from(rate) * 0x1b4e81b5) >> 32) >> 8) as usize
                };
                let coefficients = self.wah.rate_coefficients.get(bucket)?;
                batch.push_direct(origin + 53, u32::from(coefficients[0]))?;
                batch.push_direct(origin + 54, u32::from(coefficients[1]))?;
            }
            53 => {
                let value = if p[4] == 1 || (p[4] == 2 && edit.polarity.bipolar(p[16])) {
                    if p[5] >= 64 { 0x7fffff } else { 0xff800001 }
                } else {
                    0
                };
                batch.push_direct(origin + 55, value)?;
            }
            _ => return None,
        }
        Some(())
    }
}
