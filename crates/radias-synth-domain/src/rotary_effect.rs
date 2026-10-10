//! Complete Rotary Speaker insert dependency and MIDI state controllers.
use crate::{
    effect_buffer_allocation::divide_127,
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_midi::EffectMidiSources,
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
};
pub struct RotaryTables {
    pub ranges: [EffectParameterRange; 19],
    pub dependencies: [[u32; 2]; 19],
    pub groups: [EffectCoefficientGroup; 69],
    pub acceleration_range: EffectParameterRange,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RotaryInstance {
    pub kind: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub controller_source: u32,
    pub primary: i8,
    pub secondary: i8,
    pub mode: u32,
    pub speed: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RotaryRack {
    pub instances: [RotaryInstance; 8],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct RotaryEdit {
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
}
pub struct PreparedRotaryEdit {
    pub next: RotaryRack,
    pub batch: EffectParameterBatch,
}
fn direct(batch: &mut EffectParameterBatch, origin: u16, index: u32, value: u32) -> Option<()> {
    batch.push_direct(u32::from(origin) + index, value)
}
fn high_signed(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 32) as i32
}
fn divide_63(value: i32) -> i32 {
    let v = high_signed(value, 0x82082083u32 as i32).wrapping_add(value) >> 5;
    v.wrapping_add(i32::from(v < 0))
}
fn rate(p: &[u8; 20], control: i8, rotor: bool) -> u32 {
    let span = if rotor { 886 } else { 888 };
    let base = i32::from(p[8] as i8).wrapping_mul(1000);
    let modulated = i32::from(control)
        .wrapping_mul(span)
        .wrapping_mul(i32::from(p[10].wrapping_sub(64) as i8));
    let modulated = divide_63(divide_127(modulated));
    let value = divide_127(base)
        .wrapping_add(modulated)
        .wrapping_add(if rotor { 114 } else { 112 })
        .clamp(if rotor { 114 } else { 112 }, 1000);
    (value as u32).wrapping_mul(0x20c4)
}
impl RotaryTables {
    pub(crate) fn acceleration(&self, p: &[u8; 20], mode: bool) -> Option<[u32; 2]> {
        let ends = if mode {
            [[896, 416], [184, 64]]
        } else {
            [[288, 96], [72, 32]]
        };
        let compile = |i: usize| {
            self.acceleration_range
                .compile(
                    EffectCurve::Linear,
                    i32::from(p[12 + 2 * i]),
                    ends[i][0],
                    ends[i][1],
                )
                .map(|v| v as u32)
        };
        Some([compile(0)?, compile(1)?])
    }

    pub(crate) fn speeds(&self, instance: RotaryInstance, control: i8, force: bool) -> [u32; 2] {
        if instance.mode != 0 {
            return [0, 0];
        }
        if force || instance.parameters[4] == 0 {
            if instance.speed == 0 {
                [0xe5b00, 0xe8800]
            } else {
                [0x7fffff; 2]
            }
        } else {
            [
                rate(&instance.parameters, control, false),
                rate(&instance.parameters, control, true),
            ]
        }
    }
    fn publish_speed(
        &self,
        batch: &mut EffectParameterBatch,
        instance: RotaryInstance,
        control: i8,
        force: bool,
    ) -> Option<()> {
        for (i, v) in self
            .speeds(instance, control, force)
            .into_iter()
            .enumerate()
        {
            direct(batch, instance.origin, 29 + 2 * i as u32, v)?;
        }
        Some(())
    }
    fn publish_acceleration(
        &self,
        batch: &mut EffectParameterBatch,
        instance: RotaryInstance,
    ) -> Option<()> {
        for (i, v) in self
            .acceleration(&instance.parameters, instance.mode != 0)?
            .into_iter()
            .enumerate()
        {
            direct(batch, instance.origin, 30 + 2 * i as u32, v)?;
        }
        Some(())
    }
    fn midi_service(
        &self,
        next: &mut RotaryRack,
        batch: &mut EffectParameterBatch,
        midi: &EffectMidiSources,
        force_refresh: bool,
    ) -> Option<()> {
        for slot in 0..8 {
            if next.instances[slot].kind != 29
                || (slot % 2 == 1 && next.instances[slot - 1].kind >= 29)
            {
                continue;
            }
            let mut instance = next.instances[slot];
            let p = instance.parameters;
            let part = (slot / 2) as u8;
            if p[2] != 0 {
                let value = midi.value(part, p[2])?.abs();
                let old = instance.primary;
                if value != old || force_refresh {
                    let changed = if p[3] == 0 {
                        value >= 64 && old < 64
                    } else {
                        (value >= 64) != (old >= 64)
                    };
                    if changed {
                        instance.mode = if p[3] == 0 {
                            u32::from(instance.mode == 0)
                        } else {
                            u32::from(value >= 64)
                        };
                        self.publish_acceleration(batch, instance)?;
                        self.publish_speed(batch, instance, 0, true)?;
                    }
                    instance.primary = value;
                }
            }
            let mut changed = false;
            let mut value = 0;
            if p[4] == 0 {
                if p[6] != 0 {
                    value = midi.value(part, p[6])?.abs();
                    let old = instance.secondary;
                    if value != old || force_refresh {
                        changed = if p[7] == 0 {
                            value >= 64 && old < 64
                        } else {
                            (value >= 64) != (old >= 64)
                        };
                        if changed {
                            instance.speed = if p[7] == 0 {
                                u32::from(instance.speed == 0)
                            } else {
                                u32::from(value >= 64)
                            };
                        }
                        instance.secondary = value;
                    }
                }
            } else if p[9] != 0 {
                value = midi.value(part, p[9])?.abs();
                changed = value != instance.secondary || force_refresh;
                if changed {
                    instance.secondary = value;
                }
            }
            if changed && instance.mode == 0 {
                self.publish_speed(batch, instance, value, false)?;
            }
            next.instances[slot] = instance;
        }
        Some(())
    }
    pub fn prepare_midi(
        &self,
        rack: &RotaryRack,
        midi: &EffectMidiSources,
    ) -> Option<PreparedRotaryEdit> {
        self.prepare_midi_refresh(rack, midi, false)
    }
    pub fn prepare_midi_refresh(
        &self,
        rack: &RotaryRack,
        midi: &EffectMidiSources,
        force_refresh: bool,
    ) -> Option<PreparedRotaryEdit> {
        let mut next = *rack;
        let mut batch = EffectParameterBatch::from_lfo(None);
        self.midi_service(&mut next, &mut batch, midi, force_refresh)?;
        Some(PreparedRotaryEdit { next, batch })
    }
    pub fn prepare(
        &self,
        rack: &RotaryRack,
        edit: RotaryEdit,
        midi: &EffectMidiSources,
    ) -> Option<PreparedRotaryEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        if slot >= 8 || parameter >= 19 {
            return None;
        }
        let valid = |v: u8, r: EffectParameterRange| {
            let x = i32::from(v) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&x)
        };
        if !valid(edit.value, self.ranges[parameter])
            || edit.parameters[..19]
                .iter()
                .zip(self.ranges)
                .any(|(&v, r)| !valid(v, r))
        {
            return None;
        }
        let mut next = *rack;
        next.instances[slot].kind = 29;
        next.instances[slot].parameters = edit.parameters;
        next.instances[slot].origin = edit.origin;
        let mut batch = EffectParameterBatch::from_lfo(None);
        let control = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            edit.parameter,
            edit.owners[0],
            edit.owners[1],
            false,
        );
        for (index, group) in self.groups.iter().enumerate() {
            // SYS07AC52 uses the second dependency word for the entire tail;
            // SHLD at group 64 is zero, then its shift count wraps at group 65.
            let mask = if index == 64 {
                0
            } else {
                0x80000000 >> (index % 32)
            };
            if self.dependencies[parameter][usize::from(index >= 32)] & mask == 0 {
                continue;
            }
            let instance = next.instances[slot];
            let p = edit.parameters;
            match group.action {
                0 => {}
                63 => {
                    if instance.controller_source != u32::from(edit.value.min(12)) {
                        let part = edit.slot / 2;
                        next.instances[slot].controller_source = u32::from(p[2]);
                        next.instances[slot].primary = midi.value(part, p[2])?.abs();
                        next.instances[slot].secondary =
                            midi.value(part, if p[4] == 0 { p[6] } else { p[9] })?.abs();
                        self.midi_service(&mut next, &mut batch, midi, false)?;
                    }
                }
                64 => {
                    let mut value = midi.value(edit.slot / 2, p[9])?.abs();
                    match parameter {
                        1 => next.instances[slot].mode = u32::from(p[1]),
                        5 => next.instances[slot].speed = u32::from(p[5]),
                        8 => value = p[8] as i8,
                        4 | 10 => {}
                        _ => continue,
                    }
                    self.publish_speed(&mut batch, next.instances[slot], value, false)?;
                }
                65 => {
                    let words = self.acceleration(&p, p[1] == 0)?;
                    if parameter == 1 || parameter == 12 {
                        direct(&mut batch, edit.origin, 30, words[0])?;
                    }
                    if parameter == 1 || parameter == 14 {
                        direct(&mut batch, edit.origin, 32, words[1])?;
                    }
                }
                68 => {
                    let spread = p[17];
                    let (a, b) = if spread < 64 {
                        (
                            0x550000 + u32::from(spread) * 0xabff,
                            (64 - u32::from(spread)) * 0x1540,
                        )
                    } else {
                        (0x7fffff, 0)
                    };
                    for (i, v) in [a, b, b, a].into_iter().enumerate() {
                        direct(&mut batch, edit.origin, 54 + i as u32, v)?;
                    }
                }
                1 | 2 | 11 | 14 | 66 | 67 => {
                    let value = match group.action {
                        66 => {
                            if edit.value == 0 {
                                group.second as u32
                            } else {
                                (i32::from(edit.value)
                                    .wrapping_add(24)
                                    .wrapping_mul(group.first.wrapping_sub(group.second))
                                    .wrapping_div(100)) as u32
                            }
                        }
                        67 => 0x7fffffu32.wrapping_sub(group.range.compile(
                            EffectCurve::Linear,
                            i32::from(edit.value),
                            group.first,
                            group.second,
                        )? as u32),
                        1 | 2 => {
                            let width =
                                i32::from(group.range.maximum) - i32::from(group.range.minimum);
                            if width <= 0 {
                                return None;
                            }
                            let position = i32::from(edit.value) - i32::from(group.range.minimum);
                            if group.action == 1 {
                                if position > width / 2 {
                                    group
                                        .first
                                        .wrapping_div(width)
                                        .wrapping_mul(width - position)
                                        .wrapping_mul(2) as u32
                                } else {
                                    group.first as u32
                                }
                            } else if position > width / 2 {
                                group.first as u32
                            } else {
                                group
                                    .first
                                    .wrapping_div(width)
                                    .wrapping_mul(position)
                                    .wrapping_mul(2) as u32
                            }
                        }
                        action => group.range.compile(
                            match action {
                                14 => EffectCurve::InverseScale,
                                _ => EffectCurve::Linear,
                            },
                            i32::from(edit.value),
                            group.first,
                            group.second,
                        )? as u32,
                    };
                    let prepared = next.assignments.prepare(CoefficientChange {
                        direct_switch: control.direct_switch,
                        standalone: false,
                        enabled_argument: control.enabled_argument,
                        target: u32::from(edit.origin) + index as u32,
                        value,
                        mode: 0,
                    });
                    batch.append(&prepared.plan)?;
                    next.assignments = prepared.next;
                }
                _ => return None,
            }
        }
        Some(PreparedRotaryEdit { next, batch })
    }
}
