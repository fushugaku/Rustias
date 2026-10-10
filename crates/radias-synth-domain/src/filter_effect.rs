//! Original St.Filter coefficient preparation: SYS0775E2/0776C6/07671A.
//! These publications do not establish FXD03 filter sample arithmetic.
use crate::{
    controller_filter::ControllerFilterTables,
    effect_parameters::{
        EffectInterpolationControl, EffectParameterBatch, PreparedEffectParameterChange,
    },
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
};

#[derive(Clone)]
pub struct FilterEffectTables {
    pub frequency: ControllerFilterTables,
    pub resonance: [u16; 128],
    pub resonance_gain: [u32; 128],
    pub response: [u32; 128],
    pub response_complement: [u32; 128],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilterEffectCache {
    pub frequency: u32,
    pub dirty: u32,
}
#[derive(Clone, Copy)]
pub struct FilterEffectFrequency {
    pub origin: u16,
    pub cutoff: u8,
    pub resonance: u8,
    pub modulation_depth: u8,
    pub modulation: i16,
}
pub struct PreparedFilterEffectFrequency {
    pub next: FilterEffectCache,
    pub batch: EffectParameterBatch,
}
// SYS04D172 with mode 1: unsigned 32x32 product, shifted 31, then narrowed.
fn multiply_unsigned(a: u32, b: u32) -> u32 {
    ((u64::from(a) * u64::from(b)) >> 31) as u32
}
fn half_toward_zero(a: u32) -> u32 {
    ((a as i32).wrapping_add(i32::from((a as i32) < 0)) >> 1) as u32
}
impl FilterEffectTables {
    pub fn prepare_frequency(
        &self,
        cache: FilterEffectCache,
        change: FilterEffectFrequency,
    ) -> Option<PreparedFilterEffectFrequency> {
        if change.cutoff > 127
            || change.resonance > 127
            || !(1..=127).contains(&change.modulation_depth)
        {
            return None;
        }
        let depth = i32::from(change.modulation_depth) - 64;
        let code = (i32::from(change.cutoff) << 8) + ((depth * i32::from(change.modulation)) >> 6);
        let frequency = self.frequency.frequency(code);
        let mut batch = EffectParameterBatch::from_lfo(None);
        if cache.frequency == frequency && cache.dirty != 1 {
            return Some(PreparedFilterEffectFrequency { next: cache, batch });
        }
        let excess = frequency.wrapping_add(0xae7c972b);
        let shaping = if (excess as i32) < 0 {
            0
        } else {
            let square = multiply_unsigned(excess, excess);
            square.wrapping_add((square as i32 >> 1) as u32)
        };
        let damped = multiply_unsigned(
            self.resonance_gain[usize::from(change.resonance)],
            0x7fffffff_u32.wrapping_sub(shaping),
        );
        let gain = multiply_unsigned(frequency, half_toward_zero(damped).wrapping_add(0x3fffffff));
        let feedback = multiply_unsigned(
            0x7fffffff_u32.wrapping_sub(damped),
            0x7fffffff_u32.wrapping_sub(half_toward_zero(frequency)),
        );
        batch.push_direct(u32::from(change.origin) + 6, (gain as i32 >> 8) as u32)?;
        batch.push_direct(u32::from(change.origin) + 8, (feedback as i32 >> 8) as u32)?;
        Some(PreparedFilterEffectFrequency {
            next: FilterEffectCache {
                frequency,
                dirty: 0,
            },
            batch,
        })
    }
    pub fn prepare_trim(
        &self,
        origin: u16,
        resonance: u8,
        trim: u8,
    ) -> Option<EffectParameterBatch> {
        let coefficient = (u32::from(*self.resonance.get(usize::from(resonance))?) << 8) | 255;
        if trim > 127 {
            return None;
        }
        let mut batch = EffectParameterBatch::from_lfo(None);
        // SYS0776C6's signed magic division is the exact truncating /127.
        let value = (coefficient.wrapping_mul(u32::from(trim)) as i32).wrapping_div(127);
        batch.push_direct(u32::from(origin) + 5, value as u32)?;
        Some(batch)
    }
    pub fn prepare_response(
        &self,
        assignments: &EffectCoefficientAssignments,
        origin: u16,
        value: u8,
        interpolation: EffectInterpolationControl,
    ) -> Option<PreparedEffectParameterChange> {
        let first = *self.response.get(usize::from(value))?;
        let second = *self.response_complement.get(usize::from(value))?;
        let mut next = *assignments;
        let mut batch = EffectParameterBatch::from_lfo(None);
        for (offset, coefficient) in [(28, first), (29, second)] {
            let prepared = next.prepare(CoefficientChange {
                direct_switch: interpolation.direct_switch,
                standalone: false,
                enabled_argument: interpolation.enabled_argument,
                mode: 0,
                target: u32::from(origin) + offset,
                value: coefficient,
            });
            batch.append(&prepared.plan)?;
            next = prepared.next;
        }
        Some(PreparedEffectParameterChange { next, batch })
    }
}
