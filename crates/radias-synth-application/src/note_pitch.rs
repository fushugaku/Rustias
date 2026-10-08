//! Per-channel performance inputs and per-note pitch identity.
use radias_synth_domain::note_pitch::{
    BasePitch, InitializedNote, NotePitchTables, PitchProgram, ScaleContext,
};

/// Controller initialization is staged: tuning and note identity are published
/// independently before the ordinary per-note pitch composer is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotePitchController {
    pub program: PitchProgram,
    pub note: InitializedNote,
    pub assigned_note_q16: i32,
    pub tuning_q16: i32,
}
impl NotePitchController {
    pub fn new(program: PitchProgram) -> Self {
        Self {
            program,
            note: InitializedNote {
                wrapped: 0,
                clamped_q8: 0,
                scale_q16: 0,
            },
            assigned_note_q16: 0,
            tuning_q16: 0,
        }
    }
    pub fn initialize_note(
        &mut self,
        midi_note: u8,
        scale: ScaleContext,
        tables: &NotePitchTables,
        seed: &mut u16,
    ) -> bool {
        if let Some(note) = self.program.initialize(midi_note, scale, tables, seed) {
            self.note = note;
            true
        } else {
            false
        }
    }
    pub fn compile_tuning(
        &mut self,
        tables: &NotePitchTables,
        master_tune: i32,
        virtual_patch: i32,
        manual_offset: i16,
    ) {
        self.tuning_q16 =
            self.program
                .tuning_q16(tables, master_tune, virtual_patch, manual_offset);
    }
    pub fn assign_note(&mut self, portamento_q16: i32) {
        self.assigned_note_q16 = ((self.note.wrapped as i32) << 16).wrapping_add(portamento_q16);
    }
    pub fn base(&self, midi: MidiPitch, manual_offset: i16, drum: bool) -> i32 {
        BasePitch {
            assigned_note_q16: self.assigned_note_q16,
            scale_q16: self.note.scale_q16,
            bend_q16: self.program.bend_q16(midi.bend),
            tuning_q16: self.tuning_q16,
            manual_offset,
            drum_transpose: drum.then_some(self.program.transpose),
        }
        .q16()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MidiPitch {
    pub bend: i16,
    pub wheel: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceNotePitch {
    pub note: InitializedNote,
}
impl VoiceNotePitch {
    /// Non-portamento 014d48 assigns the wrapped note, not the clamped Q8
    /// controller destination. Portamento supplies its separate Q16 offset.
    pub fn base(
        self,
        program: PitchProgram,
        midi: MidiPitch,
        tables: &NotePitchTables,
        master_tune: i32,
    ) -> i32 {
        NotePitchController {
            program,
            note: self.note,
            assigned_note_q16: (self.note.wrapped as i32) << 16,
            tuning_q16: program.tuning_q16(tables, master_tune, 0, 0),
        }
        .base(midi, 0, false)
    }
}
