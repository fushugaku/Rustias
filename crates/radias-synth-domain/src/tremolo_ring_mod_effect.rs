//! Complete St.Tremolo and St.Ring Mod insert parameter compilers.
use crate::{
    effect_control::{EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_equalizer::multiply,
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_setters::EffectSelectorWrites,
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    lfo_tempo::LfoTempoTables,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TremoloRingModKind {
    Tremolo,
    RingMod,
}
impl TremoloRingModKind {
    pub fn index(self) -> usize {
        usize::from(self == Self::RingMod)
    }
    pub fn id(self) -> u8 {
        24 + self.index() as u8
    }
    pub fn parameter_count(self) -> usize {
        if self == Self::Tremolo { 10 } else { 15 }
    }
}
pub struct TremoloRingModDefinition {
    pub ranges: [EffectParameterRange; 15],
    pub dependencies: [[u32; 2]; 15],
    pub groups: [EffectCoefficientGroup; 21],
    pub group_count: usize,
    pub lfo_mapping: EffectLfoMapping,
}
pub struct TremoloRingModTables {
    pub definitions: [TremoloRingModDefinition; 2],
    pub fixed_frequency: [u32; 128],
    pub note_frequency: [u32; 128],
    pub fine_frequency: [u32; 128],
    pub tempo: LfoTempoTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TremoloRingModRack {
    pub lfos: [EffectLfoProgram; 8],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct TremoloRingModEdit {
    pub kind: TremoloRingModKind,
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub clock_rate: u32,
    /// Source SYS009D14 masks the returned current-note byte to seven bits.
    pub current_note: u8,
}
pub struct PreparedTremoloRingModEdit {
    pub next: TremoloRingModRack,
    pub batch: EffectParameterBatch,
}
fn publish(
    rack: &mut TremoloRingModRack,
    batch: &mut EffectParameterBatch,
    control: EffectInterpolationControl,
    target: u32,
    value: u32,
) -> Option<()> {
    let p = rack.assignments.prepare(CoefficientChange {
        direct_switch: control.direct_switch,
        standalone: false,
        enabled_argument: control.enabled_argument,
        target,
        value,
        mode: 0,
    });
    batch.append(&p.plan)?;
    rack.assignments = p.next;
    Some(())
}
impl TremoloRingModTables {
    pub fn frequency(&self, parameters: &[u8; 20], current_note: u8) -> Option<u32> {
        self.frequency_for_global_note(parameters, current_note & 127)
    }
    /// SYS009CFC retains the full global note byte; timbre getters mask bit7.
    pub fn frequency_for_global_note(
        &self,
        parameters: &[u8; 20],
        current_note: u8,
    ) -> Option<u32> {
        if parameters[1] == 0 {
            return self
                .fixed_frequency
                .get(usize::from(parameters[2]))
                .copied();
        }
        let note = (i32::from(current_note) + i32::from(parameters[3]) - 64).clamp(0, 127);
        let word = multiply(
            *self.note_frequency.get(note as usize)?,
            *self.fine_frequency.get(usize::from(parameters[4]))?,
            9,
        );
        Some((word as i32).wrapping_div(5) as u32)
    }
    pub fn prepare(
        &self,
        rack: &TremoloRingModRack,
        edit: TremoloRingModEdit,
    ) -> Option<PreparedTremoloRingModEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        let definition = &self.definitions[edit.kind.index()];
        if slot >= 8 || parameter >= edit.kind.parameter_count() {
            return None;
        }
        let valid = |v: u8, r: EffectParameterRange| {
            let value = i32::from(v) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&value)
        };
        if !valid(edit.value, definition.ranges[parameter])
            || edit.parameters[..edit.kind.parameter_count()]
                .iter()
                .zip(definition.ranges)
                .any(|(&v, r)| !valid(v, r))
        {
            return None;
        }
        let mut next = *rack;
        let mut batch = EffectParameterBatch::from_lfo(None);
        let control = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            edit.parameter,
            edit.owners[0],
            edit.owners[1],
            false,
        );
        for (index, group) in definition.groups[..definition.group_count]
            .iter()
            .enumerate()
        {
            if definition.dependencies[parameter][index / 32] & (0x80000000 >> (index % 32)) == 0 {
                continue;
            }
            if group.action == 34 {
                let p = next.lfos[slot].prepare(
                    &edit.parameters,
                    definition.lfo_mapping,
                    EffectLfoSlot::new(edit.slot)?,
                    0,
                    edit.clock_rate,
                    &self.tempo,
                )?;
                if let Some(p) = p {
                    next.lfos[slot] = p.program;
                }
                batch.extend(&EffectParameterBatch::from_lfo(p))?;
                continue;
            }
            if group.action == 61 {
                let mut owner = EffectInterpolationControl::from_owners(
                    edit.direct_switch,
                    2,
                    edit.owners[0],
                    edit.owners[1],
                    false,
                );
                if owner.enabled_argument == 0 {
                    owner = EffectInterpolationControl::from_owners(
                        edit.direct_switch,
                        3,
                        edit.owners[0],
                        edit.owners[1],
                        false,
                    );
                }
                publish(
                    &mut next,
                    &mut batch,
                    owner,
                    u32::from(edit.origin) + 6,
                    self.frequency(&edit.parameters, edit.current_note)?,
                )?;
                continue;
            }
            if group.action == 62 {
                let writes =
                    EffectSelectorWrites::compile(edit.origin, index as u32, u32::from(edit.value));
                for word in &writes.words[..usize::from(writes.count)] {
                    batch.push_direct(u32::from(word.address), word.tagged_value)?;
                }
                continue;
            }
            let value = if matches!(group.action, 4 | 6) {
                let mix = EffectMix::compile(
                    EffectKind::new(edit.kind.id())?,
                    edit.value,
                    Default::default(),
                )?;
                if group.action == 6 {
                    mix.dry as u32
                } else {
                    mix.wet as u32
                }
            } else {
                let curve = match group.action {
                    10 => EffectCurve::OffsetQuadratic,
                    11 => EffectCurve::Linear,
                    13 => EffectCurve::EaseOut,
                    14 => EffectCurve::InverseScale,
                    _ => return None,
                };
                group
                    .range
                    .compile(curve, i32::from(edit.value), group.first, group.second)?
                    as u32
            };
            publish(
                &mut next,
                &mut batch,
                control,
                u32::from(edit.origin) + index as u32,
                value,
            )?;
        }
        Some(PreparedTremoloRingModEdit { next, batch })
    }
}
