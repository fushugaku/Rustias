//! Complete per-note random/pitch initialization, SYS01ef78 and its callees.
use crate::{
    actor_control_state::ActorControlState,
    actor_pitch_preparation::ActorPitchPorts,
    actor_virtual_patch::{ActorVirtualPatchError, ActorVirtualPatchPorts},
    amplifier_control::AmplifierTables,
    controller_secondary::FineTuneTable,
    lfo::LfoState,
    modulation::{ModulationTables, ModulationTargets},
    note_pitch::{NotePitchTables, PitchProgram, fold_note},
    portamento::{PortamentoProgram, PortamentoRates, PortamentoState},
    raw_note_scale::{RawNoteScaleContext, RawNoteScaleTables},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorPortamentoInitialization {
    pub time: u8,
    pub switch_required: bool,
    pub switch: bool,
    pub context_flags: u8,
    pub gate_flags: u8,
    pub context_pitch_q16: i32,
}
#[derive(Clone, Copy)]
pub struct ActorNoteInitializationPorts {
    pub master_tune: i32,
    pub pitch: ActorPitchPorts,
    pub scale: RawNoteScaleContext,
    pub portamento: ActorPortamentoInitialization,
    pub sources: ActorVirtualPatchPorts,
}
pub struct ActorNoteInitializationTables<'a> {
    pub pitch: &'a NotePitchTables,
    pub scale: &'a RawNoteScaleTables,
    pub portamento: &'a PortamentoRates,
    pub amplifier: &'a AmplifierTables,
    pub modulation: &'a ModulationTables,
}
pub struct InitializedActorNote {
    pub controller: ActorControlState,
    pub random_seed: u16,
    pub modulations: ModulationTargets,
    pub controller_clocks: u16,
}
fn fold_work(mut note: i32) -> u16 {
    let mut work = 11;
    loop {
        if note < 0 {
            note += 12;
            work += 7;
        } else if (note as i8) < 0 {
            note -= 12;
            work += 10;
        } else {
            return work;
        }
    }
}
impl ActorControlState {
    pub fn prepare_tuning(
        &mut self,
        body: &[u8; 104],
        tables: &NotePitchTables,
        master_tune: i32,
    ) -> u16 {
        let value = PitchProgram {
            fine_tune: body[20],
            ..Default::default()
        }
        .tuning_q16(tables, master_tune, self.long(0xa0), self.word(0xe4));
        self.set_long(0x1c, value);
        32
    }
    /// Three signed random offsets share intensity, but retain separate words.
    /// The source compiles tuning and pitch before transposing the new note.
    pub fn prepare_random_offsets(
        &mut self,
        body: &[u8; 104],
        master_tune: i32,
        ports: ActorPitchPorts,
        tables: &NotePitchTables,
    ) -> u16 {
        let depth = i32::from(body[18] & 127);
        for index in 0..3 {
            let value =
                ((i32::from(self.word(0x164 + 2 * index)) * depth) >> 7).clamp(-32767, 32767);
            self.set_word(0xe4 + 2 * index, value as i16);
        }
        88 + self.prepare_tuning(body, tables, master_tune)
            + self
                .prepare_primary_pitch(body, ports, &tables.vibrato)
                .controller_clocks
    }
    pub fn prepare_transposed_note(
        &mut self,
        body: &[u8; 104],
        context: RawNoteScaleContext,
        tables: &RawNoteScaleTables,
        seed: &mut u16,
    ) -> u16 {
        let note = i32::from(self.bytes[0x35]) + i32::from(body[19]) - 64;
        let wrapped = fold_note(note);
        self.bytes[0x36] = wrapped;
        let (offset, work) = tables.offset(wrapped, context, seed);
        self.set_long(0x18, offset);
        self.set_word(0xfa, (note.clamp(0, 127) << 8) as i16);
        42 + fold_work(note) + work
    }
    pub fn initialize_portamento(
        &mut self,
        ports: ActorPortamentoInitialization,
        rates: &PortamentoRates,
    ) -> u16 {
        let rate = PortamentoProgram {
            time: ports.time,
            switch_required: ports.switch_required,
            curve: 0,
        }
        .rate(
            rates,
            ports.switch,
            self.word(0x132),
            self.bytes[0x180] as i8,
        );
        // This entry calls SYS01fa46 directly, omitting the live 01fb02
        // three-clock trampoline.
        let rate_work = if !ports.switch_required {
            39
        } else if ports.switch {
            44
        } else {
            25
        };
        let inherited = ports.context_flags & 128 != 0 || ports.gate_flags & 1 != 0;
        let branch_work = if rate == 0 {
            13
        } else if ports.context_flags & 128 == 0 && ports.gate_flags & 1 != 0 {
            40
        } else {
            32
        };
        if rate != 0 && inherited {
            self.set_long(0x14, ports.context_pitch_q16);
        }
        let mut state = PortamentoState::default();
        state.begin(rate, self.bytes[0x36], self.long(0x14), None);
        self.set_long(0x84, state.rate as i32);
        self.set_long(0x80, state.phase as i32);
        self.set_long(0x88, state.start_q16);
        self.set_long(0x8c, state.current_q16);
        54 + rate_work + branch_work + self.prepare_note_identity()
    }
    pub fn prepare_secondary_pitch(&mut self, body: &[u8; 104], tables: &FineTuneTable) -> u16 {
        self.compile_live_secondary_pitch(body, tables);
        let fine = i32::from(body[29] & 127) * 256 + i32::from(self.word(0x188));
        let limit = if fine > 0x7f00 {
            7
        } else if fine < 0 {
            10
        } else {
            9
        };
        let interpolation = if fine.clamp(0, 0x7f00) & 255 == 0 {
            23
        } else {
            34
        };
        58 + limit + interpolation
    }
    /// Whole SYS01ef78: all three PRNG calls, random/tuning/old pitch,
    /// transposed note/scale, portamento, preparation VP, pitch, OSC2, OSC1.
    /// A rejected active route commits neither controller nor shared seed.
    pub fn initialize_actor_note(
        &self,
        body: &[u8; 104],
        ports: ActorNoteInitializationPorts,
        mut seed: u16,
        tables: &ActorNoteInitializationTables<'_>,
    ) -> Result<InitializedActorNote, ActorVirtualPatchError> {
        let mut controller = *self;
        let mut work = 50;
        for index in 0..3 {
            work += 103 - (seed & 0x8805).count_ones() as u16;
            controller.set_word(0x164 + 2 * index, LfoState::next_random(&mut seed));
        }
        work +=
            controller.prepare_random_offsets(body, ports.master_tune, ports.pitch, tables.pitch);
        work += controller.prepare_transposed_note(body, ports.scale, tables.scale, &mut seed);
        work += 3 + controller.initialize_portamento(ports.portamento, tables.portamento);
        let (modulations, patches) = controller.prepare_virtual_patches_with_work(
            body,
            ports.sources,
            tables.amplifier,
            tables.modulation,
        )?;
        work += patches.total();
        work += controller
            .prepare_primary_pitch(body, ports.pitch, &tables.pitch.vibrato)
            .controller_clocks;
        work += controller.prepare_secondary_pitch(body, &tables.pitch.vibrato);
        work += controller.primary_preparation_clocks();
        controller.refresh_primary();
        Ok(InitializedActorNote {
            controller,
            random_seed: seed,
            modulations,
            controller_clocks: work,
        })
    }
}
