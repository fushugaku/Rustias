//! Unchanged SH3 physical age/tag assignment and oldest group note-off.
use radias_synth_domain::{
    note_groups::NoteGroups,
    voice_allocation::{AllocationOwner, VoiceClaim, VoiceOrder},
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let mut errors = [0; 3];
    for (group, name, width, count) in [
        (0, "assign", 99, 32768),
        (1, "select", 146, 32768),
        (2, "counter", 2, 65536),
    ] {
        let raw = fs::read(out.join(format!("note-groups-{name}.bin")))?;
        if raw.len() != width * 4 * count {
            return Err(format!("Incomplete original note-group {name}").into());
        }
        for (index, row) in raw.chunks_exact(width * 4).enumerate() {
            let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
            let mismatch = match group {
                0 => {
                    let mut groups = NoteGroups {
                        counter: w(0) as u16,
                        ages: core::array::from_fn(|i| w(3 + 2 * i) as u16),
                        tags: core::array::from_fn(|i| w(4 + 2 * i) as u8),
                    };
                    groups.assign(w(1), w(2));
                    (0..24).any(|i| {
                        groups.ages[i] as u32 != w(51 + 2 * i)
                            || groups.tags[i] as u32 != w(52 + 2 * i)
                    })
                }
                1 => {
                    let groups = NoteGroups {
                        counter: 0,
                        ages: core::array::from_fn(|i| w(28 + 4 * i) as u16),
                        tags: core::array::from_fn(|i| w(27 + 4 * i) as u8),
                    };
                    let order = VoiceOrder(core::array::from_fn(|i| w(1 + i) as u8));
                    let mut claims = core::array::from_fn::<_, 24, _>(|i| VoiceClaim {
                        owner: AllocationOwner(w(25 + 4 * i)),
                        note_flags: w(26 + 4 * i) as u8,
                        release_flags: 0,
                    });
                    let mask = groups.release_mask(&order, &claims, AllocationOwner(0), w(0));
                    for (slot, claim) in claims.iter_mut().enumerate() {
                        if mask & (1 << slot) != 0 {
                            claim.note_flags &= 127;
                        }
                    }
                    mask != w(121) || (0..24).any(|i| claims[i].note_flags as u32 != w(122 + i))
                }
                2 => {
                    let mut groups = NoteGroups {
                        counter: w(0) as u16,
                        ..Default::default()
                    };
                    groups.dispatch();
                    groups.counter as u32 != w(1)
                }
                _ => unreachable!(),
            };
            if mismatch {
                if errors[group] < 3 {
                    eprintln!("Note group {name} {index} differs");
                }
                errors[group] += 1;
            }
        }
    }
    let report = serde_json::json!({"passed":errors==[0;3],"errors":errors,"original_assignment_cases":32768,"original_oldest_release_cases":32768,
        "original_dispatch_counter_cases":65536,"physical_age_wrap_tag_owner_and_held_matching_preserved":true,
        "source_envelope_HPI_release_calls_excluded_from_selection_fixture":true,"complete_native_engine":false});
    fs::write(
        out.join("note-groups-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != [0; 3] {
        return Err("Original note group identity/release differs".into());
    }
    println!("131072 original age/tag/group release cases match");
    Ok(())
}
