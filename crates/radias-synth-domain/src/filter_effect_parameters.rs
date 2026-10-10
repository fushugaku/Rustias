//! Complete St.Filter insert parameter dependency controller, SYS07AC52.
use crate::{
    effect_control::{EffectKind, EffectMix},
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    effect_midi::{EffectMidiSources, effect_controller_level},
    effect_parameters::{EffectInterpolationControl, EffectParameterBatch},
    effect_routing::{EffectRoutingContext, EffectRoutingInstance, EffectRoutingTables},
    effect_updates::EffectCoefficientAssignments,
    filter_effect::{FilterEffectCache, FilterEffectFrequency, FilterEffectTables},
    lfo_tempo::LfoTempoTables,
    program::Program,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterEffectInstance {
    pub kind: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub lfo: EffectLfoProgram,
    pub controller_source: u32,
    pub controller_value: i8,
    pub secondary_value: i8,
}
impl Default for FilterEffectInstance {
    fn default() -> Self {
        Self {
            kind: 4,
            parameters: [0; 20],
            origin: 0,
            owners: [0; 2],
            lfo: EffectLfoProgram::default(),
            controller_source: 0,
            controller_value: 0,
            secondary_value: 0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterEffectRack {
    pub instances: [FilterEffectInstance; 8],
    pub caches: [FilterEffectCache; 9],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct FilterParameterEdit {
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    /// The original handler can receive a requested byte distinct from its
    /// current stored snapshot. These are separate declared editor inputs.
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub clock_rate: u32,
}
pub struct FilterParameterTables<'a> {
    pub coefficients: &'a FilterEffectTables,
    pub routing: &'a EffectRoutingTables,
    pub tempo: &'a LfoTempoTables,
    pub lfo_mapping: EffectLfoMapping,
}
pub struct PreparedFilterParameterEdit {
    pub next: FilterEffectRack,
    pub batch: EffectParameterBatch,
}
const MINIMUM: [u8; 16] = [0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0];
const MAXIMUM: [u8; 16] = [
    100, 4, 127, 127, 127, 1, 127, 127, 1, 127, 16, 4, 127, 1, 18, 12,
];
impl FilterEffectRack {
    pub fn prepare(
        &self,
        edit: FilterParameterEdit,
        tables: &FilterParameterTables<'_>,
        midi: &EffectMidiSources,
        program: &Program,
    ) -> Option<PreparedFilterParameterEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        if slot >= 8
            || parameter >= 16
            || !(MINIMUM[parameter]..=MAXIMUM[parameter]).contains(&edit.value)
        {
            return None;
        }
        for i in 0..16 {
            if !(MINIMUM[i]..=MAXIMUM[i]).contains(&edit.parameters[i]) {
                return None;
            }
        }
        let mut next = *self;
        let mut batch = EffectParameterBatch::from_lfo(None);
        next.instances[slot].parameters = edit.parameters;
        next.instances[slot].kind = 4;
        next.instances[slot].origin = edit.origin;
        next.instances[slot].owners = edit.owners;
        let interpolation = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            edit.parameter,
            edit.owners[0],
            edit.owners[1],
            false,
        );
        match edit.parameter {
            0 => {
                let mix = EffectMix::compile(EffectKind::new(4)?, edit.value, Default::default())?;
                batch.push_direct(u32::from(edit.origin), mix.dry as u32)?;
                batch.push_direct(u32::from(edit.origin) + 1, mix.wet as u32)?;
            }
            1 => next.prepare_mode(
                &mut batch,
                slot,
                edit.value,
                edit.direct_switch,
                tables,
                program,
            )?,
            2 | 5 | 6 => {
                next.refresh_frequency_parameter(&mut batch, slot, tables.coefficients, midi)?
            }
            3 => {
                batch.extend(&tables.coefficients.prepare_trim(
                    edit.origin,
                    edit.parameters[3],
                    edit.parameters[4],
                )?)?;
                next.refresh_frequency_parameter(&mut batch, slot, tables.coefficients, midi)?;
            }
            4 => batch.extend(&tables.coefficients.prepare_trim(
                edit.origin,
                edit.parameters[3],
                edit.parameters[4],
            )?)?,
            7 => {
                let prepared = tables.coefficients.prepare_response(
                    &next.assignments,
                    edit.origin,
                    edit.value,
                    interpolation,
                )?;
                batch.extend(&prepared.batch)?;
                next.assignments = prepared.next;
            }
            8..=14 => {
                let publication = next.instances[slot].lfo.prepare(
                    &edit.parameters,
                    tables.lfo_mapping,
                    EffectLfoSlot::new(edit.slot)?,
                    0,
                    edit.clock_rate,
                    tables.tempo,
                )?;
                if let Some(p) = publication {
                    next.instances[slot].lfo = p.program;
                }
                batch.extend(&EffectParameterBatch::from_lfo(publication))?;
            }
            15 => {
                if next.instances[slot].controller_source != u32::from(edit.value) {
                    let source = edit.parameters[15];
                    let value = midi.value((slot / 2) as u8, source)?;
                    next.instances[slot].controller_source = u32::from(source);
                    next.instances[slot].controller_value = value.abs();
                    next.instances[slot].secondary_value = 0;
                    // Complete SYS079702 sweeps both inserts of every timbre.
                    for index in 0..8 {
                        let instance = next.instances[index];
                        if instance.controller_source == 0 {
                            continue;
                        }
                        let value = midi.value(
                            (index / 2) as u8,
                            instance.controller_source.try_into().ok()?,
                        )?;
                        if value == instance.controller_value {
                            continue;
                        }
                        if instance.parameters[5] == 1 {
                            next.frequency(&mut batch, index, value, tables.coefficients)?;
                        }
                        next.instances[index].controller_value = value;
                    }
                }
            }
            _ => return None,
        }
        Some(PreparedFilterParameterEdit { next, batch })
    }
    fn frequency(
        &mut self,
        batch: &mut EffectParameterBatch,
        slot: usize,
        value: i8,
        tables: &FilterEffectTables,
    ) -> Option<()> {
        let instance = self.instances[slot];
        let prepared = tables.prepare_frequency(
            self.caches[slot],
            FilterEffectFrequency {
                origin: instance.origin,
                cutoff: instance.parameters[2],
                resonance: instance.parameters[3],
                modulation_depth: instance.parameters[6],
                modulation: (effect_controller_level(value) >> 8) as i16,
            },
        )?;
        batch.extend(&prepared.batch)?;
        self.caches[slot] = prepared.next;
        Some(())
    }
    fn refresh_frequency_parameter(
        &mut self,
        batch: &mut EffectParameterBatch,
        slot: usize,
        tables: &FilterEffectTables,
        midi: &EffectMidiSources,
    ) -> Option<()> {
        let instance = self.instances[slot];
        if instance.parameters[5] == 1 {
            let value = midi.value(
                (slot / 2) as u8,
                instance.controller_source.try_into().ok()?,
            )?;
            self.frequency(batch, slot, value, tables)?;
        }
        // SYS07771C marks dirty AFTER any immediate modulation calculation.
        self.caches[slot].dirty = 1;
        Some(())
    }
    fn prepare_mode(
        &self,
        batch: &mut EffectParameterBatch,
        slot: usize,
        value: u8,
        direct: u32,
        tables: &FilterParameterTables<'_>,
        program: &Program,
    ) -> Option<()> {
        let instance = self.instances[slot];
        let origin = u32::from(instance.origin);
        if direct == 0 {
            batch.push_direct(origin, 0x7fffff)?;
            batch.push_direct(origin + 1, 0)?;
            for (address, value) in [(0x2fb, 0x2f7), (0x2fa, 0), (0x2fd, 0x2f7), (0x2fc, 0)] {
                batch.push_direct(address, value)?;
            }
            batch.push_command(0, 0x01000014)?;
        }
        let selected = match value {
            0 => 2,
            1 => 1,
            2 => 0,
            3 => 3,
            4 => 4,
            _ => return None,
        };
        for offset in 0..5 {
            batch.push_direct(
                origin + 10 + offset,
                if offset == selected { 0x7fffff } else { 0 },
            )?;
        }
        if direct == 0 && program.timbre(slot / 2)?.effect(slot % 2)?.enabled() {
            batch.push_command(0, 0x01000028)?;
            let mix = EffectMix::compile(
                EffectKind::new(4)?,
                instance.parameters[0],
                Default::default(),
            )?;
            batch.push_direct(origin, mix.dry as u32)?;
            batch.push_direct(origin + 1, mix.wet as u32)?;
            self.restore_routing(batch, slot / 2, tables.routing, program)?;
        }
        Some(())
    }
    fn restore_routing(
        &self,
        batch: &mut EffectParameterBatch,
        part: usize,
        tables: &EffectRoutingTables,
        program: &Program,
    ) -> Option<()> {
        let instances = core::array::from_fn(|i| {
            self.instances
                .get(i)
                .map_or_else(EffectRoutingInstance::default, |instance| {
                    EffectRoutingInstance {
                        kind: instance.kind,
                        parameters: instance.parameters,
                        origin: instance.origin,
                    }
                })
        });
        batch.extend(&tables.prepare(
            program,
            &instances,
            EffectRoutingContext::Insert(part as u8),
            0,
        )?)
    }
}
