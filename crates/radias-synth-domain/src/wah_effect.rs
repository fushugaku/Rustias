//! Original St.Wah insert dependency compiler, including complete SYS handlers.
use crate::{
    effect_control::{EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_equalizer::multiply,
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    effect_midi::{EffectMidiSources, effect_controller_level},
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_routing::{EffectRoutingContext, EffectRoutingInstance, EffectRoutingTables},
    effect_setters::EffectSelectorWrites,
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    lfo_tempo::LfoTempoTables,
    program::Program,
};
pub struct WahEffectTables {
    pub ranges: [EffectParameterRange; 17],
    pub dependencies: [[u32; 2]; 17],
    pub groups: [EffectCoefficientGroup; 56],
    pub mode: [u32; 6],
    pub resonance_bound: [u32; 6],
    pub frequency: [[u32; 6]; 127],
    pub resonance: [[u32; 6]; 127],
    pub frequency_modulation: [u32; 127],
    pub response: [u32; 128],
    pub response_complement: [u32; 128],
    pub free_rate: [u8; 128],
    pub sync_divisors: [u32; 17],
    pub rate_coefficients: [[u16; 2]; 101],
    pub lfo_mapping: EffectLfoMapping,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WahEffectInstance {
    pub kind: u8,
    pub parameters: [u8; 20],
    pub previous_parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub lfo: EffectLfoProgram,
    pub controller_source: u32,
    pub controller_value: i8,
    pub secondary_value: i8,
}
impl Default for WahEffectInstance {
    fn default() -> Self {
        Self {
            kind: 5,
            parameters: [0; 20],
            previous_parameters: [0; 20],
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
pub struct WahEffectRack {
    pub instances: [WahEffectInstance; 8],
    pub assignments: EffectCoefficientAssignments,
}
pub use crate::effect_midi::EffectMidiPolarity;
#[derive(Clone, Copy)]
pub struct WahParameterEdit {
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub previous_parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub clock_rate: u32,
    pub tempo: u16,
}
pub struct WahParameterTables<'a> {
    pub coefficients: &'a WahEffectTables,
    pub routing: &'a EffectRoutingTables,
    pub tempo: &'a LfoTempoTables,
    pub midi: &'a EffectMidiSources,
    pub polarity: EffectMidiPolarity,
}
pub struct PreparedWahEdit {
    pub next: WahEffectRack,
    pub batch: EffectParameterBatch,
}
impl WahEffectTables {
    pub(crate) fn controller_polarity_word(
        parameters: &[u8; 20],
        polarity: EffectMidiPolarity,
    ) -> u32 {
        if parameters[4] == 1 || (parameters[4] == 2 && polarity.bipolar(parameters[16])) {
            if parameters[5] >= 64 {
                0x7fffff
            } else {
                0xff800001
            }
        } else {
            0
        }
    }
    fn modulation(&self, value: u8) -> Option<i32> {
        self.ranges[5].compile(
            EffectCurve::OffsetScale,
            i32::from(value),
            0x7fffff,
            -0x7fffff,
        )
    }
    fn assigned(
        rack: &mut WahEffectRack,
        batch: &mut EffectParameterBatch,
        origin: u16,
        offset: u32,
        value: u32,
        control: EffectInterpolationControl,
    ) -> Option<()> {
        let next = rack.assignments.prepare(CoefficientChange {
            direct_switch: control.direct_switch,
            standalone: false,
            enabled_argument: control.enabled_argument,
            mode: 0,
            target: u32::from(origin) + offset,
            value,
        });
        batch.append(&next.plan)?;
        rack.assignments = next.next;
        Some(())
    }
    pub(crate) fn frequency_mod(&self, parameters: &[u8; 20]) -> Option<u32> {
        let modulation = self.modulation(parameters[5])?;
        let magnitude = modulation.unsigned_abs();
        let value = multiply(
            multiply(magnitude, magnitude, 9),
            *self
                .frequency_modulation
                .get(usize::from(parameters[2].checked_sub(1)?))?,
            9,
        );
        Some(if modulation < 0 {
            value.wrapping_neg()
        } else {
            value
        })
    }
    pub(crate) fn resonance_mod(&self, parameters: &[u8; 20]) -> Option<u32> {
        let modulation = self.modulation(parameters[5])?;
        let value = multiply(0x3fffff, modulation.unsigned_abs() << 1, 9);
        let value = if (value as i32) > 0x7fffff {
            0x7fffff
        } else {
            value
        };
        Some(if modulation < 0 {
            value.wrapping_neg()
        } else {
            value
        })
    }
    pub(crate) fn bound(&self, parameters: &[u8; 20], mode: u8, unsigned: bool) -> Option<u32> {
        let mut value = *self.resonance_bound.get(usize::from(mode))?;
        if parameters[5] < 64 {
            let resonance =
                self.resonance[usize::from(parameters[3].checked_sub(1)?)][usize::from(mode)];
            let ceiling = 0x7f9d53u32.wrapping_sub(resonance);
            if if unsigned {
                value > ceiling
            } else {
                (value as i32) > (ceiling as i32)
            } {
                value = if (ceiling as i32) > 0 { ceiling } else { 0 };
            }
        }
        Some(value)
    }
    fn routing_instances(rack: &WahEffectRack) -> [EffectRoutingInstance; 9] {
        core::array::from_fn(|i| {
            rack.instances
                .get(i)
                .map_or_else(EffectRoutingInstance::default, |x| EffectRoutingInstance {
                    kind: x.kind,
                    origin: x.origin,
                    parameters: x.parameters,
                })
        })
    }
    fn mode(
        &self,
        rack: &mut WahEffectRack,
        batch: &mut EffectParameterBatch,
        edit: WahParameterEdit,
        tables: &WahParameterTables<'_>,
        program: &Program,
    ) -> Option<()> {
        let instance = rack.instances[usize::from(edit.slot)];
        let origin = u32::from(instance.origin);
        let context = EffectRoutingContext::Insert(edit.slot / 2);
        let instances = Self::routing_instances(rack);
        if edit.direct_switch == 0 {
            batch.push_direct(origin, 0x7fffff)?;
            batch.push_direct(origin + 1, 0)?;
            batch.extend(&tables.routing.prepare(program, &instances, context, 1)?)?;
            batch.push_command(0, 0x01000014)?;
        }
        let mode = usize::from(edit.value);
        batch.push_direct(origin + 33, *self.mode.get(mode)?)?;
        batch.push_direct(
            origin + 34,
            self.frequency[usize::from(instance.parameters[2].checked_sub(1)?)][mode],
        )?;
        batch.push_direct(
            origin + 35,
            self.bound(&instance.parameters, edit.value, true)?,
        )?;
        batch.push_direct(
            origin + 36,
            self.resonance[usize::from(instance.parameters[3].checked_sub(1)?)][mode],
        )?;
        if edit.direct_switch == 0
            && program
                .timbre(usize::from(edit.slot / 2))?
                .effect(usize::from(edit.slot % 2))?
                .enabled()
        {
            batch.push_command(0, 0x01000028)?;
            let mix = EffectMix::compile(
                EffectKind::new(5)?,
                instance.parameters[0],
                Default::default(),
            )?;
            batch.push_direct(origin, mix.dry as u32)?;
            batch.push_direct(origin + 1, mix.wet as u32)?;
            batch.extend(&tables.routing.prepare(program, &instances, context, 0)?)?;
        }
        Some(())
    }
    fn midi_sweep(
        rack: &mut WahEffectRack,
        batch: &mut EffectParameterBatch,
        tables: &WahParameterTables<'_>,
    ) -> Option<()> {
        for slot in 0..8 {
            let instance = &mut rack.instances[slot];
            if instance.controller_source == 0 {
                continue;
            }
            let source = instance.controller_source.try_into().ok()?;
            let value = tables.midi.value((slot / 2) as u8, source)?;
            if value == instance.controller_value {
                continue;
            }
            if instance.parameters[4] == 2 {
                let mut level = effect_controller_level(value);
                if level < 0 && !tables.polarity.bipolar(instance.parameters[16]) {
                    level = level.wrapping_neg();
                }
                batch.push_direct(u32::from(instance.origin) + 5, level as u32)?;
            }
            instance.controller_value = value;
        }
        Some(())
    }
    pub fn prepare(
        &self,
        rack: &WahEffectRack,
        edit: WahParameterEdit,
        tables: &WahParameterTables<'_>,
        program: &Program,
    ) -> Option<PreparedWahEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        if slot >= 8 || parameter >= 17 {
            return None;
        }
        let valid = |v: u8, r: EffectParameterRange| {
            let v = i32::from(v) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&v)
        };
        if !valid(edit.value, self.ranges[parameter])
            || edit
                .parameters
                .iter()
                .zip(&self.ranges)
                .any(|(&v, &r)| !valid(v, r))
        {
            return None;
        }
        let mut next = *rack;
        next.instances[slot].parameters = edit.parameters;
        next.instances[slot].kind = 5;
        next.instances[slot].previous_parameters = edit.previous_parameters;
        next.instances[slot].origin = edit.origin;
        next.instances[slot].owners = edit.owners;
        let mut batch = EffectParameterBatch::from_lfo(None);
        for index in 0..56 {
            if self.dependencies[parameter][index / 32] & (0x80000000 >> (index % 32)) == 0 {
                continue;
            }
            let instance = next.instances[slot];
            let p = &instance.parameters;
            let origin = u32::from(edit.origin);
            let control = EffectInterpolationControl::from_owners(
                edit.direct_switch,
                edit.parameter,
                instance.owners[0],
                instance.owners[1],
                false,
            );
            let group = &self.groups[index];
            match group.action {
                4 | 6 => {
                    let mix =
                        EffectMix::compile(EffectKind::new(5)?, edit.value, Default::default())?;
                    Self::assigned(
                        &mut next,
                        &mut batch,
                        edit.origin,
                        index as u32,
                        if group.action == 6 {
                            mix.dry as u32
                        } else {
                            mix.wet as u32
                        },
                        control,
                    )?;
                }
                10 | 11 => {
                    let curve = if group.action == 10 {
                        EffectCurve::OffsetQuadratic
                    } else {
                        EffectCurve::Linear
                    };
                    let value = group.range.compile(
                        curve,
                        i32::from(edit.value),
                        group.first,
                        group.second,
                    )?;
                    Self::assigned(
                        &mut next,
                        &mut batch,
                        edit.origin,
                        index as u32,
                        value as u32,
                        control,
                    )?;
                }
                25 => self.mode(&mut next, &mut batch, edit, tables, program)?,
                34 => {
                    let publication = instance.lfo.prepare(
                        p,
                        self.lfo_mapping,
                        EffectLfoSlot::new(edit.slot)?,
                        0,
                        edit.clock_rate,
                        tables.tempo,
                    )?;
                    if let Some(value) = publication {
                        next.instances[slot].lfo = value.program;
                    }
                    batch.extend(&EffectParameterBatch::from_lfo(publication))?;
                }
                41 => {
                    if next.instances[slot].controller_source != u32::from(edit.value) {
                        let source = p[16];
                        next.instances[slot].controller_source = u32::from(source);
                        next.instances[slot].controller_value =
                            tables.midi.value((slot / 2) as u8, source)?.abs();
                        next.instances[slot].secondary_value = 0;
                        Self::midi_sweep(&mut next, &mut batch, tables)?;
                    }
                }
                47 => {
                    let writes = EffectSelectorWrites::compile(
                        edit.origin,
                        index as u32,
                        u32::from(edit.value),
                    );
                    for word in &writes.words[..usize::from(writes.count)] {
                        batch.push_command(word.address, word.tagged_value)?;
                    }
                }
                48 => {
                    let value = if p[4] == 0 {
                        self.ranges[6].compile(
                            EffectCurve::Linear,
                            i32::from(p[6]),
                            0x3200,
                            0x100,
                        )? as u32
                    } else {
                        self.response[usize::from(p[6])]
                    };
                    Self::assigned(&mut next, &mut batch, edit.origin, 20, value, control)?;
                    Self::assigned(
                        &mut next,
                        &mut batch,
                        edit.origin,
                        24,
                        self.response_complement[usize::from(p[6])],
                        control,
                    )?;
                }
                49 => {
                    Self::assigned(
                        &mut next,
                        &mut batch,
                        edit.origin,
                        51,
                        self.frequency_mod(p)?,
                        control,
                    )?;
                    batch.push_direct(
                        origin + 34,
                        self.frequency[usize::from(p[2] - 1)][usize::from(p[1])],
                    )?;
                }
                50 => {
                    Self::assigned(
                        &mut next,
                        &mut batch,
                        edit.origin,
                        46,
                        self.resonance_mod(p)?,
                        control,
                    )?;
                    batch.push_direct(origin + 35, self.bound(p, p[1], false)?)?;
                    batch.push_direct(
                        origin + 36,
                        self.resonance[usize::from(p[3] - 1)][usize::from(p[1])],
                    )?;
                }
                51 => {
                    let first = if control.enabled_argument == 0 {
                        EffectInterpolationControl::from_owners(
                            edit.direct_switch,
                            2,
                            instance.owners[0],
                            instance.owners[1],
                            false,
                        )
                    } else {
                        control
                    };
                    let second = if control.enabled_argument == 0 {
                        EffectInterpolationControl::from_owners(
                            edit.direct_switch,
                            3,
                            instance.owners[0],
                            instance.owners[1],
                            false,
                        )
                    } else {
                        control
                    };
                    Self::assigned(
                        &mut next,
                        &mut batch,
                        edit.origin,
                        51,
                        self.frequency_mod(p)?,
                        first,
                    )?;
                    Self::assigned(
                        &mut next,
                        &mut batch,
                        edit.origin,
                        46,
                        self.resonance_mod(p)?,
                        second,
                    )?;
                    if (i32::from(instance.previous_parameters[5]) - 64) * (i32::from(p[5]) - 64)
                        <= 0
                    {
                        batch.push_direct(origin + 35, self.bound(p, p[1], false)?)?;
                    }
                }
                52 => {
                    let bucket = if p[4] != 1 {
                        0
                    } else if p[9] == 0 {
                        usize::from(self.free_rate[usize::from(p[10])])
                    } else {
                        let rate = u32::from(edit.tempo).wrapping_mul(192)
                            / self.sync_divisors[usize::from(p[11])];
                        (((u64::from(rate) * 0x1b4e81b5) >> 32) >> 8) as usize
                    };
                    let coefficients = self.rate_coefficients.get(bucket)?;
                    batch.push_direct(origin + 53, u32::from(coefficients[0]))?;
                    batch.push_direct(origin + 54, u32::from(coefficients[1]))?;
                }
                53 => {
                    let value = Self::controller_polarity_word(p, tables.polarity);
                    batch.push_direct(origin + 55, value)?;
                }
                _ => return None,
            }
        }
        Some(PreparedWahEdit { next, batch })
    }
}
