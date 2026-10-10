//! Complete St.2BandEQ/Distortion insert dependency controllers, SYS07AC52.
use crate::{
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_equalizer::EffectEqualizerTables,
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    program::Program,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EqualizerEffectKind {
    StereoEqualizer,
    Distortion,
}
impl EqualizerEffectKind {
    pub fn from_type(kind: u8) -> Option<Self> {
        match kind {
            6 => Some(Self::StereoEqualizer),
            7 => Some(Self::Distortion),
            _ => None,
        }
    }
    fn index(self) -> usize {
        usize::from(self == Self::Distortion)
    }
}
pub struct EqualizerEffectDefinition {
    pub ranges: [EffectParameterRange; 15],
    pub dependencies: [[u32; 2]; 15],
    pub groups: [EffectCoefficientGroup; 43],
    pub parameter_count: u8,
    pub group_count: u8,
}
pub struct EqualizerEffectTables {
    pub definitions: [EqualizerEffectDefinition; 2],
    pub gain: [u32; 73],
    pub distortion_gain: [u32; 128],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EqualizerEffectInstance {
    pub parameters: [u8; 20],
    pub previous_parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EqualizerEffectRack {
    pub instances: [EqualizerEffectInstance; 8],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct EqualizerParameterEdit {
    pub kind: EqualizerEffectKind,
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub previous_parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
}
pub struct PreparedEqualizerEdit {
    pub next: EqualizerEffectRack,
    pub batch: EffectParameterBatch,
}
impl EqualizerEffectTables {
    pub fn prepare(
        &self,
        rack: &EqualizerEffectRack,
        edit: EqualizerParameterEdit,
        core: &EffectEqualizerTables,
        program: &Program,
    ) -> Option<PreparedEqualizerEdit> {
        let definition = &self.definitions[edit.kind.index()];
        let parameter = usize::from(edit.parameter);
        let slot = usize::from(edit.slot);
        if slot >= 8 || parameter >= usize::from(definition.parameter_count) {
            return None;
        }
        let valid = |value: u8, range: EffectParameterRange| {
            let v = i32::from(value) - i32::from(range.encoded_zero);
            (i32::from(range.minimum)..=i32::from(range.maximum)).contains(&v)
        };
        if !valid(edit.value, definition.ranges[parameter]) {
            return None;
        }
        for (value, range) in edit
            .parameters
            .iter()
            .zip(&definition.ranges)
            .take(usize::from(definition.parameter_count))
        {
            if !valid(*value, *range) {
                return None;
            }
        }
        let mut next = *rack;
        next.instances[slot] = EqualizerEffectInstance {
            parameters: edit.parameters,
            previous_parameters: edit.previous_parameters,
            origin: edit.origin,
            owners: edit.owners,
        };
        let mut batch = EffectParameterBatch::from_lfo(None);
        for index in 0..usize::from(definition.group_count) {
            if definition.dependencies[parameter][index / 32] & (0x80000000 >> (index % 32)) == 0 {
                continue;
            }
            let group = &definition.groups[index];
            if group.action == 35 {
                self.band(&mut next, &mut batch, edit, index, core, program)?;
                continue;
            }
            let instance = next.instances[slot];
            let interpolation = EffectInterpolationControl::from_owners(
                edit.direct_switch,
                edit.parameter,
                instance.owners[0],
                instance.owners[1],
                false,
            );
            let value = if group.action == 26 {
                if edit.kind == EqualizerEffectKind::StereoEqualizer {
                    let old = i32::from(instance.previous_parameters[parameter]) - 64;
                    let current = i32::from(instance.parameters[parameter]) - 64;
                    if old * current < 0 {
                        batch.push_command(0, 0x01000014)?;
                    }
                }
                *self.gain.get(usize::from(edit.value.checked_sub(28)?))?
            } else if group.action == 33 {
                *self.distortion_gain.get(usize::from(edit.value))?
            } else {
                let curve = match group.action {
                    9 => EffectCurve::Quadratic,
                    11 => EffectCurve::Linear,
                    14 => EffectCurve::InverseScale,
                    15 => EffectCurve::Select,
                    _ => return None,
                };
                group
                    .range
                    .compile(curve, i32::from(edit.value), group.first, group.second)?
                    as u32
            };
            let prepared = next.assignments.prepare(CoefficientChange {
                direct_switch: edit.direct_switch,
                standalone: false,
                enabled_argument: interpolation.enabled_argument,
                mode: 0,
                target: u32::from(edit.origin) + index as u32,
                value,
            });
            batch.append(&prepared.plan)?;
            next.assignments = prepared.next;
        }
        Some(PreparedEqualizerEdit { next, batch })
    }
    fn band(
        &self,
        rack: &mut EqualizerEffectRack,
        batch: &mut EffectParameterBatch,
        edit: EqualizerParameterEdit,
        index: usize,
        core: &EffectEqualizerTables,
        program: &Program,
    ) -> Option<()> {
        let slot = usize::from(edit.slot);
        let instance = &mut rack.instances[slot];
        let origin = u32::from(instance.origin);
        let base = if edit.kind == EqualizerEffectKind::StereoEqualizer {
            if edit.parameter == 3 || edit.parameter >= 7 {
                7
            } else {
                4
            }
        } else if edit.parameter < 5 {
            2
        } else if edit.parameter < 8 {
            5
        } else if edit.parameter < 11 {
            8
        } else {
            11
        };
        if edit.kind == EqualizerEffectKind::Distortion {
            // SYS077FB4's insert getters use the unoffset parameter (<48),
            // selecting this timbre's first stored insert-controller pair.
            for (owner, value) in instance
                .owners
                .iter_mut()
                .zip(program.timbre(slot / 2)?.effect(0)?.controllers())
            {
                if *owner != u32::from(value) && matches!(value, 4 | 7 | 10 | 13) {
                    *owner = u32::from(value);
                    batch.push_direct(
                        origin + 46,
                        origin.wrapping_add(index as u32).wrapping_sub(20),
                    )?;
                }
            }
        }
        let interpolation = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            (base + 2) as u8,
            instance.owners[0],
            instance.owners[1],
            false,
        );
        let shape = if edit.kind == EqualizerEffectKind::Distortion
            || instance.parameters[if base == 4 { 2 } else { 3 }] == 0
        {
            2
        } else if base == 4 {
            0
        } else {
            1
        };
        let frequency = instance.parameters[base];
        let q = instance.parameters[base + 1];
        let gain = (i32::from(instance.parameters[base + 2]) - 64) as i8;
        let band = if edit.kind == EqualizerEffectKind::StereoEqualizer {
            (base - 4) / 3
        } else {
            (base - 2) / 3
        };
        if gain == 0 {
            let prepared = rack.assignments.prepare(CoefficientChange {
                direct_switch: edit.direct_switch,
                standalone: false,
                enabled_argument: interpolation.enabled_argument,
                mode: 0,
                target: origin + 36 + band as u32,
                value: self.gain[36],
            });
            batch.append(&prepared.plan)?;
            rack.assignments = prepared.next;
            if edit.direct_switch == 0 {
                batch.push_command(0, 0x01000014)?;
            }
        }
        let selected_gain = if gain < 0 { -36 } else { 36 };
        let values = if shape == 2 {
            let coefficients = core.peaking(frequency, q, selected_gain)?;
            let scale = if frequency < 35 {
                1
            } else if frequency < 46 {
                2
            } else {
                3
            };
            let convert =
                |word: u32, exponent: u32| ((word as i32) >> (8 + scale - exponent)) as u32;
            [
                scale,
                convert(coefficients[0], coefficients[5]),
                convert(coefficients[1], coefficients[6]),
                convert(coefficients[3], 1),
                convert(coefficients[2], coefficients[7]),
                convert(coefficients[4], 0),
            ]
        } else {
            let coefficients = if shape == 0 {
                core.low_shelf(frequency, selected_gain)?
                    .map(|v| ((v as i32) >> 2) as u32)
            } else if gain == 0 {
                // Preserve SYS07859E's original zero-gain low-shelf call.
                core.low_shelf(frequency, 36)?
            } else {
                core.high_shelf(frequency, selected_gain)?
            };
            [3, coefficients[0], coefficients[1], coefficients[2], 0, 0]
        };
        for (offset, value) in values.into_iter().enumerate() {
            let tag = if edit.direct_switch == 0 {
                (0x80 | (6 - offset as u32)) << 24
            } else {
                0
            };
            batch.push_command(
                (origin + 12 + 6 * band as u32 + offset as u32) as u16,
                tag | (value & 0xffffff),
            )?;
        }
        Some(())
    }
}
