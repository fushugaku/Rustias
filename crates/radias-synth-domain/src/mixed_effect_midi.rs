//! Whole SYS079702 mixed Insert/Master MIDI service in original slot order.
use crate::{
    delay_time::DelayClock,
    effect_midi::{EffectMidiPolarity, EffectMidiSources, effect_controller_level},
    effect_parameters::{EffectInterpolationControl, EffectParameterBatch},
    effect_updates::CoefficientChange,
    filter_effect::{FilterEffectCache, FilterEffectFrequency},
    insert_effect_construction::InsertEffectInstance,
    master_effect_construction::MasterEffectInstance,
    master_effect_control::{MasterControlTables, MasterEdit},
    rotary_effect::{RotaryInstance, RotaryRack},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixedEffectMidiState {
    pub inserts: [InsertEffectInstance; 8],
    pub master: MasterEffectInstance,
    pub insert_filter_caches: [FilterEffectCache; 8],
}
#[derive(Clone, Copy)]
pub struct MixedEffectMidiFrame {
    pub midi: EffectMidiSources,
    pub polarity: EffectMidiPolarity,
    pub current_notes: [u8; 5],
    pub master_origin: u16,
    pub direct_switch: u32,
    /// SYS079724 passes one; the ordinary SYS079702 service passes zero.
    pub force_refresh: bool,
}
pub struct PreparedMixedEffectMidi {
    pub next: MixedEffectMidiState,
    pub batch: EffectParameterBatch,
}
fn generic_level(kind: u8, p: &[u8; 20], value: i8, polarity: EffectMidiPolarity) -> Option<u32> {
    let mut level = effect_controller_level(value);
    if kind == 5 && p[4] == 2 {
        if level < 0 && !polarity.bipolar(p[16]) {
            level = level.wrapping_neg();
        }
    } else if kind == 30 && p[7] == 2 {
        if polarity.bipolar(p[19]) {
            let x = level.wrapping_add(0x7fffff);
            level = x.wrapping_add(i32::from(x < 0)) >> 1;
        }
    } else {
        return None;
    }
    Some(level as u32)
}
impl MasterControlTables {
    pub fn prepare_mixed_midi(
        &self,
        state: &MixedEffectMidiState,
        frame: MixedEffectMidiFrame,
    ) -> Option<PreparedMixedEffectMidi> {
        if state.master.kind >= 31
            || state
                .inserts
                .iter()
                .enumerate()
                .any(|(slot, i)| i.slot as usize != slot || i.buffer.kind >= 31)
        {
            return None;
        }
        let mut next = *state;
        let mut batch = EffectParameterBatch::from_lfo(None);
        for slot in 0..8 {
            if slot % 2 == 1 && next.inserts[slot - 1].buffer.kind >= 29 {
                continue;
            }
            let instance = next.inserts[slot];
            let p = instance.buffer.parameters;
            let origin = instance.buffer.origin;
            match instance.buffer.kind {
                29 => {
                    // Restrict the existing qualified Rotary service to this
                    // slot so mixed effect callbacks keep source publication order.
                    let rack = RotaryRack {
                        instances: core::array::from_fn(|index| {
                            let i = next.inserts[index];
                            RotaryInstance {
                                kind: if index == slot { 29 } else { 0 },
                                parameters: i.buffer.parameters,
                                origin: i.buffer.origin,
                                controller_source: i.controller_source,
                                primary: i.controller_values[0],
                                secondary: i.controller_values[1],
                                mode: i.rotary_mode,
                                speed: i.rotary_speed,
                            }
                        }),
                        assignments: next.master.control.assignments,
                    };
                    let prepared = self.rotary.prepare_midi_refresh(
                        &rack,
                        &frame.midi,
                        frame.force_refresh,
                    )?;
                    let i = prepared.next.instances[slot];
                    next.inserts[slot].controller_values = [i.primary, i.secondary];
                    next.inserts[slot].rotary_mode = i.mode;
                    next.inserts[slot].rotary_speed = i.speed;
                    next.master.control.assignments = prepared.next.assignments;
                    batch.extend(&prepared.batch)?;
                }
                25 => {
                    let note = frame.current_notes[slot / 2] & 127;
                    if p[1] == 1 && i32::from(instance.controller_values[0]) != i32::from(note) {
                        let mut control = EffectInterpolationControl::from_owners(
                            frame.direct_switch,
                            2,
                            instance.owners[0],
                            instance.owners[1],
                            false,
                        );
                        if control.enabled_argument == 0 {
                            control = EffectInterpolationControl::from_owners(
                                frame.direct_switch,
                                3,
                                instance.owners[0],
                                instance.owners[1],
                                false,
                            );
                        }
                        let prepared = next.master.control.assignments.prepare(CoefficientChange {
                            direct_switch: frame.direct_switch,
                            standalone: false,
                            enabled_argument: control.enabled_argument,
                            mode: 0,
                            target: u32::from(origin) + 6,
                            value: self.ring.frequency(&p, note)?,
                        });
                        next.master.control.assignments = prepared.next;
                        batch.append(&prepared.plan)?;
                        next.inserts[slot].controller_values[0] = note as i8;
                    }
                }
                kind => {
                    if instance.controller_source == 0 {
                        continue;
                    }
                    let value = frame.midi.value(
                        (slot / 2) as u8,
                        instance.controller_source.try_into().ok()?,
                    )?;
                    if value == instance.controller_values[0] && !frame.force_refresh {
                        continue;
                    }
                    if kind == 4 && p[5] == 1 {
                        let prepared = self.filter.prepare_frequency(
                            next.insert_filter_caches[slot],
                            FilterEffectFrequency {
                                origin,
                                cutoff: p[2],
                                resonance: p[3],
                                modulation_depth: p[6],
                                modulation: (effect_controller_level(value) >> 8) as i16,
                            },
                        )?;
                        next.insert_filter_caches[slot] = prepared.next;
                        batch.extend(&prepared.batch)?;
                    } else if let Some(level) = generic_level(kind, &p, value, frame.polarity) {
                        batch.push_direct(u32::from(origin) + 5, level)?;
                    }
                    next.inserts[slot].controller_values[0] = value;
                }
            }
        }
        let master = next.master;
        let p = master.parameters;
        match master.kind {
            29 => {
                let edit = MasterEdit {
                    kind: 29,
                    parameter: 0,
                    value: 0,
                    parameters: p,
                    previous_parameters: master.previous_parameters,
                    stored_owner: master.control.owner as u8,
                    stored_effect_type: 29,
                    stored_enabled: master.enabled_argument != 0,
                    update_marker: master.control.update_marker,
                    origin: frame.master_origin,
                    owner: master.control.owner,
                    direct_switch: frame.direct_switch,
                    clock_rate: 0,
                    clock: DelayClock {
                        tempo: 0,
                        status: 0,
                    },
                    current_note: frame.current_notes[4],
                    midi: frame.midi,
                    polarity: frame.polarity,
                    prefix_origin: 0,
                    body_origin: 0,
                    relocation_origin: 0,
                    transition_marker: 0,
                };
                let prepared =
                    self.prepare_rotary_midi_refresh(&master.control, edit, frame.force_refresh)?;
                next.master.control = prepared.next;
                batch.extend(&prepared.batch)?;
            }
            25 => {
                let note = frame.current_notes[4];
                if p[1] == 1 && i32::from(master.control.midi_binding.values[0]) != i32::from(note)
                {
                    let mut control = EffectInterpolationControl::from_owners(
                        frame.direct_switch,
                        2,
                        master.control.owner,
                        0,
                        true,
                    );
                    if control.enabled_argument == 0 {
                        control = EffectInterpolationControl::from_owners(
                            frame.direct_switch,
                            3,
                            master.control.owner,
                            0,
                            true,
                        );
                    }
                    let prepared = next.master.control.assignments.prepare(CoefficientChange {
                        direct_switch: frame.direct_switch,
                        standalone: false,
                        enabled_argument: control.enabled_argument,
                        mode: 0,
                        target: u32::from(frame.master_origin) + 6,
                        value: self.ring.frequency_for_global_note(&p, note)?,
                    });
                    next.master.control.assignments = prepared.next;
                    batch.append(&prepared.plan)?;
                    next.master.control.midi_binding.values[0] = note as i8;
                }
            }
            kind => {
                let source = master.control.midi_binding.source;
                if source != 0 {
                    let value = frame.midi.value(4, source.try_into().ok()?)?;
                    if value != master.control.midi_binding.values[0] || frame.force_refresh {
                        if kind == 4 && p[5] == 1 {
                            let prepared = self.filter.prepare_frequency(
                                master.control.filter_cache,
                                FilterEffectFrequency {
                                    origin: frame.master_origin,
                                    cutoff: p[2],
                                    resonance: p[3],
                                    modulation_depth: p[6],
                                    modulation: (effect_controller_level(value) >> 8) as i16,
                                },
                            )?;
                            next.master.control.filter_cache = prepared.next;
                            batch.extend(&prepared.batch)?;
                        } else if let Some(level) = generic_level(kind, &p, value, frame.polarity) {
                            batch.push_direct(u32::from(frame.master_origin) + 5, level)?;
                        }
                        next.master.control.midi_binding.values[0] = value;
                    }
                }
            }
        }
        Some(PreparedMixedEffectMidi { next, batch })
    }
}
