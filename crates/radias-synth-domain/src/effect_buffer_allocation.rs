//! Whole insert buffer relocation, including the selected neighbor callback.
use crate::{
    delay_time::{DelayClock, DelayTimeState, DelayTimeTables, divide_192, encode_delay_frames},
    early_reflect_time::{EarlyReflectTimeEdit, EarlyReflectTimeTables},
    effect_buffers::{
        EffectBufferSlice, EffectBufferTables, PreparedEffectBufferTemplate, effect_uses_buffer,
    },
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_parameters::EffectParameterBatch,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectBufferInstance {
    pub kind: u8,
    pub origin: u16,
    pub parameters: [u8; 20],
    pub buffer_origin: u32,
    pub layout: EffectBufferSlice,
    pub cached_tempo: u16,
    pub ratio: u32,
    pub limited: u32,
    pub pending_coefficients: [u32; 2],
    pub pending_argument: u32,
}
pub struct EffectBufferAllocationTables {
    pub buffers: EffectBufferTables,
    pub time: DelayTimeTables,
    pub early: EarlyReflectTimeTables,
    pub milliseconds: [[u16; 128]; 3],
    pub templates: [[u32; 80]; 31],
    pub reverb_templates: [[u32; 80]; 3],
    pub grain_period: [u32; 128],
    pub clock_notes: [u32; 17],
    pub flanger_response: [u32; 128],
    pub flanger_milliseconds: [u16; 114],
    pub flanger_sync_words: [u32; 128],
}
pub struct PreparedEffectBufferAllocation {
    pub instances: [EffectBufferInstance; 8],
    pub template: PreparedEffectBufferTemplate,
    pub batch: EffectParameterBatch,
}
pub(crate) fn divide_127(value: i32) -> i32 {
    let high = ((i64::from(value) * i64::from(0x81020409u32 as i32)) >> 32) as i32;
    let result = high.wrapping_add(value) >> 6;
    result.wrapping_add(i32::from(result < 0))
}
impl EffectBufferAllocationTables {
    fn coefficients(&self, instance: &EffectBufferInstance) -> Option<&[u32; 80]> {
        if instance.kind == 11 {
            self.reverb_templates
                .get(usize::from(instance.parameters[1]))
        } else {
            self.templates.get(usize::from(instance.kind))
        }
    }
    fn neighbor(
        &self,
        instance: &mut EffectBufferInstance,
        mut layout: EffectBufferSlice,
        clock: DelayClock,
        batch: &mut EffectParameterBatch,
    ) -> Option<()> {
        let kind = instance.kind;
        let p = instance.parameters;
        let origin = u32::from(instance.origin);
        let base = layout.offset.wrapping_add(instance.buffer_origin);
        let template = self.coefficients(instance)?;
        match kind {
            8 => batch.push_direct(origin + 6, base)?,
            11 => {
                for (i, &v) in template[6..30].iter().enumerate() {
                    batch.push_direct(origin + 6 + i as u32, v.wrapping_add(base))?;
                }
            }
            12 => {
                batch.push_direct(origin + 33, base)?;
                batch.extend(&self.early.prepare(EarlyReflectTimeEdit {
                    origin: instance.origin,
                    buffer_origin: base,
                    size: p[2],
                    pre_delay: p[3],
                })?)?;
                for (i, &v) in template[57..61].iter().enumerate() {
                    batch.push_direct(origin + 57 + i as u32, v.wrapping_add(base))?;
                }
            }
            13..=20 | 27 => {
                let (pointer_offsets, time_offsets, shift) = match kind {
                    13 => ([5, 5], [6, 7], 6),
                    14..=16 => ([7, 9], [8, 10], if kind == 15 { 6 } else { 7 }),
                    17..=20 => (
                        [6, 8],
                        [7, if kind == 19 { 8 } else { 9 }],
                        if matches!(kind, 17 | 19) { 6 } else { 7 },
                    ),
                    _ => ([5, 7], [6, 8], 7),
                };
                if kind == 17 {
                    instance.layout.frames = instance.layout.frames.wrapping_sub(1920);
                }
                if kind == 20 {
                    layout.frames = layout.frames.wrapping_sub(2880);
                }
                batch.push_direct(origin + pointer_offsets[0], base)?;
                if kind != 13 && kind != 19 {
                    batch.push_direct(
                        origin + pointer_offsets[1],
                        base.wrapping_add(if matches!(kind, 14 | 16 | 18 | 20 | 27) {
                            layout.frames >> 1
                        } else {
                            0
                        }),
                    )?;
                }
                if matches!(kind, 18 | 19) {
                    layout.frames = layout
                        .frames
                        .wrapping_sub(if kind == 18 { 3840 } else { 960 });
                }
                let mut state = DelayTimeState {
                    cached_tempo: instance.cached_tempo,
                    capacity: layout.frames,
                    ratio: instance.ratio,
                    limited: instance.limited,
                };
                let prepared = if kind == 13 {
                    self.time.lcr(&p, state, clock)?
                } else if kind == 14 {
                    self.time.stereo(&p, state, clock)?
                } else {
                    let mut mapped = [0; 20];
                    mapped[2..8].copy_from_slice(&p[1..7]);
                    if kind == 20 {
                        mapped[2] = 0;
                        mapped[3] = 64;
                        mapped[4] = p[4];
                        mapped[5] = p[5];
                        mapped[6] = 0;
                        mapped[7] = 0;
                    }
                    if kind == 27 {
                        mapped[4] = p[3];
                        mapped[5] = p[3];
                        mapped[6] = p[4];
                        mapped[7] = p[4];
                    }
                    let capacity = if matches!(kind, 16 | 18 | 20 | 27) {
                        layout.frames >> 1
                    } else {
                        layout.frames
                    };
                    let ms = match kind {
                        15 => &self.time.lcr_milliseconds,
                        16 | 27 => &self.time.stereo_milliseconds,
                        17 | 19 => &self.milliseconds[0],
                        18 => &self.milliseconds[1],
                        _ => &self.milliseconds[2],
                    };
                    self.time.two_channel_scaled(
                        &mapped,
                        state,
                        clock,
                        capacity,
                        ms,
                        if kind == 20 { 10 } else { 1 },
                    )?
                };
                state = prepared.state;
                instance.cached_tempo = state.cached_tempo;
                instance.ratio = state.ratio;
                instance.limited = state.limited;
                if matches!(kind, 14 | 16 | 18) {
                    batch.push_direct(
                        origin
                            + match kind {
                                14 => 29,
                                16 => 32,
                                _ => 28,
                            },
                        self.time.feedback_limit(
                            prepared.frames[0],
                            prepared.frames[1],
                            p[if kind == 14 { 8 } else { 7 }],
                        )?,
                    )?;
                }
                if kind == 27 {
                    let period = if p[5] == 0 {
                        *self.grain_period.get(usize::from(p[6]))?
                    } else {
                        let tempo = if p[1] == 0 {
                            u32::from(clock.tempo)
                        } else {
                            u32::from(state.cached_tempo)
                        };
                        if tempo == 0 {
                            return None;
                        }
                        divide_192(
                            (600000 / tempo)
                                .wrapping_mul(*self.clock_notes.get(usize::from(p[7]))?)
                                .wrapping_mul(48),
                        )
                    };
                    instance.pending_coefficients = core::array::from_fn(|i| {
                        (encode_delay_frames(prepared.frames[i].min(period), 7).max(0x1700)
                            & 0xffffff)
                            | 0x80000000
                    });
                    instance.pending_argument = 0;
                } else if kind == 13 {
                    for (i, &frame) in prepared.frames.iter().enumerate() {
                        batch.push_direct(origin + 6 + i as u32, encode_delay_frames(frame, 6))?;
                    }
                } else {
                    for (i, offset) in time_offsets.into_iter().enumerate() {
                        batch.push_direct(
                            u32::from((origin + offset) as u16),
                            encode_delay_frames(prepared.frames[i], shift),
                        )?;
                    }
                }
            }
            21 => {
                for i in [13, 14, 15, 19] {
                    batch.push_direct(origin + i as u32, template[i].wrapping_add(base))?;
                }
            }
            22 => {
                let response = if p[1] == 0 {
                    let r = EffectParameterRange {
                        minimum: 0,
                        maximum: 127,
                        encoded_zero: 0,
                    }
                    .compile(
                        EffectCurve::Quadratic,
                        i32::from(p[5]),
                        0x7fffff,
                        0,
                    )?;
                    if p[6] == 1 { r.wrapping_neg() } else { r }
                } else {
                    let center = (i32::from(p[5]) - divide_127((127 - i32::from(p[3])) * 120))
                        .clamp(-127, 127);
                    let index = divide_127(center.wrapping_mul(63).wrapping_add(8128));
                    divide_127(
                        (self.flanger_response.get(index as usize).copied()? as i32)
                            .wrapping_mul((i32::from(p[5]) * 8).clamp(0, 127)),
                    )
                };
                let time = if p[1] == 0 {
                    encode_delay_frames(
                        u32::from(*self.flanger_milliseconds.get(usize::from(p[2].min(113)))?) * 48
                            / 10,
                        7,
                    )
                } else {
                    *self.flanger_sync_words.get(usize::from(p[3]))?
                };
                batch.push_direct(origin + 10, response as u32)?;
                batch.push_direct(origin + 7, time)?;
                batch.push_direct(origin + 9, time)?;
                batch.push_direct(origin + 6, template[6].wrapping_add(base))?;
                batch.push_direct(origin + 8, template[8].wrapping_add(base))?;
            }
            26 => batch.push_direct(origin + 22, template[22].wrapping_add(base))?,
            28 => {
                batch.push_direct(origin + 6, base)?;
                batch.push_direct(origin + 8, base.wrapping_add(layout.frames >> 1))?;
            }
            29 => {
                for i in [41, 42, 49, 50, 52] {
                    batch.push_direct(origin + i as u32, template[i].wrapping_add(base))?;
                }
            }
            _ => {}
        }
        Some(())
    }
    pub fn prepare(
        &self,
        instances: &[EffectBufferInstance; 8],
        slot: u8,
        input: &[u32],
        clock: DelayClock,
    ) -> Option<PreparedEffectBufferAllocation> {
        let index = usize::from(slot);
        if index >= 8 || input.len() > 80 {
            return None;
        }
        let other = index ^ 1;
        if instances[index].kind >= 31 || instances[other].kind >= 31 {
            return None;
        }
        let mut next = *instances;
        let mut template = PreparedEffectBufferTemplate {
            layout: instances[index].layout,
            words: [0; 80],
            count: input.len(),
        };
        template.words[..input.len()].copy_from_slice(input);
        let mut batch = EffectParameterBatch::from_lfo(None);
        if effect_uses_buffer(u32::from(next[index].kind))
            || effect_uses_buffer(u32::from(next[other].kind))
        {
            let pair = if index % 2 == 0 {
                self.buffers.pair(next[index].kind, next[other].kind)?
            } else {
                self.buffers.pair(next[other].kind, next[index].kind)?
            };
            let current_layout = pair[index % 2];
            let other_layout = pair[other % 2];
            template = self.buffers.relocate_template(
                next[index].kind,
                current_layout,
                next[index].buffer_origin,
                input,
            )?;
            if effect_uses_buffer(u32::from(next[other].kind)) {
                next[other].layout = other_layout;
                self.neighbor(&mut next[other], other_layout, clock, &mut batch)?;
            }
            next[index].layout = template.layout;
        }
        Some(PreparedEffectBufferAllocation {
            instances: next,
            template,
            batch,
        })
    }
}
