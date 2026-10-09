//! Whole SYS01e838 construction: two pool passes and ordered native publishers.
use crate::{
    actor_control_state::ActorControlState,
    actor_copy::{ActorTemplateBinding, ParameterTemplateAddresses},
    actor_descriptors::DescriptorPlan,
    actor_lfo_initialization::ActorLfoState,
    actor_lifecycle::ActorLifecycle,
    actor_note_preparation::{
        ActorNotePreparationError, ActorNotePreparationPorts, ActorNotePreparationRequest,
        ActorNotePreparationState, ActorNotePreparationTables,
    },
    complete_actor_startup::{CompleteStartupError, CompleteStartupTables},
    construction_first_pass::ConstructionFirstPass,
    dsp_control::ParameterPacket,
    lfo::LfoState,
    modulation::ModulationTargets,
    motion_initialization::MotionControlState,
    parameter_template::TemplateCompilationError,
};

/// Raw signed ordinal lookup, including source ROM words outside musical groups.
pub struct ConstructionGroupTables {
    pub detune: [Option<[i16; 256]>; 9],
    pub pan: [Option<[i16; 256]>; 9],
    pub drum_output_routes: [u32; 8],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorConstructionState {
    pub controllers: [ActorControlState; 24],
    pub lfos: [[ActorLfoState; 2]; 24],
    pub motion: [MotionControlState; 24],
    pub allocation_flags: [u8; 24],
    pub context: [u8; 100],
    pub random_seed: u16,
    pub pending_retirement: u32,
    pub lifecycle: ActorLifecycle,
    pub modulations: ModulationTargets,
}
#[derive(Clone, Copy)]
pub struct ActorConstructionRequest<'a> {
    pub selected: u32,
    pub midi_word: u32,
    pub owner_identity: u32,
    pub body: &'a [u8; 104],
    pub ports: ActorNotePreparationPorts,
    pub shared_lfos: [ActorLfoState; 2],
    pub group_ordinals: [u8; 24],
    pub detune_amount: u8,
    pub spread_amount: u8,
    pub binding: ActorTemplateBinding,
}
pub struct ActorConstructionTables<'a> {
    pub note: &'a ActorNotePreparationTables<'a>,
    pub startup: &'a CompleteStartupTables<'a>,
    pub groups: &'a ConstructionGroupTables,
    pub addresses: &'a ParameterTemplateAddresses,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorConstructionError {
    InvalidGroupBank,
    Note(ActorNotePreparationError),
    Descriptors(TemplateCompilationError),
    Startup(CompleteStartupError),
}
#[derive(Clone, Copy, Debug)]
pub struct ConstructionStep {
    pub slot: usize,
    pub publication: DescriptorPlan,
}
pub struct ConstructedActors {
    pub state: ActorConstructionState,
    steps: [ConstructionStep; 224],
    count: usize,
}
impl ConstructedActors {
    pub fn steps(&self) -> &[ConstructionStep] {
        &self.steps[..self.count]
    }
    fn push(&mut self, slot: usize, publication: DescriptorPlan, before: u16) {
        self.steps[self.count] = ConstructionStep {
            slot,
            publication: publication.with_preparation_work(before),
        };
        self.count += 1;
    }
    fn work(&mut self, slot: usize, clocks: u16) {
        let mut plan = DescriptorPlan::default();
        plan.work(clocks);
        self.steps[self.count] = ConstructionStep {
            slot,
            publication: plan,
        };
        self.count += 1;
    }
    fn detach(&mut self, slot: usize, before: u16) {
        let mut plan = DescriptorPlan::default();
        if self.state.lifecycle.detach(slot) {
            let physical = 0x300c + 64 * (slot % 12) as u16;
            let parameter = 0x2000 + 160 * (slot % 12) as u16;
            plan.send(
                ParameterPacket::DETACH_ACTOR_SENDER,
                physical - parameter,
                u32::from(parameter),
                30,
                6,
            );
        } else {
            plan.work(20);
        }
        self.push(slot, plan, before);
    }
    fn retire_pending(&mut self, slot: usize, before: u16) {
        let mut remaining = self.state.pending_retirement & 0x00ff_ffff;
        self.state.pending_retirement = 0;
        if remaining == 0 {
            self.work(slot, before + 20);
            return;
        }
        let mut work = before + 14;
        let mut candidate = 0;
        loop {
            let selected = remaining & 1 != 0;
            remaining >>= 1;
            if selected {
                self.detach(candidate, work + 5);
                work = 0;
            } else {
                work += 4;
            }
            work += if remaining == 0 { 4 } else { 5 };
            if remaining == 0 {
                break;
            }
            candidate += 1;
        }
        self.work(slot, work + 6);
    }
}
impl ActorConstructionState {
    pub fn construct(
        &self,
        request: ActorConstructionRequest<'_>,
        tables: &ActorConstructionTables<'_>,
    ) -> Result<ConstructedActors, ActorConstructionError> {
        let bank = self.context[0x57] as usize;
        if bank >= 9 {
            return Err(ActorConstructionError::InvalidGroupBank);
        }
        let mut result = ConstructedActors {
            state: *self,
            steps: [ConstructionStep {
                slot: 0,
                publication: DescriptorPlan::default(),
            }; 224],
            count: 0,
        };
        let selected = request.selected & 0x00ff_ffff;
        let first_pass =
            ConstructionFirstPass::compile(&self.controllers, selected, request.owner_identity);
        for store in first_pass.stores() {
            store.apply(
                &mut result.state.controllers,
                &mut result.state.allocation_flags,
            );
        }
        result.work(0, first_pass.return_clock + 9);
        for slot in 0..24 {
            if selected & (1 << slot) == 0 {
                result.work(slot, if slot == 23 { 12 } else { 13 });
                continue;
            }
            result.state.context[0x48] = request.midi_word as u8;
            result.state.context[0x49] = (request.midi_word >> 8) as u8;
            let controller = &mut result.state.controllers[slot];
            controller.bytes[0x35] = 60;
            controller.bytes[0x37] = (request.midi_word >> 8) as u8;
            let ordinal = usize::from(request.group_ordinals[slot]);
            let detune = tables.groups.detune[bank]
                .as_ref()
                .map(|words| words[ordinal]);
            let mut work = 17;
            let tuning = if let Some(value) = detune {
                if value == 0 {
                    work += 17;
                    0
                } else {
                    work += 147 - (result.state.random_seed & 0x8805).count_ones() as u16;
                    let random = i32::from(LfoState::next_random(&mut result.state.random_seed));
                    let jitter = (random.wrapping_mul(65).wrapping_shl(1) >> 16) as i16;
                    (i32::from(value) + i32::from(jitter))
                        .wrapping_mul(i32::from(request.detune_amount as i8))
                }
            } else {
                work += 8;
                0
            };
            controller.set_long(0xa0, tuning);
            work += 3;
            let pan = if let Some(words) = tables.groups.pan[bank].as_ref() {
                let value = words[ordinal];
                if value == 0 {
                    work += 17;
                    0
                } else {
                    work += 31;
                    ((i32::from(value) * i32::from(request.spread_amount & 127)) >> 8) as i8
                }
            } else {
                work += 8;
                0
            };
            controller.bytes[0x1e8] = pan as u8;
            work += 3;
            controller.prepare_from_body(request.body, bank as u8);
            work += 45 + controller.primary_preparation_clocks() + 3;
            let note = ActorNotePreparationState {
                controller: *controller,
                lfos: result.state.lfos[slot],
                motion: result.state.motion[slot],
                random_seed: result.state.random_seed,
            }
            .prepare_note_controls(
                ActorNotePreparationRequest {
                    body: request.body,
                    ports: request.ports,
                    shared_lfos: request.shared_lfos,
                },
                tables.note,
            )
            .map_err(ActorConstructionError::Note)?;
            result.state.controllers[slot] = note.state.controller;
            result.state.lfos[slot] = note.state.lfos;
            result.state.motion[slot] = note.state.motion;
            result.state.random_seed = note.state.random_seed;
            result.state.modulations = note.modulations;
            result.push(slot, note.publication, work);
            result.retire_pending(slot, 4);
            result.detach(slot, 4);
            // SYS020266 publishes the selected drum output pair before the actor copy.
            let controller = &mut result.state.controllers[slot];
            let drum = request.binding.drum_instrument & 15;
            controller.bytes[0x1ed] = drum;
            let parameter = 0x2000 + 160 * (slot % 12) as u16;
            let source = tables.addresses.drums[usize::from(drum)];
            let output_routes =
                tables.groups.drum_output_routes[usize::from(request.binding.program_kind >> 5)];
            let mut plan = DescriptorPlan::default();
            plan.send(
                0,
                source.wrapping_add(0x82).wrapping_sub(parameter),
                output_routes >> 16,
                43,
                0,
            );
            plan.send(
                0,
                source.wrapping_add(0x83).wrapping_sub(parameter),
                output_routes & 65535,
                7,
                6,
            );
            result.push(slot, plan, 4);
            let mut copy = DescriptorPlan::default();
            copy.send(
                ParameterPacket::ACTOR_COPY_SENDER,
                0,
                u32::from(request.binding.source(tables.addresses)),
                request.binding.sender_gap(),
                ActorTemplateBinding::RETURN_GAP,
            );
            result.push(slot, copy, 4);
            let descriptor = DescriptorPlan::compile(
                12,
                request.body,
                result.state.controllers[slot].descriptor_cache(),
                tables.startup.descriptors,
            )
            .map_err(ActorConstructionError::Descriptors)?;
            result.push(slot, descriptor, 3);
            let startup = result.state.controllers[slot]
                .compile_complete_startup(
                    0,
                    slot,
                    request.body,
                    result.state.lifecycle,
                    tables.startup,
                )
                .map_err(ActorConstructionError::Startup)?;
            result.state.controllers[slot] = startup.controller;
            result.state.lifecycle = startup.lifecycle;
            result.push(slot, startup.publication, 3);
            result.state.allocation_flags[slot] = 0xc0;
            result.state.context[0x58] = slot as u8;
            result.work(slot, if slot == 23 { 11 } else { 12 });
        }
        result.work(0, 14);
        Ok(result)
    }
}
