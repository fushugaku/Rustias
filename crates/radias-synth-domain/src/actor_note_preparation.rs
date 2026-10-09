//! Complete SYS01ee38 controller preparation and its ordered DSP publications.
use crate::{
    actor_amplifier_preparation::{ActorAmplifierPorts, InvalidUnisonGainBank},
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorPlan,
    actor_lfo_initialization::{ActorLfoState, initialize_lfo_rates},
    actor_note_initialization::{ActorNoteInitializationPorts, ActorNoteInitializationTables},
    actor_virtual_patch::ActorVirtualPatchError,
    controller_mixer::{MixerLevel, MixerScales},
    controller_pan::PanControl,
    lfo_tempo::LfoTempoTables,
    manual_parameters::{ManualCompilerPorts, ManualCompilerTables},
    modulation::ModulationTargets,
    motion_initialization::{MotionControlState, MotionInitializationError, MotionNoteRequest},
    note_modulators::{NoteModulatorRequest, NoteModulatorTables, initialize_note_modulators},
    note_refresh::{NoteRefreshError, NoteRefreshPorts, NoteRefreshRequest, NoteRefreshTables},
    virtual_patch_live::LiveCompilerPorts,
};

#[derive(Clone, Copy)]
pub struct ActorNotePreparationPorts {
    pub motion_assignments: [u8; 3],
    pub motion_program_flags: u8,
    pub motion_global_flags: u8,
    pub lfo_clock_rate: u32,
    pub midi_pan: u8,
    pub note: ActorNoteInitializationPorts,
    pub amplifier: ActorAmplifierPorts,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorNotePreparationState {
    pub controller: ActorControlState,
    pub motion: MotionControlState,
    pub lfos: [ActorLfoState; 2],
    pub random_seed: u16,
}
#[derive(Clone, Copy)]
pub struct ActorNotePreparationRequest<'a> {
    pub body: &'a [u8; 104],
    pub ports: ActorNotePreparationPorts,
    pub shared_lfos: [ActorLfoState; 2],
}
pub struct ActorNotePreparationTables<'a> {
    pub manual: &'a ManualCompilerTables<'a>,
    pub tempo: &'a LfoTempoTables,
    pub modulators: &'a NoteModulatorTables<'a>,
    pub refresh: &'a NoteRefreshTables<'a>,
    pub note: &'a ActorNoteInitializationTables<'a>,
    pub mixer: &'a MixerScales,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorNotePreparationError {
    Motion(MotionInitializationError),
    VirtualPatch(ActorVirtualPatchError),
    Refresh(NoteRefreshError),
    GainBank(InvalidUnisonGainBank),
}
pub struct PreparedActorNoteControls {
    pub state: ActorNotePreparationState,
    pub modulations: ModulationTargets,
    pub publication: DescriptorPlan,
}

impl ActorControlState {
    /// SYS01ee5c..01ee82, including the complete SYS020154 selector.
    pub fn prepare_initial_mixer_scales(&mut self, tables: &MixerScales) -> u16 {
        self.set_word(0xfe, tables.primary(self.bytes[0x1e0]) as i16);
        self.set_word(0x100, tables.secondary(self.bytes[0x1e1]) as i16);
        if self.bytes[0x1e1] & 16 == 0 { 35 } else { 34 }
    }
    /// Whole SYS00279e/002882. Squared DSP gain is a later publisher.
    pub fn prepare_initial_mixer_level(&mut self, body: &[u8; 104], noise: bool) -> u16 {
        let index = if noise { 2 } else { 1 };
        let level = MixerLevel {
            level: body[30 + index],
            manual_offset: self.word(0x18a + 2 * index),
            modulation: self.word(0x11a + 2 * index),
            scale: 0,
        };
        self.set_word(0x102 + 2 * index, level.composed() as i16);
        let raw = 2
            * ((i32::from(level.level as i8) << 8)
                + i32::from(level.manual_offset)
                + 2 * i32::from(level.modulation));
        if raw < 0 {
            25
        } else if raw > 65535 {
            28
        } else {
            27
        }
    }
    /// Whole SYS00250e; the DSP pan lookup is deliberately outside this entry.
    pub fn prepare_initial_pan(&mut self, body: &[u8; 104], midi_pan: u8) -> u16 {
        let control = PanControl {
            position: body[49],
            manual_offset: self.word(0x1a4),
            modulation: self.word(0x12c),
            timbre_offset: self.bytes[0x1e8] as i8,
            midi_pan: (self.bytes[0x1e7] != 0).then_some(midi_pan),
        };
        self.set_word(0xfc, control.target() as i16);
        let raw = ((i32::from(control.position as i8) << 8)
            + i32::from(control.manual_offset)
            + 2 * i32::from(control.modulation))
        .clamp(0, 32767);
        let own = if raw == 16384 {
            47
        } else if raw < 16384 {
            64
        } else {
            70
        };
        own + control.midi_pan.map_or(0, |pan| match pan & 127 {
            64 => 8,
            0..=63 => 25,
            _ => 31,
        })
    }
    pub fn prepare_initial_amplifier(
        &mut self,
        body: &[u8; 104],
        ports: ActorAmplifierPorts,
        tables: &crate::amplifier_control::AmplifierTables,
    ) -> Result<u16, InvalidUnisonGainBank> {
        Ok(11
            + self.prepare_amplifier_key(body, tables)
            + self.prepare_amplifier_target(body, ports, tables)?)
    }
}

impl ActorNotePreparationState {
    pub fn prepare_note_controls(
        &self,
        request: ActorNotePreparationRequest<'_>,
        tables: &ActorNotePreparationTables<'_>,
    ) -> Result<PreparedActorNoteControls, ActorNotePreparationError> {
        let mut state = *self;
        let ports = request.ports;
        let live = LiveCompilerPorts {
            portamento_time: ports.note.portamento.time,
            portamento_switch_required: ports.note.portamento.switch_required,
            portamento_switch: ports.note.portamento.switch,
            midi_pan: (state.controller.bytes[0x1e7] != 0).then_some(ports.midi_pan),
        };
        let motion = state
            .controller
            .initialize_motion_note(
                MotionNoteRequest {
                    assignments: ports.motion_assignments,
                    program_flags: ports.motion_program_flags,
                    global_flags: ports.motion_global_flags,
                    body: request.body,
                    ports: ManualCompilerPorts {
                        live,
                        pitch: ports.note.pitch,
                        amplifier: ports.amplifier,
                    },
                },
                state.motion,
                tables.manual,
            )
            .map_err(ActorNotePreparationError::Motion)?;
        state.controller = motion.controller;
        state.motion = motion.motion;
        let mut publication = DescriptorPlan::default();
        publication.work(5);
        publication.append_compacted(&motion.publication);
        let mut before_refresh = 4 + initialize_lfo_rates(
            &mut state.lfos,
            request.body,
            ports.lfo_clock_rate,
            tables.tempo,
        );
        let modulators = initialize_note_modulators(
            NoteModulatorRequest {
                controller: state.controller,
                lfos: state.lfos,
                shared_lfos: request.shared_lfos,
                random_seed: state.random_seed,
                body: request.body,
                ports: ports.note.sources,
            },
            tables.modulators,
        )
        .map_err(ActorNotePreparationError::VirtualPatch)?;
        state.controller = modulators.controller;
        state.lfos = modulators.lfos;
        state.random_seed = modulators.random_seed;
        before_refresh += 4 + modulators.controller_clocks + 4;
        let mut work = DescriptorPlan::default();
        work.work(before_refresh);
        publication.append_compacted(&work);
        let refresh = state
            .controller
            .compile_note_refresh(
                NoteRefreshRequest {
                    body: request.body,
                    lfos: state.lfos,
                    ports: NoteRefreshPorts {
                        virtual_patch: ports.note.sources,
                        live_compilers: live,
                        pitch: ports.note.pitch,
                        amplifier: ports.amplifier,
                    },
                },
                tables.refresh,
            )
            .map_err(ActorNotePreparationError::Refresh)?;
        state.controller = refresh.controller;
        publication.append_compacted(&refresh.publication);
        let note = state
            .controller
            .initialize_actor_note(request.body, ports.note, state.random_seed, tables.note)
            .map_err(ActorNotePreparationError::VirtualPatch)?;
        state.controller = note.controller;
        state.random_seed = note.random_seed;
        let mut after_refresh = 3 + note.controller_clocks + 4;
        after_refresh += state
            .controller
            .initialize_note_filters(request.body, tables.refresh.compilers);
        after_refresh += state.controller.prepare_initial_mixer_scales(tables.mixer);
        after_refresh += 4 + state
            .controller
            .prepare_initial_mixer_level(request.body, false);
        after_refresh += 4 + state
            .controller
            .prepare_initial_mixer_level(request.body, true);
        after_refresh += 4 + state
            .controller
            .prepare_initial_pan(request.body, ports.midi_pan);
        after_refresh += 4 + state
            .controller
            .prepare_initial_amplifier(request.body, ports.amplifier, tables.modulators.amplifier)
            .map_err(ActorNotePreparationError::GainBank)?;
        after_refresh += 4;
        let mut work = DescriptorPlan::default();
        work.work(after_refresh);
        publication.append_compacted(&work);
        Ok(PreparedActorNoteControls {
            state,
            modulations: note.modulations,
            publication,
        })
    }
}
