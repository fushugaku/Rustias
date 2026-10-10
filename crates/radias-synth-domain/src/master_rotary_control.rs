//! Master Rotary state transitions, including SYS079234's retained change flag.
use crate::{
    effect_parameters::EffectParameterBatch,
    master_effect_control::{
        MasterControlState, MasterControlTables, MasterEdit, PreparedMasterEdit,
    },
    rotary_effect::RotaryInstance,
};
impl MasterControlTables {
    pub fn prepare_rotary_midi(
        &self,
        state: &MasterControlState,
        edit: MasterEdit,
    ) -> Option<PreparedMasterEdit> {
        self.prepare_rotary_midi_refresh(state, edit, false)
    }
    pub fn prepare_rotary_midi_refresh(
        &self,
        state: &MasterControlState,
        edit: MasterEdit,
        force_refresh: bool,
    ) -> Option<PreparedMasterEdit> {
        if edit.kind != 29 {
            return None;
        }
        let definition = &self.definitions[29];
        for (&v, range) in edit.parameters[..19].iter().zip(definition.ranges) {
            let value = i32::from(v) - i32::from(range.encoded_zero);
            if !(i32::from(range.minimum)..=i32::from(range.maximum)).contains(&value) {
                return None;
            }
        }
        let mut next = *state;
        let mut batch = EffectParameterBatch::from_lfo(None);
        self.rotary_midi(&mut next, &mut batch, edit, force_refresh)?;
        Some(PreparedMasterEdit {
            next,
            batch,
            program_writes: [None; 2],
            body_program: None,
        })
    }
    fn rotary_speeds(
        &self,
        state: &MasterControlState,
        batch: &mut EffectParameterBatch,
        edit: MasterEdit,
        value: i8,
        force: bool,
    ) -> Option<()> {
        let instance = RotaryInstance {
            kind: 29,
            parameters: edit.parameters,
            origin: edit.origin,
            controller_source: state.midi_binding.source,
            primary: state.midi_binding.values[0],
            secondary: state.midi_binding.values[1],
            mode: state.rotary_mode,
            speed: state.rotary_speed,
        };
        for (i, word) in self
            .rotary
            .speeds(instance, value, force)
            .into_iter()
            .enumerate()
        {
            batch.push_direct(u32::from(edit.origin) + 29 + 2 * i as u32, word)?;
        }
        Some(())
    }
    fn rotary_midi(
        &self,
        state: &mut MasterControlState,
        batch: &mut EffectParameterBatch,
        edit: MasterEdit,
        force_refresh: bool,
    ) -> Option<()> {
        let p = edit.parameters;
        let mut changed = false;
        // Original R11 is retained between primary and secondary branches.
        let mut value = 0;
        if p[2] != 0 {
            value = edit.midi.value(4, p[2])?.abs();
            let old = state.midi_binding.values[0];
            if value != old || force_refresh {
                let transition = if p[3] == 0 {
                    value >= 64 && old < 64
                } else {
                    (value >= 64) != (old >= 64)
                };
                if transition {
                    state.rotary_mode = if p[3] == 0 {
                        u32::from(state.rotary_mode == 0)
                    } else {
                        u32::from(value >= 64)
                    };
                    changed = true;
                    for (i, word) in self
                        .rotary
                        .acceleration(&p, state.rotary_mode != 0)?
                        .into_iter()
                        .enumerate()
                    {
                        batch.push_direct(u32::from(edit.origin) + 30 + 2 * i as u32, word)?;
                    }
                    self.rotary_speeds(state, batch, edit, 0, true)?;
                }
                state.midi_binding.values[0] = value;
            }
        }
        if p[4] == 0 {
            if p[6] != 0 {
                value = edit.midi.value(4, p[6])?.abs();
                let old = state.midi_binding.values[1];
                if value != old || force_refresh {
                    let transition = if p[7] == 0 {
                        value >= 64 && old < 64
                    } else {
                        (value >= 64) != (old >= 64)
                    };
                    if transition {
                        state.rotary_speed = if p[7] == 0 {
                            u32::from(state.rotary_speed == 0)
                        } else {
                            u32::from(value >= 64)
                        };
                        changed = true;
                    }
                    state.midi_binding.values[1] = value;
                }
            }
        } else if p[9] != 0 {
            value = edit.midi.value(4, p[9])?.abs();
            if value != state.midi_binding.values[1] || force_refresh {
                state.midi_binding.values[1] = value;
                changed = true;
            }
        }
        if changed && state.rotary_mode == 0 {
            self.rotary_speeds(state, batch, edit, value, false)?;
        }
        Some(())
    }
    pub(crate) fn prepare_rotary_action(
        &self,
        state: &mut MasterControlState,
        batch: &mut EffectParameterBatch,
        edit: MasterEdit,
        action: u8,
    ) -> Option<()> {
        let p = edit.parameters;
        match action {
            63 => {
                if state.midi_binding.source != u32::from(edit.value.min(12)) {
                    state.midi_binding.source = u32::from(p[2]);
                    state.midi_binding.values = [
                        edit.midi.value(4, p[2])?.abs(),
                        edit.midi
                            .value(4, if p[4] == 0 { p[6] } else { p[9] })?
                            .abs(),
                    ];
                    self.rotary_midi(state, batch, edit, false)?;
                }
            }
            64 => {
                let mut value = edit.midi.value(4, p[9])?.abs();
                match edit.parameter {
                    1 => state.rotary_mode = u32::from(p[1]),
                    5 => state.rotary_speed = u32::from(p[5]),
                    8 => value = p[8] as i8,
                    4 | 10 => {}
                    _ => return Some(()),
                }
                self.rotary_speeds(state, batch, edit, value, false)?;
            }
            65 => {
                let words = self.rotary.acceleration(&p, p[1] == 0)?;
                if matches!(edit.parameter, 1 | 12) {
                    batch.push_direct(u32::from(edit.origin) + 30, words[0])?;
                }
                if matches!(edit.parameter, 1 | 14) {
                    batch.push_direct(u32::from(edit.origin) + 32, words[1])?;
                }
            }
            68 => {
                let spread = u32::from(p[17]);
                let (a, b) = if spread < 64 {
                    (0x550000 + spread * 0xabff, (64 - spread) * 0x1540)
                } else {
                    (0x7fffff, 0)
                };
                for (i, v) in [a, b, b, a].into_iter().enumerate() {
                    batch.push_direct(u32::from(edit.origin) + 54 + i as u32, v)?;
                }
            }
            _ => return None,
        }
        Some(())
    }
}
