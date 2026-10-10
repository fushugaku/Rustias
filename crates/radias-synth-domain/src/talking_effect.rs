//! Complete Talking Modulator insert parameter, program and MIDI controllers.
use crate::{
    effect_control::{
        EffectBank, EffectBufferLayout, EffectKind, EffectLoad, EffectMix, EffectOrigins,
        EffectRequest, MixContext, PreparedEffect,
    },
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_equalizer::multiply,
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    effect_midi::{EffectMidiPolarity, EffectMidiSources, effect_controller_level},
    effect_pair_transition::EffectPairTransitionTables,
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_program_staging::{EffectProgramStaging, EffectStagedProgramWrite},
    effect_routing::{EffectRoutingContext, EffectRoutingInstance, EffectRoutingTables},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    lfo_tempo::LfoTempoTables,
    program::Program,
};
pub struct TalkingTables {
    pub ranges: [EffectParameterRange; 20],
    pub dependencies: [[u32; 2]; 20],
    pub groups: [EffectCoefficientGroup; 63],
    pub voices: [[u32; 8]; 5],
    pub response: [u32; 128],
    pub damping: [u32; 128],
    pub positive_range: EffectParameterRange,
    pub centered_range: EffectParameterRange,
    pub lfo_mapping: EffectLfoMapping,
    pub tempo: LfoTempoTables,
    pub routing: EffectRoutingTables,
    pub pair_transition: EffectPairTransitionTables,
    pub transition_programs: [[u64; 2]; 5],
    pub programs: [[u8; 720]; 2],
    pub program_layout: EffectBufferLayout,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TalkingInstance {
    pub kind: u8,
    pub parameters: [u8; 20],
    pub previous_parameters: [u8; 20],
    pub owners: [u32; 2],
    pub origin: u16,
    pub relocation_origin: u16,
    pub prefix_origin: u16,
    pub body_origin: u16,
    pub lfo: EffectLfoProgram,
    pub controller_source: u32,
    pub controller_value: i8,
    pub secondary_value: i8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TalkingRack {
    pub instances: [TalkingInstance; 8],
    pub assignments: EffectCoefficientAssignments,
    pub staging: EffectProgramStaging,
}
#[derive(Clone, Copy)]
pub struct TalkingEdit {
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub previous_parameters: [u8; 20],
    pub origin: u16,
    pub relocation_origin: u16,
    pub prefix_origin: u16,
    pub body_origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub update_marker: u32,
    pub clock_rate: u32,
}
pub struct PreparedTalkingEdit {
    pub next: TalkingRack,
    pub batch: EffectParameterBatch,
    pub program_writes: [Option<EffectStagedProgramWrite>; 2],
    pub body_program: Option<PreparedEffect>,
}
fn direct(b: &mut EffectParameterBatch, origin: u16, offset: u32, v: u32) -> Option<()> {
    b.push_direct(u32::from(origin) + offset, v)
}
fn assigned(
    r: &mut TalkingRack,
    b: &mut EffectParameterBatch,
    c: EffectInterpolationControl,
    origin: u16,
    index: u32,
    v: u32,
) -> Option<()> {
    let p = r.assignments.prepare(CoefficientChange {
        direct_switch: c.direct_switch,
        standalone: false,
        enabled_argument: c.enabled_argument,
        target: u32::from(origin) + index,
        value: v,
        mode: 0,
    });
    b.append(&p.plan)?;
    r.assignments = p.next;
    Some(())
}
fn routing_instances(r: &TalkingRack) -> [EffectRoutingInstance; 9] {
    core::array::from_fn(|i| {
        if i < 8 {
            let p = r.instances[i];
            EffectRoutingInstance {
                kind: p.kind,
                origin: p.origin,
                parameters: p.parameters,
            }
        } else {
            EffectRoutingInstance::default()
        }
    })
}
impl TalkingTables {
    fn midi(
        &self,
        r: &mut TalkingRack,
        b: &mut EffectParameterBatch,
        midi: &EffectMidiSources,
        polarity: EffectMidiPolarity,
    ) -> Option<()> {
        for slot in 0..8 {
            if r.instances[slot].kind != 30 || (slot % 2 == 1 && r.instances[slot - 1].kind >= 29) {
                continue;
            }
            let mut i = r.instances[slot];
            if i.controller_source == 0 {
                continue;
            }
            let v = midi.value((slot / 2) as u8, i.controller_source.try_into().ok()?)?;
            if v != i.controller_value {
                if i.parameters[7] == 2 {
                    let mut level = effect_controller_level(v);
                    if polarity.bipolar(i.parameters[19]) {
                        let x = level.wrapping_add(0x7fffff);
                        level = x.wrapping_add(i32::from(x < 0)) >> 1;
                    }
                    direct(b, i.origin, 5, level as u32)?;
                }
                i.controller_value = v;
            }
            r.instances[slot] = i;
        }
        Some(())
    }
    pub(crate) fn voice(&self, b: &mut EffectParameterBatch, edit: TalkingEdit) -> Option<()> {
        let p = edit.parameters;
        let width = i32::from(self.positive_range.maximum) - i32::from(self.positive_range.minimum);
        if width == 0 {
            return None;
        }
        let position = i32::from(p[5]) - i32::from(self.positive_range.minimum);
        let gain = 0x7fffffi32
            .wrapping_div(1000)
            .wrapping_mul(
                width
                    .wrapping_mul(920)
                    .wrapping_sub(position.wrapping_mul(900)),
            )
            .wrapping_div(width) as u32;
        let voices = [
            self.voices[usize::from(p[2])],
            self.voices[usize::from(p[3])],
            self.voices[usize::from(p[4])],
        ];
        if edit.parameter == 5 {
            for k in 0..4 {
                for (v, index) in voices.into_iter().zip([26, 34, 42]) {
                    direct(
                        b,
                        edit.origin,
                        index + k as u32,
                        multiply(v[4 + k], gain, 9),
                    )?;
                }
            }
        } else {
            let (v, index) = match edit.parameter {
                3 => (voices[1], 30),
                4 => (voices[2], 38),
                _ => (voices[0], 22),
            };
            for k in 0..4 {
                direct(b, edit.origin, index + k as u32, v[k])?;
                direct(
                    b,
                    edit.origin,
                    index + 4 + k as u32,
                    multiply(v[4 + k], gain, 9),
                )?;
            }
        }
        Some(())
    }
    fn depth(
        &self,
        r: &mut TalkingRack,
        b: &mut EffectParameterBatch,
        edit: TalkingEdit,
        c: EffectInterpolationControl,
    ) -> Option<()> {
        let p = edit.parameters;
        let response = if p[7] == 0 {
            self.positive_range
                .compile(EffectCurve::Linear, i32::from(p[9]), 12800, 256)? as u32
        } else {
            self.response[usize::from(p[9])]
        };
        let value = self.centered_range.compile(
            EffectCurve::OffsetScale,
            i32::from(p[8]),
            response as i32,
            (response as i32).wrapping_neg(),
        )? as u32;
        assigned(r, b, c, edit.origin, 62, value)?;
        if matches!(edit.parameter, 7 | 9) {
            direct(b, edit.origin, 15, response)?;
            direct(b, edit.origin, 16, self.damping[usize::from(p[9])])?;
        }
        Some(())
    }
    fn bypass(
        &self,
        r: &mut TalkingRack,
        b: &mut EffectParameterBatch,
        program: &Program,
        edit: TalkingEdit,
        reset: bool,
    ) -> Option<()> {
        let part = usize::from(edit.slot / 2);
        let timbre = program.timbre(part)?;
        let first = timbre.effect(0)?.kind()?.raw();
        for role in 0..2 {
            if role == 1 && matches!(first, 29 | 30) {
                break;
            }
            let i = r.instances[2 * part + role];
            let mix = if !reset && i.kind != 0 && timbre.effect(role)?.enabled() {
                EffectMix::compile(
                    EffectKind::new(i.kind)?,
                    i.parameters[0],
                    MixContext {
                        byte1: i.parameters[1],
                        byte5: i.parameters[5],
                        byte6: i.parameters[6],
                    },
                )?
            } else {
                EffectMix {
                    dry: 0x7fffff,
                    wet: 0,
                }
            };
            let c = EffectInterpolationControl::from_owners(
                edit.direct_switch,
                0,
                i.owners[0],
                i.owners[1],
                false,
            );
            assigned(r, b, c, i.origin, 0, mix.dry as u32)?;
            assigned(r, b, c, i.origin, 1, mix.wet as u32)?;
        }
        Some(())
    }
    fn mode(
        &self,
        next: &mut TalkingRack,
        b: &mut EffectParameterBatch,
        writes: &mut [Option<EffectStagedProgramWrite>; 2],
        body: &mut Option<PreparedEffect>,
        program: &Program,
        edit: TalkingEdit,
    ) -> Option<()> {
        let p = edit.parameters;
        let previous = edit.previous_parameters[7];
        let changed = p[7] != previous && (p[7] == 0 || previous == 0);
        let part = usize::from(edit.slot / 2);
        let instances = routing_instances(next);
        let pair = [instances[2 * part], instances[2 * part + 1]];
        if changed {
            if edit.direct_switch == 0 {
                self.bypass(next, b, program, edit, true)?;
                b.extend(&self.routing.prepare(
                    program,
                    &instances,
                    EffectRoutingContext::Insert(part as u8),
                    1,
                )?)?;
                b.push_command(0, 0x01000014)?;
                let write = next
                    .staging
                    .store(self.transition_programs[part][0], false)?;
                b.push_command(
                    next.instances[2 * part].prefix_origin,
                    0x02000000 | u32::from(write.selector),
                )?;
                writes[0] = Some(write);
            }
            if edit.direct_switch == 0 || edit.update_marker != 0 {
                let prepared = PreparedEffect::compile(
                    EffectRequest {
                        kind: EffectKind::new(30)?,
                        bank: EffectBank::Insert,
                        load: EffectLoad::Default,
                        work_slot: u16::from(next.staging.cursor),
                        selector_byte: p[7],
                        origins: EffectOrigins {
                            program: edit.body_origin,
                            data: edit.origin,
                            coefficients: edit.relocation_origin,
                        },
                    },
                    &self.programs[usize::from(p[7] != 0)],
                    self.program_layout,
                )
                .ok()?;
                b.push_command(prepared.blocks[0].destination, prepared.blocks[0].tag)?;
                *body = Some(prepared);
            }
        }
        match p[7] {
            1 => {
                direct(b, edit.origin, 19, 0)?;
                direct(b, edit.origin, 20, 0x7fffff)?;
            }
            2 => {
                direct(b, edit.origin, 19, 0x7fffff)?;
                direct(b, edit.origin, 20, 0)?;
            }
            0 => {
                direct(b, edit.origin, 19, 0)?;
                direct(b, edit.origin, 20, 0)?;
                direct(
                    b,
                    edit.origin,
                    17,
                    self.positive_range.compile(
                        EffectCurve::Linear,
                        i32::from(p[10]),
                        0x7fffff,
                        0,
                    )? as u32,
                )?;
                direct(
                    b,
                    edit.origin,
                    61,
                    self.centered_range.compile(
                        EffectCurve::OffsetQuadratic,
                        i32::from(p[10]),
                        0x7fffff,
                        -0x7fffff,
                    )? as u32,
                )?;
            }
            _ => return None,
        }
        if changed && edit.direct_switch == 0 {
            b.extend(&self.pair_transition.prepare(&pair, 1)?)?;
            b.push_command(0, 0x01000028)?;
            let write = next
                .staging
                .store(self.transition_programs[part][1], true)?;
            b.push_command(
                next.instances[2 * part].prefix_origin,
                0x02000000 | u32::from(write.selector),
            )?;
            writes[1] = Some(write);
            b.push_command(0, 0x01000001)?;
            next.staging.advance();
            b.extend(&self.pair_transition.prepare(&pair, 0)?)?;
            b.push_command(0, 0x01000001)?;
            b.extend(&self.routing.prepare(
                program,
                &instances,
                EffectRoutingContext::Insert(part as u8),
                0,
            )?)?;
            self.bypass(next, b, program, edit, false)?;
        }
        Some(())
    }
    pub fn prepare_midi(
        &self,
        rack: &TalkingRack,
        midi: &EffectMidiSources,
        polarity: EffectMidiPolarity,
    ) -> Option<PreparedTalkingEdit> {
        let mut next = *rack;
        let mut batch = EffectParameterBatch::from_lfo(None);
        self.midi(&mut next, &mut batch, midi, polarity)?;
        Some(PreparedTalkingEdit {
            next,
            batch,
            program_writes: [None; 2],
            body_program: None,
        })
    }
    pub fn prepare(
        &self,
        rack: &TalkingRack,
        program: &Program,
        midi: &EffectMidiSources,
        polarity: EffectMidiPolarity,
        edit: TalkingEdit,
    ) -> Option<PreparedTalkingEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        if slot >= 8 || parameter >= 20 {
            return None;
        }
        let valid = |v: u8, r: EffectParameterRange| {
            let x = i32::from(v) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&x)
        };
        if !valid(edit.value, self.ranges[parameter])
            || edit
                .parameters
                .iter()
                .zip(self.ranges)
                .any(|(&v, r)| !valid(v, r))
        {
            return None;
        }
        let mut next = *rack;
        let i = &mut next.instances[slot];
        i.kind = 30;
        i.parameters = edit.parameters;
        i.previous_parameters = edit.previous_parameters;
        i.origin = edit.origin;
        i.relocation_origin = edit.relocation_origin;
        i.prefix_origin = edit.prefix_origin;
        i.body_origin = edit.body_origin;
        i.owners = edit.owners;
        let mut batch = EffectParameterBatch::from_lfo(None);
        let mut program_writes = [None; 2];
        let mut body_program = None;
        let c = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            edit.parameter,
            edit.owners[0],
            edit.owners[1],
            false,
        );
        for (index, g) in self.groups.iter().enumerate() {
            if self.dependencies[parameter][index / 32] & (0x80000000 >> (index % 32)) == 0 {
                continue;
            }
            match g.action {
                0 => {}
                34 => {
                    let p = next.instances[slot].lfo.prepare(
                        &edit.parameters,
                        self.lfo_mapping,
                        EffectLfoSlot::new(edit.slot)?,
                        0,
                        edit.clock_rate,
                        &self.tempo,
                    )?;
                    if let Some(p) = p {
                        next.instances[slot].lfo = p.program;
                    }
                    batch.extend(&EffectParameterBatch::from_lfo(p))?;
                }
                41 => {
                    if next.instances[slot].controller_source != u32::from(edit.value.min(12)) {
                        let v = edit.parameters[19];
                        next.instances[slot].controller_source = u32::from(v);
                        next.instances[slot].secondary_value = 0;
                        next.instances[slot].controller_value = midi.value(edit.slot / 2, v)?.abs();
                        self.midi(&mut next, &mut batch, midi, polarity)?;
                    }
                }
                73 => self.mode(
                    &mut next,
                    &mut batch,
                    &mut program_writes,
                    &mut body_program,
                    program,
                    edit,
                )?,
                74 => self.voice(&mut batch, edit)?,
                75 => self.depth(&mut next, &mut batch, edit, c)?,
                4 | 6 | 9 | 10 | 11 | 12 => {
                    let value = if matches!(g.action, 4 | 6) {
                        let mix = EffectMix::compile(
                            EffectKind::new(30)?,
                            edit.value,
                            Default::default(),
                        )?;
                        if g.action == 6 {
                            mix.dry as u32
                        } else {
                            mix.wet as u32
                        }
                    } else {
                        g.range.compile(
                            match g.action {
                                9 => EffectCurve::Quadratic,
                                10 => EffectCurve::OffsetQuadratic,
                                12 => EffectCurve::OffsetScale,
                                _ => EffectCurve::Linear,
                            },
                            i32::from(edit.value),
                            g.first,
                            g.second,
                        )? as u32
                    };
                    assigned(&mut next, &mut batch, c, edit.origin, index as u32, value)?;
                }
                _ => return None,
            }
        }
        Some(PreparedTalkingEdit {
            next,
            batch,
            program_writes,
            body_program,
        })
    }
}
