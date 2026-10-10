//! Original SYS028F7C MIDI source values used by the effect controllers.
//! Raw event state enters this boundary; computed source outputs are not inputs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectMidiTimbre {
    pub control_49: i8,
    pub bend: i16,
    pub control_4b: i8,
    pub channel: u8,
    pub switch_45: u8,
    pub controls_4c_50: [i8; 5],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectMidiSources {
    pub timbres: [EffectMidiTimbre; 4],
    pub channel_controls: [[u8; 16]; 2],
    /// Original consecutive global words; the second word belongs to an
    /// alternate axis helper omitted from the twelve-selector dispatch table.
    pub global_controls: [u16; 12],
    pub shared_control: i8,
}
/// Raw global assignment bytes at offsets 9..13. SYS076C18 reads the stored
/// codes without clamping them to the editor's nominal 0..120 range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectMidiPolarity {
    pub assignments: [u8; 5],
}
impl EffectMidiPolarity {
    pub fn bipolar(self, selector: u8) -> bool {
        match selector {
            2 => true,
            8..=12 => matches!(self.assignments[usize::from(selector - 8)], 0 | 116..=120),
            _ => false,
        }
    }
}
impl EffectMidiSources {
    /// Parts 0..3 are timbres; part 4 is the global/master source context.
    pub fn value(&self, part: u8, selector: u8) -> Option<i8> {
        if selector == 0 {
            return Some(0);
        }
        if selector > 12 || part > 4 {
            return None;
        }
        let value = if part == 4 {
            let index = if (2..=6).contains(&selector) {
                selector
            } else {
                selector - 1
            };
            let raw = self.global_controls[usize::from(index)];
            match selector {
                2 => i32::from((raw >> 8) as u8 as i8),
                7 => i32::from(self.shared_control),
                8..=12 => i32::from(raw as u8 as i8),
                _ => i32::from(raw & 127),
            }
        } else {
            let timbre = &self.timbres[usize::from(part)];
            match selector {
                1 => i32::from(timbre.control_49),
                2 => i32::from(timbre.bend) >> 6,
                3 => i32::from(timbre.control_4b),
                4 | 5 => i32::from(
                    self.channel_controls[usize::from(selector - 4)]
                        [usize::from(timbre.channel & 15)]
                        & 127,
                ),
                6 => {
                    if timbre.switch_45 == 0 {
                        0
                    } else {
                        127
                    }
                }
                7 => i32::from(self.shared_control),
                8..=12 => i32::from(timbre.controls_4c_50[usize::from(selector - 8)]),
                _ => unreachable!(),
            }
        };
        Some(value.clamp(-127, 127) as i8)
    }
}
/// SYS07758E's signed, separately truncated normalization.
pub fn effect_controller_level(value: i8) -> i32 {
    let value = i32::from(value);
    value.wrapping_mul(0x7fffff).wrapping_div(127)
}
