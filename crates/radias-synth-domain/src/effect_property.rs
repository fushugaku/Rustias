//! Effect properties of SYS080024: stored packing and runtime parameter bytes.
use crate::{
    effect_curves::EffectParameterRange, insert_effect_control::InsertControlState,
    mixed_effect_parameter::EffectParameterTarget, program::Program,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectProperty {
    Enabled,
    Kind,
    Owner(u8),
    Parameter(u8),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectPropertyChange {
    pub target: EffectParameterTarget,
    pub property: EffectProperty,
    pub value: i32,
}
pub struct EffectPropertyTables {
    pub ranges: [EffectParameterRange; 256],
    pub insert_range_indices: [[u8; 20]; 31],
    pub master_range_indices: [[u8; 20]; 31],
    pub tempo_rate: [u8; 30],
    pub free_rate: [u8; 100],
    pub tempo_rate_display: [u8; 128],
    pub free_rate_display: [u8; 128],
}
pub struct PreparedEffectProperty {
    pub next: InsertControlState,
    pub program: Program,
    pub value: i32,
}
impl EffectPropertyTables {
    pub fn displayed_value(
        &self,
        program: &Program,
        target: EffectParameterTarget,
        property: EffectProperty,
    ) -> Option<i32> {
        let (offset, insert, indices) = match target {
            EffectParameterTarget::Insert(slot) if slot < 8 => (
                168 + 228 * usize::from(slot / 2) + 24 * usize::from(slot % 2),
                true,
                &self.insert_range_indices,
            ),
            EffectParameterTarget::Insert(_) => return None,
            EffectParameterTarget::Master => (1038, false, &self.master_range_indices),
        };
        let b = program.bytes();
        Some(match property {
            EffectProperty::Enabled => i32::from(b[offset] >> 7),
            EffectProperty::Kind => i32::from(b[offset] & 127),
            EffectProperty::Owner(n) if insert && n < 2 => {
                i32::from(b[offset + 2 + usize::from(n)] & 31)
            }
            EffectProperty::Owner(0) if !insert => i32::from(b[offset + 1] & 31),
            EffectProperty::Owner(_) => return None,
            EffectProperty::Parameter(p) if p < 20 => {
                let p = usize::from(p);
                let row = indices.get(usize::from(b[offset] & 127))?;
                let index = usize::from(row[p]);
                let range = self.ranges[index];
                let raw = i32::from(b[offset + if insert { 4 } else { 2 } + p])
                    - i32::from(range.encoded_zero);
                let value = raw.clamp(i32::from(range.minimum), i32::from(range.maximum));
                if index == 149 {
                    let prev = p.checked_sub(1)?;
                    let r = self.ranges[usize::from(row[prev])];
                    let mode = (i32::from(b[offset + if insert { 4 } else { 2 } + prev])
                        - i32::from(r.encoded_zero))
                    .clamp(i32::from(r.minimum), i32::from(r.maximum));
                    i32::from(
                        if (insert && mode == 2) || (!insert && matches!(mode, 4 | 5)) {
                            self.tempo_rate_display[value as usize]
                        } else {
                            self.free_rate_display[value as usize]
                        },
                    )
                } else {
                    value
                }
            }
            EffectProperty::Parameter(_) => return None,
        })
    }
    pub fn prepare(
        &self,
        state: &InsertControlState,
        program: &Program,
        edit: EffectPropertyChange,
    ) -> Option<PreparedEffectProperty> {
        let (offset, slot, indices) = match edit.target {
            EffectParameterTarget::Insert(slot) if slot < 8 => (
                168 + 228 * usize::from(slot / 2) + 24 * usize::from(slot % 2),
                Some(usize::from(slot)),
                &self.insert_range_indices,
            ),
            EffectParameterTarget::Insert(_) => return None,
            EffectParameterTarget::Master => (1038, None, &self.master_range_indices),
        };
        let mut next = *state;
        let mut bytes = *program.bytes();
        let (byte, mask, shift, index, parameter) = match edit.property {
            EffectProperty::Enabled => (offset, 128u8, 7, 36usize, None),
            EffectProperty::Kind => (offset, 127, 0, 53, None),
            EffectProperty::Owner(owner) if slot.is_some() && owner < 2 => {
                (offset + 2 + usize::from(owner), 31, 0, 55, None)
            }
            EffectProperty::Owner(0) if slot.is_none() => (offset + 1, 31, 0, 55, None),
            EffectProperty::Owner(_) => return None,
            EffectProperty::Parameter(parameter) if parameter < 20 => {
                let p = usize::from(parameter);
                let kind = usize::from(bytes[offset] & 127);
                let index = usize::from(*indices.get(kind)?.get(p)?);
                (
                    offset + if slot.is_some() { 4 } else { 2 } + p,
                    255,
                    0,
                    index,
                    Some(p),
                )
            }
            EffectProperty::Parameter(_) => return None,
        };
        let range = self.ranges[index];
        let mut value = edit
            .value
            .clamp(i32::from(range.minimum), i32::from(range.maximum));
        // SYS075820 tests the property index 31h: the second Insert kind
        // selector admits IDs 0..28, while first Insert and Master admit 0..30.
        if edit.property == EffectProperty::Kind && slot.is_some_and(|s| s % 2 == 1) {
            value = edit.value.clamp(0, 28);
        }
        let encoded = if index == 149 {
            let p = parameter?.checked_sub(1)?;
            let previous_index = usize::from(indices[usize::from(bytes[offset] & 127)][p]);
            let previous_range = self.ranges[previous_index];
            let previous = i32::from(bytes[offset + if slot.is_some() { 4 } else { 2 } + p])
                - i32::from(previous_range.encoded_zero);
            let mode = previous.clamp(
                i32::from(previous_range.minimum),
                i32::from(previous_range.maximum),
            );
            let tempo = if slot.is_some() {
                mode == 2
            } else {
                matches!(mode, 4 | 5)
            };
            value = edit.value.clamp(0, if tempo { 29 } else { 99 });
            if tempo {
                self.tempo_rate[value as usize]
            } else {
                self.free_rate[value as usize]
            }
        } else {
            value.wrapping_add(i32::from(range.encoded_zero)) as u8
        };
        bytes[byte] = (bytes[byte] & !mask) | encoded.wrapping_shl(shift);
        if let Some(parameter) = parameter {
            if let Some(slot) = slot {
                next.midi.inserts[slot].buffer.parameters[parameter] = encoded;
            } else {
                next.midi.master.parameters[parameter] = encoded;
            }
        }
        Some(PreparedEffectProperty {
            next,
            program: Program::from_bytes(&bytes).ok()?,
            value,
        })
    }
}
