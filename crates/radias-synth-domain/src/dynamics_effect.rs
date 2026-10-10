//! Native SYS 2.00 dynamics parameter compilers, separate from FXD03 audio.
use crate::{
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_parameters::{
        EffectInterpolationControl, EffectParameterBatch, PreparedEffectParameterChange,
    },
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicsEffectKind {
    Compressor,
    Limiter,
    Gate,
}
impl DynamicsEffectKind {
    pub fn from_effect_type(kind: u8) -> Option<Self> {
        Some(match kind {
            1 => Self::Compressor,
            2 => Self::Limiter,
            3 => Self::Gate,
            _ => return None,
        })
    }
    pub fn effect_type(self) -> u8 {
        self.index() as u8 + 1
    }
    pub fn parameter_count(self) -> usize {
        if self == Self::Compressor { 5 } else { 6 }
    }
    fn index(self) -> usize {
        match self {
            Self::Compressor => 0,
            Self::Limiter => 1,
            Self::Gate => 2,
        }
    }
}
pub use crate::effect_parameters::EffectCoefficientGroup as DynamicsCoefficientGroup;
#[derive(Clone)]
pub struct DynamicsEffectDefinition {
    pub parameter_ranges: [EffectParameterRange; 6],
    pub dependencies: [u32; 6],
    pub groups: [DynamicsCoefficientGroup; 19],
}
#[derive(Clone)]
pub struct DynamicsEffectTables {
    pub definitions: [DynamicsEffectDefinition; 3],
    pub limiter_threshold: [u32; 41],
    /// Legal encoded values 23..=88; the original pointer is biased by -23.
    pub gain: [u32; 66],
    pub ratio: [u32; 70],
    pub gate_threshold: [u32; 128],
    /// Legal raw sensitivity values 1..=127.
    pub sensitivity: [u32; 127],
    /// SYS0766A6's unchanged pointer lookup for every raw Master byte,
    /// including the preceding word at encoded zero and adjacent table data.
    pub raw_master_sensitivity: [u32; 256],
    pub attack: [u32; 128],
    pub release: [u32; 128],
    /// Immutable ROM windows for arguments -128..=255. Insert MOV.B sign
    /// extends stored bytes; Master EXTU.B retains their unsigned values.
    /// The original getters also read adjacent ROM outside legal UI ranges.
    pub raw_lookup: [[u32; 384]; 7],
}
#[derive(Clone, Copy)]
pub struct DynamicsParameterChange {
    pub kind: DynamicsEffectKind,
    pub origin: u16,
    pub parameter: u8,
    pub value: u8,
    pub interpolation: EffectInterpolationControl,
}
impl DynamicsEffectTables {
    pub fn raw_word(&self, action: u8, argument: i32) -> Option<u32> {
        let table = match action {
            17 => 0,
            18 => 1,
            19 => 2,
            27 => 3,
            28 => 4,
            29 => 5,
            30 => 6,
            _ => return None,
        };
        self.raw_lookup[table]
            .get(usize::try_from(argument.checked_add(128)?).ok()?)
            .copied()
    }
    pub fn prepare(
        &self,
        assignments: &EffectCoefficientAssignments,
        change: DynamicsParameterChange,
    ) -> Option<PreparedEffectParameterChange> {
        self.prepare_inner(assignments, change, false)
    }
    /// SYS07AC52's stored argument is independent of constructor-clamped
    /// parameters and is sign extended before every dynamics group call.
    pub fn prepare_stored_insert_argument(
        &self,
        assignments: &EffectCoefficientAssignments,
        change: DynamicsParameterChange,
    ) -> Option<PreparedEffectParameterChange> {
        self.prepare_inner(assignments, change, true)
    }
    fn prepare_inner(
        &self,
        assignments: &EffectCoefficientAssignments,
        change: DynamicsParameterChange,
        stored_insert: bool,
    ) -> Option<PreparedEffectParameterChange> {
        let DynamicsParameterChange {
            kind,
            origin,
            parameter,
            value,
            interpolation,
        } = change;
        let definition = &self.definitions[kind.index()];
        let parameter = usize::from(parameter);
        if parameter >= kind.parameter_count() {
            return None;
        }
        let range = definition.parameter_ranges[parameter];
        let decoded = i32::from(value) - i32::from(range.encoded_zero);
        if !stored_insert
            && (decoded < i32::from(range.minimum) || decoded > i32::from(range.maximum))
        {
            return None;
        }
        let argument = if stored_insert {
            i32::from(value as i8)
        } else {
            i32::from(value)
        };
        let mut next = *assignments;
        let mut batch = EffectParameterBatch::from_lfo(None);
        for (index, group) in definition.groups.iter().enumerate() {
            if definition.dependencies[parameter] & (0x80000000 >> index) == 0 {
                continue;
            }
            if group.action == 28 {
                // Complete SYS0766A6: two separately assigned updates followed
                // by an unconditional direct copy. The fifth stack argument
                // changes between calls; both actual R7 modes remain zero.
                let coefficient =
                    group
                        .range
                        .compile(EffectCurve::Linear, argument, group.first, group.second)?
                        as u32;
                let dependent = self.raw_word(28, argument)?;
                for (offset, value) in [(11, coefficient), (28, dependent)] {
                    let prepared = next.prepare(CoefficientChange {
                        direct_switch: interpolation.direct_switch,
                        standalone: false,
                        enabled_argument: interpolation.enabled_argument,
                        mode: 0,
                        target: u32::from(origin) + offset,
                        value,
                    });
                    batch.append(&prepared.plan)?;
                    next = prepared.next;
                }
                batch.push_direct(u32::from(origin) + 12, coefficient)?;
                continue;
            }
            let curve = match group.action {
                9 => Some(EffectCurve::Quadratic),
                11 => Some(EffectCurve::Linear),
                14 => Some(EffectCurve::InverseScale),
                15 => Some(EffectCurve::Select),
                _ => None,
            };
            let coefficient = if let Some(curve) = curve {
                group
                    .range
                    .compile(curve, argument, group.first, group.second)? as u32
            } else {
                self.raw_word(group.action, argument)?
            };
            let prepared = next.prepare(CoefficientChange {
                direct_switch: interpolation.direct_switch,
                standalone: false,
                enabled_argument: interpolation.enabled_argument,
                mode: 0,
                target: u32::from(origin) + index as u32,
                value: coefficient,
            });
            batch.append(&prepared.plan)?;
            next = prepared.next;
        }
        Some(PreparedEffectParameterChange { next, batch })
    }
}
