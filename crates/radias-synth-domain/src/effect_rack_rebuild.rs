//! SYS07E4AC effect-rack reconstruction, including ordered queue transitions.
//! Timbre voice-output changes are explicit cross-domain steps for the adapter.
use crate::{
    effect_control::{EffectKind, EffectMix, MixContext},
    effect_equalizer::EffectEqualizerTables,
    effect_pair_transition::EffectPairTransitionTables,
    effect_parameters::EffectParameterBatch,
    effect_program_staging::EffectStagedProgramWrite,
    effect_rack_initialization::{EffectRackInitializationContext, EffectRackInitializationTables},
    effect_routing::EffectRoutingInstance,
    effect_transition_queue::EffectTransitionQueueState,
    effect_updates::UNASSIGNED_TARGET,
    insert_effect_construction::{InsertConstructionTables, InsertPatch},
    insert_effect_control::{InsertControlContext, InsertControlState, InsertControlStep},
    master_effect_construction::{MasterConstruction, MasterPatch},
    mixed_effect_midi::MixedEffectMidiFrame,
};

pub struct EffectRackRebuildTables {
    pub rack: EffectRackInitializationTables,
    pub constructors: InsertConstructionTables,
    pub pair: EffectPairTransitionTables,
    pub equalizer: EffectEqualizerTables,
    pub prefixes: [u64; 5],
    pub tails: [u64; 5],
    pub master_transition_offsets: [[u8; 2]; 31],
    pub temporary_indices: [[u16; 4]; 9],
}
// Fixed storage keeps the domain allocation-free, including complete program words.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectRackRebuildStep {
    Effect(InsertControlStep),
    /// SYS006078/006084: the synthesis bounded context owns active voice work.
    TimbreOutput {
        part: u8,
        alternate: bool,
    },
}
#[derive(Clone, Copy)]
pub struct EffectRackRebuildContext<'a> {
    pub rack: EffectRackInitializationContext<'a>,
    pub raw_tempo: u16,
    pub queue: EffectTransitionQueueState,
}
pub struct PreparedEffectRackRebuild {
    pub next: InsertControlState,
    pub tempo: u16,
    pub direct_switch: u32,
    steps: [Option<EffectRackRebuildStep>; 256],
    count: usize,
}
impl PreparedEffectRackRebuild {
    pub fn steps(&self) -> impl Iterator<Item = &EffectRackRebuildStep> {
        self.steps[..self.count].iter().flatten()
    }
    fn push(&mut self, step: EffectRackRebuildStep) -> Option<()> {
        *self.steps.get_mut(self.count)? = Some(step);
        self.count += 1;
        Some(())
    }
    fn batch(&mut self, batch: EffectParameterBatch) -> Option<()> {
        self.push(EffectRackRebuildStep::Effect(InsertControlStep {
            batch,
            program_writes: [None; 2],
            body_program: None,
        }))
    }
    fn wait(&mut self, ticks: u16) -> Option<()> {
        let mut batch = EffectParameterBatch::from_lfo(None);
        batch.push_command(0, 0x01000000 | u32::from(ticks))?;
        self.batch(batch)
    }
}
impl EffectRackRebuildTables {
    pub fn prepare(
        &self,
        state: &InsertControlState,
        context: EffectRackRebuildContext<'_>,
    ) -> Option<PreparedEffectRackRebuild> {
        let tempo = context.raw_tempo.clamp(200, 3000);
        let mut common = context.rack.common;
        common.clock.tempo = tempo;
        let mut plan = PreparedEffectRackRebuild {
            next: *state,
            tempo,
            direct_switch: 0,
            steps: [None; 256],
            count: 0,
        };
        let mut start = EffectParameterBatch::from_lfo(None);
        start.push_command(0, 0x03000000)?;
        plan.batch(start)?;
        // These eight EQ publications precede the forced direct switch.
        for part in 0..4 {
            let p = common.program.timbre(part)?.bytes();
            for (frequency, gain, target, high) in [
                (
                    p[168],
                    (i16::from(p[169]) - 64 + 6) as i8,
                    8 * part + 2,
                    false,
                ),
                (
                    p[170].wrapping_add(33),
                    (i16::from(p[171]) - 64 + 6) as i8,
                    8 * part + 5,
                    true,
                ),
            ] {
                let words = if high {
                    self.equalizer.high_shelf(frequency, gain)?
                } else {
                    self.equalizer.low_shelf(frequency, gain)?
                };
                let mut batch = EffectParameterBatch::from_lfo(None);
                for (i, value) in words.into_iter().enumerate() {
                    batch.push_command(
                        (target + i) as u16,
                        (value & 0xffffff)
                            | if common.midi.direct_switch == 0 {
                                (0x83 - i as u32) << 24
                            } else {
                                0
                            },
                    )?;
                }
                plan.batch(batch)?;
            }
        }

        for part in 0..4 {
            let p = common.program.timbre(part)?.bytes();
            let value = u32::from(p[2]);
            // Whole SYS077514/07754C crossfade, split at (127+1)/2.
            let first = if value <= 64 {
                0x7fffff
            } else {
                0x7fffffu32.wrapping_mul(127u32.wrapping_sub(value)) / 63
            };
            let second = if value == 0 {
                0
            } else if value <= 64 {
                0x7fffffu32.wrapping_mul(value - 1) / 63
            } else {
                0x7fffff
            };
            let alternate = p[0] & 0x30 != 0;
            let mut batch = EffectParameterBatch::from_lfo(None);
            if !alternate {
                batch.push_direct((36 + 3 * part) as u32, first)?;
                batch.push_direct((37 + 3 * part) as u32, second)?;
                batch.push_direct((38 + 3 * part) as u32, 0)?;
                plan.batch(batch)?;
                plan.push(EffectRackRebuildStep::TimbreOutput {
                    part: part as u8,
                    alternate,
                })?;
            } else {
                plan.push(EffectRackRebuildStep::TimbreOutput {
                    part: part as u8,
                    alternate,
                })?;
                batch.push_direct((36 + 3 * part) as u32, first)?;
                batch.push_direct((37 + 3 * part) as u32, 0)?;
                batch.push_direct((38 + 3 * part) as u32, second)?;
                plan.batch(batch)?;
            }
        }
        common.midi.direct_switch = 1;

        self.mix_all(&mut plan, common, true)?;
        self.mix_master(&mut plan, common, true)?;

        for slot in 0..8 {
            let start = 168 + slot / 2 * 228 + slot % 2 * 24;
            let raw = &common.program.bytes()[start..start + 24];
            plan.next.midi.inserts[slot] = self.constructors.construct_stored(
                &plan.next.midi.inserts[slot],
                InsertPatch {
                    header: raw[..4].try_into().ok()?,
                    parameters: raw[4..].try_into().ok()?,
                },
                raw[0] & 127,
            )?;
        }
        let raw = &common.program.bytes()[1038..1060];
        plan.next.midi.master = self
            .rack
            .control
            .common
            .construct_master(
                &plan.next.midi.master,
                MasterPatch {
                    header: raw[..2].try_into().ok()?,
                    parameters: raw[2..].try_into().ok()?,
                },
                raw[0] & 127,
                MasterConstruction::StoredWithRawHistory,
            )?
            .next;

        if context.queue.control & 1 == (context.queue.control >> 1) & 1 {
            let batch = self
                .rack
                .control
                .common
                .release_master_assignments(&mut plan.next.midi.master.control, 1)?;
            plan.batch(batch)?;
        } else {
            let mut batch = EffectParameterBatch::from_lfo(None);
            for record in plan.next.midi.master.control.assignments.slots {
                for (i, value) in [record.target, record.last_value, 0x7a9765, 0x5689a]
                    .into_iter()
                    .enumerate()
                {
                    batch.push_direct(u32::from(record.indices[i]), value)?;
                }
            }
            plan.batch(batch)?;
        }

        self.routing_transition(&mut plan, common, context.rack, true)?;

        let rack = self.rack.prepare(
            &plan.next,
            EffectRackInitializationContext {
                common,
                ..context.rack
            },
        )?;
        plan.next = rack.next;
        for &step in rack.steps() {
            plan.push(EffectRackRebuildStep::Effect(step))?;
        }

        self.routing_transition(&mut plan, common, context.rack, false)?;

        self.mix_all(&mut plan, common, false)?;
        self.mix_master(&mut plan, common, !common.program.master_effect().enabled())?;

        self.temporary_routes(&mut plan, common)?;
        let mut end = EffectParameterBatch::from_lfo(None);
        end.push_command(0, 0x04000000)?;
        plan.batch(end)?;

        let midi = self.rack.control.common.prepare_mixed_midi(
            &plan.next.midi,
            MixedEffectMidiFrame {
                direct_switch: 0,
                force_refresh: true,
                ..common.midi
            },
        )?;
        plan.next.midi = midi.next;
        plan.batch(midi.batch)?;
        Some(plan)
    }
    fn mix_all(
        &self,
        plan: &mut PreparedEffectRackRebuild,
        common: InsertControlContext<'_>,
        bypass: bool,
    ) -> Option<()> {
        for part in 0..4 {
            let first = plan.next.midi.inserts[part * 2];
            let mut batch = EffectParameterBatch::from_lfo(None);
            for role in 0..2 {
                if role == 1
                    && *self
                        .rack
                        .occupies_pair
                        .get(usize::from(first.buffer.kind))?
                {
                    break;
                }
                let i = plan.next.midi.inserts[part * 2 + role];
                let p = i.buffer.parameters;
                let mix = if bypass
                    || i.buffer.kind == 0
                    || !common.program.timbre(part)?.effect(role)?.enabled()
                {
                    [0x7fffff, 0]
                } else {
                    EffectMix::compile(
                        EffectKind::new(i.buffer.kind)?,
                        p[0],
                        MixContext {
                            byte1: p[1],
                            byte5: p[5],
                            byte6: p[6],
                        },
                    )?
                    .host_words()
                };
                for (offset, value) in mix.into_iter().enumerate() {
                    batch.push_direct(u32::from(i.buffer.origin) + offset as u32, value)?;
                }
            }
            plan.batch(batch)?;
        }
        Some(())
    }
    fn mix_master(
        &self,
        plan: &mut PreparedEffectRackRebuild,
        common: InsertControlContext<'_>,
        bypass: bool,
    ) -> Option<()> {
        let i = plan.next.midi.master;
        let p = i.parameters;
        let mix = if bypass || i.kind == 0 {
            [0x7fffff, 0]
        } else {
            EffectMix::compile(
                EffectKind::new(i.kind)?,
                p[0],
                MixContext {
                    byte1: p[1],
                    byte5: p[5],
                    byte6: p[6],
                },
            )?
            .host_words()
        };
        let mut batch = EffectParameterBatch::from_lfo(None);
        for (offset, value) in mix.into_iter().enumerate() {
            batch.push_direct(u32::from(common.midi.master_origin) + offset as u32, value)?;
        }
        plan.batch(batch)
    }
    fn routing_transition(
        &self,
        plan: &mut PreparedEffectRackRebuild,
        common: InsertControlContext<'_>,
        rack: EffectRackInitializationContext<'_>,
        reset: bool,
    ) -> Option<()> {
        if !reset {
            plan.wait(70)?;
        }
        for part in 0..4 {
            let pair = core::array::from_fn(|role| {
                let i = plan.next.midi.inserts[part * 2 + role];
                EffectRoutingInstance {
                    kind: i.buffer.kind,
                    origin: i.buffer.origin,
                    parameters: i.buffer.parameters,
                }
            });
            if !reset {
                plan.batch(self.pair.prepare(&pair, 1)?)?;
            }
            let first = plan.next.midi.inserts[part * 2];
            let selector = 60 + first.slot * 3 + if reset { 0 } else { 2 };
            let word = if reset {
                self.prefixes[part]
            } else {
                self.tails[part]
            };
            let mut batch = EffectParameterBatch::from_lfo(None);
            batch.push_command(
                plan.next.prefix_origins[part * 2],
                0x02000000 | u32::from(selector),
            )?;
            plan.push(EffectRackRebuildStep::Effect(InsertControlStep {
                batch,
                program_writes: [Some(EffectStagedProgramWrite { selector, word }), None],
                body_program: None,
            }))?;
            if !reset {
                plan.wait(1)?;
                plan.batch(self.pair.prepare(&pair, 0)?)?;
                plan.wait(1)?;
            }
        }
        if reset {
            plan.wait(20)?;
        } else {
            plan.batch(self.master_transition(
                plan.next.midi.master.kind,
                common.midi.master_origin,
                true,
            )?)?;
        }
        let selector = if reset { 84 } else { 86 };
        let word = if reset {
            self.prefixes[4]
        } else {
            self.tails[4]
        };
        let mut batch = EffectParameterBatch::from_lfo(None);
        batch.push_command(rack.master_prefix, 0x02000000 | u32::from(selector))?;
        plan.push(EffectRackRebuildStep::Effect(InsertControlStep {
            batch,
            program_writes: [Some(EffectStagedProgramWrite { selector, word }), None],
            body_program: None,
        }))?;
        if !reset {
            plan.wait(1)?;
            plan.batch(self.master_transition(
                plan.next.midi.master.kind,
                common.midi.master_origin,
                false,
            )?)?;
            plan.wait(1)?;
        }
        Some(())
    }
    fn master_transition(
        &self,
        kind: u8,
        origin: u16,
        muted: bool,
    ) -> Option<EffectParameterBatch> {
        let offsets = self.master_transition_offsets.get(usize::from(kind))?;
        let values = if muted {
            [0, 0x7fffff]
        } else {
            [0x7f150f, 0xeaf0]
        };
        let mut batch = EffectParameterBatch::from_lfo(None);
        for (offset, value) in [offsets[1], offsets[0]].into_iter().zip(values) {
            batch.push_direct(u32::from(origin) + u32::from(offset), value)?;
        }
        Some(batch)
    }
    fn temporary_routes(
        &self,
        plan: &mut PreparedEffectRackRebuild,
        common: InsertControlContext<'_>,
    ) -> Option<()> {
        let routing = &self.rack.control.common.flanger.routing;
        let mut batch = EffectParameterBatch::from_lfo(None);
        for slot in 0..9 {
            let (kind, instance, profile) = if slot == 8 {
                let i = plan.next.midi.master;
                let k = common.program.master_effect().kind()?.raw();
                (
                    k,
                    EffectRoutingInstance {
                        kind: i.kind,
                        origin: common.midi.master_origin,
                        parameters: i.parameters,
                    },
                    *routing.master.get(usize::from(k))?,
                )
            } else {
                if slot % 2 == 1
                    && self.rack.occupies_pair
                        [usize::from(plan.next.midi.inserts[slot - 1].buffer.kind)]
                {
                    continue;
                }
                let i = plan.next.midi.inserts[slot];
                let k = common
                    .program
                    .timbre(slot / 2)?
                    .effect(slot % 2)?
                    .kind()?
                    .raw();
                (
                    k,
                    EffectRoutingInstance {
                        kind: i.buffer.kind,
                        origin: i.buffer.origin,
                        parameters: i.buffer.parameters,
                    },
                    *routing.insert.get(usize::from(k))?,
                )
            };
            let target = u32::from(instance.origin) + u32::from(profile.offset);
            if kind != 0 && target != UNASSIGNED_TARGET {
                for (index, value) in self.temporary_indices[slot].into_iter().zip([
                    target,
                    routing.gain(&instance, profile)?,
                    0x7a9765,
                    0x5689a,
                ]) {
                    batch.push_direct(u32::from(index), value)?;
                }
            }
        }
        plan.batch(batch)?;
        plan.wait(20)?;
        let mut batch = EffectParameterBatch::from_lfo(None);
        for indices in self.temporary_indices {
            for (index, value) in indices
                .into_iter()
                .zip([UNASSIGNED_TARGET, 0, 0x7a9765, 0x5689a])
            {
                batch.push_direct(u32::from(index), value)?;
            }
        }
        plan.batch(batch)
    }
}
