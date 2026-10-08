//! Original01EBE4 survivor-group bank/offset/random boundary.
use radias_synth_application::polyphony::PolyphonicRenderer;
use radias_synth_application::voice_groups::repair_retired_groups;
use radias_synth_domain::{
    note_groups::NoteGroups,
    voice_allocation::VoiceClaim,
    voice_group::{GroupOffsets, VoiceGroupProgram, VoiceGroupSlots},
};
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("voice-group-repairs.bin"))?;
    if raw.len() != 16384 * 1084 {
        return Err("Incomplete original survivor-group boundary".into());
    }
    let tables =
        firmware::voice_group_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let mut errors = 0;
    let mut repaired_actors = 0;
    let mut group_cases = 0;
    for (case, row) in raw.chunks_exact(1084).enumerate() {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let groups = NoteGroups {
            counter: 0,
            ages: core::array::from_fn(|i| w(6 + 8 * i) as u16),
            tags: core::array::from_fn(|i| w(7 + 8 * i) as u8),
        };
        let claims = core::array::from_fn(|i| VoiceClaim {
            note_flags: w(8 + 8 * i) as u8,
            release_flags: w(9 + 8 * i) as u8,
            ..Default::default()
        });
        let slots = VoiceGroupSlots {
            indices: core::array::from_fn(|i| w(10 + 8 * i) as u8),
            ..Default::default()
        };
        let mut banks = core::array::from_fn(|i| w(11 + 8 * i) as u8);
        let mut offsets = core::array::from_fn(|i| GroupOffsets {
            tuning_q16: w(12 + 8 * i) as i32,
            pan: w(13 + 8 * i) as i8,
        });
        let mut seed = w(5) as u16;
        let mask = repair_retired_groups(
            w(0),
            VoiceGroupProgram {
                raw: w(1) as u8,
                detune: w(3) as u8,
                spread: w(4) as u8,
            },
            w(2) as u8,
            &groups,
            &claims,
            &slots,
            &mut banks,
            &mut offsets,
            &tables,
            &mut seed,
        )
        .ok_or("Invalid native repair")?;
        repaired_actors += mask.count_ones();
        group_cases += u32::from(mask != 0);
        let mut pool = PolyphonicRenderer::default();
        pool.configure_voice_groups(tables);
        pool.initialize_voice_group_slots(slots);
        pool.modulation_random = w(5) as u16;
        let pool_mask = pool.repair_retired_group_members(
            w(0),
            VoiceGroupProgram {
                raw: w(1) as u8,
                detune: w(3) as u8,
                spread: w(4) as u8,
            },
            w(2) as u8,
            &groups,
            &claims,
        );
        let pool_matches = pool_mask == mask
            && pool.modulation_random == seed
            && (0..24).all(|i| {
                if mask & (1 << i) != 0 {
                    pool.voice_group_bank(i) == banks[i]
                        && pool.voice_group_offsets(i) == offsets[i]
                } else {
                    pool.voice_group_bank(i) == 0
                        && pool.voice_group_offsets(i) == GroupOffsets::default()
                }
            });
        let matches = seed as u32 == w(198)
            && (0..24).all(|i| {
                banks[i] as u32 == w(199 + 3 * i)
                    && offsets[i].tuning_q16 as u32 == w(200 + 3 * i)
                    && offsets[i].pan as u8 as u32 == w(201 + 3 * i)
            });
        if !matches || !pool_matches {
            if errors < 3 {
                eprintln!(
                    "Survivor case{case}: seed{seed} != {}, retired{:x}, repaired{mask:x}",
                    w(198),
                    w(0)
                );
                for i in 0..24 {
                    if banks[i] as u32 != w(199 + 3 * i)
                        || offsets[i].tuning_q16 as u32 != w(200 + 3 * i)
                        || offsets[i].pan as u8 as u32 != w(201 + 3 * i)
                    {
                        eprintln!(
                            " actor{i}: bank{} tuning{} pan{} vs {}/{}/{}",
                            banks[i],
                            offsets[i].tuning_q16,
                            offsets[i].pan,
                            w(199 + 3 * i),
                            w(200 + 3 * i) as i32,
                            w(201 + 3 * i) as u8 as i8
                        );
                    }
                }
            }
            errors += 1;
        }
    }
    let report = serde_json::json!({"passed":errors==0,"original_cases":16384,"cases_with_survivors":group_cases,"repaired_actors":repaired_actors,
        "different_cases":errors,"original_offset_and_random_kernels_executed":true,"native_application_use_case_used":true,
        "production_pool_cases":16384,"tuning_pan_publication_callbacks_excluded":true,"full_stealing_audio_qualified":false});
    fs::write(
        out.join("voice-group-repair-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Native survivor-group repair differs".into());
    }
    Ok(())
}
