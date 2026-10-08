//! Original01EBE4 retunes surviving members after a physical actor is stolen.
use radias_synth_domain::{
    note_groups::NoteGroups,
    voice_allocation::{VOICE_COUNT, VoiceClaim},
    voice_group::{GroupOffsets, VoiceGroupProgram, VoiceGroupSlots, VoiceGroupTables},
};

/// Original01EE1C/01EA64 applies each Detune or Spread parameter callback to
/// the selected physical groups. Both offsets are recomputed on every call.
#[allow(clippy::too_many_arguments)]
pub fn edit_selected_groups(
    selected: u32,
    program: VoiceGroupProgram,
    primary: u8,
    groups: &NoteGroups,
    claims: &[VoiceClaim; VOICE_COUNT],
    slots: &mut VoiceGroupSlots,
    banks: &mut [u8; VOICE_COUNT],
    offsets: &mut [GroupOffsets; VOICE_COUNT],
    tables: &VoiceGroupTables,
    seed: &mut u16,
) -> Option<u32> {
    let requested_bank = if primary & 63 == 8 {
        1
    } else if program.raw & 128 != 0 {
        (program.raw & 15) + 1
    } else {
        0
    };
    let mut remaining = selected & 0x00ff_ffff;
    while remaining != 0 {
        let mask = groups.take_selected_group(&mut remaining, claims);
        let bank = mask.count_ones() as u8 - 1;
        if bank >= 8 {
            return None;
        }
        let mut ordinal = 0;
        for slot in 0..VOICE_COUNT {
            if mask & (1 << slot) == 0 {
                continue;
            }
            if bank != requested_bank {
                slots.indices[slot] = ordinal;
                ordinal += 1;
            }
            banks[slot] = bank;
            offsets[slot] = tables.offsets(program, bank, slots.indices[slot], seed)?;
        }
    }
    Some(selected & 0x00ff_ffff)
}

/// The caller supplies pre-note identity and the allocator's temporary flags.
/// No allocation, envelope restart or private-LFO restart occurs in this step.
#[allow(clippy::too_many_arguments)]
pub fn repair_retired_groups(
    retired: u32,
    program: VoiceGroupProgram,
    primary: u8,
    groups: &NoteGroups,
    claims: &[VoiceClaim; VOICE_COUNT],
    slots: &VoiceGroupSlots,
    banks: &mut [u8; VOICE_COUNT],
    offsets: &mut [GroupOffsets; VOICE_COUNT],
    tables: &VoiceGroupTables,
    seed: &mut u16,
) -> Option<u32> {
    let mut remaining = retired & 0x00ff_ffff;
    let mut repaired = 0;
    let requested_bank = if primary & 63 == 8 {
        1
    } else if program.raw & 128 != 0 {
        (program.raw & 15) + 1
    } else {
        0
    };
    while remaining != 0 {
        let survivors = groups.surviving_group(&mut remaining, claims);
        if survivors == 0 {
            continue;
        }
        let bank = (survivors.count_ones() as u8 - 1).min(requested_bank);
        for slot in 0..VOICE_COUNT {
            if survivors & (1 << slot) != 0 {
                banks[slot] = bank;
                offsets[slot] = tables.offsets(program, bank, slots.indices[slot], seed)?;
            }
        }
        repaired |= survivors;
    }
    Some(repaired)
}
