//! Original selected-group Detune/Spread and count/enable retirement boundaries.
use radias_synth_application::{polyphony::PolyphonicRenderer, voice_groups::edit_selected_groups};
use radias_synth_domain::{
    note_groups::NoteGroups,
    voice_allocation::{AllocationOwner, VoiceAllocator, VoiceClaim, VoiceOrder},
    voice_group::{GroupOffsets, VoiceGroupProgram, VoiceGroupSlots},
};
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("voice-group-live-edits.bin"))?;
    if raw.len() != 16384 * 1180 {
        return Err("Incomplete original live group boundary".into());
    }
    let tables =
        firmware::voice_group_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let mut errors = 0;
    let mut actors = 0;
    for (case, row) in raw.chunks_exact(1180).enumerate() {
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
        let mut slots = VoiceGroupSlots {
            indices: core::array::from_fn(|i| w(10 + 8 * i) as u8),
            ..Default::default()
        };
        let initial_slots = slots;
        let mut banks = core::array::from_fn(|i| w(11 + 8 * i) as u8);
        let mut offsets = core::array::from_fn(|i| GroupOffsets {
            tuning_q16: w(12 + 8 * i) as i32,
            pan: w(13 + 8 * i) as i8,
        });
        let mut seed = w(5) as u16;
        let program = VoiceGroupProgram {
            raw: w(1) as u8,
            detune: w(3) as u8,
            spread: w(4) as u8,
        };
        let selected = edit_selected_groups(
            w(0),
            program,
            w(2) as u8,
            &groups,
            &claims,
            &mut slots,
            &mut banks,
            &mut offsets,
            &tables,
            &mut seed,
        )
        .ok_or("Invalid native selected group")?;
        let mut pool = PolyphonicRenderer::default();
        pool.configure_voice_groups(tables);
        pool.initialize_voice_group_slots(initial_slots);
        pool.modulation_random = w(5) as u16;
        let pool_selected =
            pool.edit_selected_group_members(w(0), program, w(2) as u8, &groups, &claims);
        let pool_match = pool_selected == selected
            && pool.modulation_random == seed
            && (0..24).all(|i| {
                pool.voice_group_slots().indices[i] == slots.indices[i]
                    && (selected & (1 << i) == 0
                        || (pool.voice_group_bank(i) == banks[i]
                            && pool.voice_group_offsets(i) == offsets[i]))
            });
        let matches = seed as u32 == w(198)
            && (0..24).all(|i| {
                slots.indices[i] as u32 == w(199 + 4 * i)
                    && banks[i] as u32 == w(200 + 4 * i)
                    && offsets[i].tuning_q16 as u32 == w(201 + 4 * i)
                    && offsets[i].pan as u8 as u32 == w(202 + 4 * i)
            });
        if !matches || !pool_match {
            if errors < 3 {
                eprintln!(
                    "Original live group case{case} differs: seed{seed}/{} pool{pool_match}",
                    w(198)
                );
            }
            errors += 1;
        }
        actors += selected.count_ones();
    }
    let raw = fs::read(out.join("voice-group-edit-retirements.bin"))?;
    if raw.len() != 16384 * 968 {
        return Err("Incomplete original edit retirement boundary".into());
    }
    let mut retirement_errors = 0;
    let mut retired_actors = 0;
    for (case, row) in raw.chunks_exact(968).enumerate() {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let mut allocator = VoiceAllocator {
            order: VoiceOrder(core::array::from_fn(|i| w(98 + i) as u8)),
            ..Default::default()
        };
        for i in 0..24 {
            allocator.budget.costs[i] = w(2 + 4 * i) as u16;
            allocator.claims[i] = VoiceClaim {
                owner: AllocationOwner(w(3 + 4 * i) + 1),
                note_flags: w(4 + 4 * i) as u8,
                release_flags: w(5 + 4 * i) as u8,
            };
        }
        let initial = allocator.clone();
        let retired = allocator.retire_owner_for_program_edit(AllocationOwner(w(0) + 1));
        let mut pool = PolyphonicRenderer::default();
        pool.allocator = initial;
        let pool_retired = pool.retire_timbre_for_program_edit(w(0) as u8);
        let matches = pool_retired == retired
            && pool.allocator == allocator
            && (0..24).all(|i| {
                allocator.budget.costs[i] as u32 == w(122 + 4 * i)
                    && allocator.claims[i].owner.0 - 1 == w(123 + 4 * i)
                    && allocator.claims[i].note_flags as u32 == w(124 + 4 * i)
                    && allocator.claims[i].release_flags as u32 == w(125 + 4 * i)
                    && allocator.order.0[i] as u32 == w(218 + i)
            });
        if !matches {
            if retirement_errors < 3 {
                eprintln!("Original group edit retirement{case}/poly{} differs", w(1));
            }
            retirement_errors += 1;
        }
        retired_actors += retired.count_ones();
    }
    let scene = "live-voice-groups-edit-reference";
    let observed: Vec<serde_json::Value> =
        fs::read_to_string(out.join(format!("{scene}-group-live-boundaries.jsonl")))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
    if observed.len() != 12 {
        return Err("Four full original Detune/Spread callbacks required".into());
    }
    for triple in observed.chunks_exact(3) {
        let (input, before, after) = (&triple[0], &triple[1], &triple[2]);
        if input["kind"] != "input"
            || before["kind"] != "before"
            || after["kind"] != "after"
            || input["id"] != before["id"]
            || input["id"] != after["id"]
        {
            return Err("Original callback boundary ordering differs".into());
        }
        let n = |v: &serde_json::Value| v.as_u64().ok_or("Original scene field absent");
        let a = |i: usize, j: usize| n(&before["actors"][i][j]).unwrap() as u32;
        let groups = NoteGroups {
            counter: 0,
            ages: core::array::from_fn(|i| a(i, 0) as u16),
            tags: core::array::from_fn(|i| a(i, 1) as u8),
        };
        let claims = core::array::from_fn(|i| VoiceClaim {
            note_flags: a(i, 2) as u8,
            release_flags: a(i, 3) as u8,
            ..Default::default()
        });
        let mut slots = VoiceGroupSlots {
            indices: core::array::from_fn(|i| a(i, 4) as u8),
            ..Default::default()
        };
        let mut banks = core::array::from_fn(|i| a(i, 5) as u8);
        let mut offsets = core::array::from_fn(|i| GroupOffsets {
            tuning_q16: a(i, 6) as i32,
            pan: a(i, 7) as i8,
        });
        let mut seed = n(&before["seed"])? as u16;
        edit_selected_groups(
            n(&input["mask"])? as u32,
            VoiceGroupProgram {
                raw: n(&input["raw"])? as u8,
                detune: n(&input["detune"])? as u8,
                spread: n(&input["spread"])? as u8,
            },
            n(&input["primary"])? as u8,
            &groups,
            &claims,
            &mut slots,
            &mut banks,
            &mut offsets,
            &tables,
            &mut seed,
        )
        .ok_or("Original live group is invalid")?;
        if seed as u64 != n(&after["seed"])?
            || (0..24).any(|i| {
                slots.indices[i] as u64 != n(&after["actors"][i][4]).unwrap()
                    || banks[i] as u64 != n(&after["actors"][i][5]).unwrap()
                    || offsets[i].tuning_q16 as u32 as u64 != n(&after["actors"][i][6]).unwrap()
                    || offsets[i].pan as u8 as u64 != n(&after["actors"][i][7]).unwrap()
            })
        {
            return Err(
                format!("Native full original live callback{} differs", input["id"]).into(),
            );
        }
    }
    let state_rows: Vec<serde_json::Value> =
        fs::read_to_string(out.join(format!("{scene}-allocation-states.jsonl")))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
    if state_rows.len() != 16 {
        return Err("Original count/disable/enable scene is incomplete".into());
    }
    let handler_rows: Vec<serde_json::Value> =
        fs::read_to_string(out.join(format!("{scene}-group-handler-events.jsonl")))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
    let owner = handler_rows
        .iter()
        .find(|r| r["pc"].as_u64() == Some(0x0c009180))
        .and_then(|r| r["timbre"].as_u64())
        .ok_or("Original retirement handler missing")? as u32;
    for (before, after, old_raw, new_raw) in
        [(10, 11, 131, 130), (12, 13, 130, 2), (14, 15, 2, 130)]
    {
        let a = &state_rows[before];
        let e = &state_rows[after];
        let v =
            |r: &serde_json::Value, i: usize, j: usize| r["slots"][i][j].as_u64().unwrap() as u32;
        let map_owner = |raw| AllocationOwner(if raw == owner { 1 } else { raw });
        let mut pool = PolyphonicRenderer::default();
        pool.edit_voice_group(
            0,
            VoiceGroupProgram {
                raw: old_raw,
                detune: 64,
                spread: 127,
            },
        );
        pool.allocator.order = VoiceOrder(core::array::from_fn(|i| {
            a["order"][i].as_u64().unwrap() as u8
        }));
        for i in 0..24 {
            pool.allocator.claims[i] = VoiceClaim {
                owner: map_owner(v(a, i, 2)),
                note_flags: v(a, i, 0) as u8,
                release_flags: v(a, i, 1) as u8,
            };
            pool.allocator.budget.costs[i] = v(a, i, 3) as u16;
        }
        pool.edit_voice_group(
            0,
            VoiceGroupProgram {
                raw: new_raw,
                detune: 64,
                spread: 127,
            },
        );
        if (0..24).any(|i| {
            pool.allocator.claims[i]
                != VoiceClaim {
                    owner: map_owner(v(e, i, 2)),
                    note_flags: v(e, i, 0) as u8,
                    release_flags: v(e, i, 1) as u8,
                }
                || pool.allocator.budget.costs[i] as u32 != v(e, i, 3)
                || pool.allocator.order.0[i] as u64 != e["order"][i].as_u64().unwrap()
        }) {
            return Err(
                format!("Native full original raw{old_raw}->{new_raw} edit differs").into(),
            );
        }
    }
    let report = serde_json::json!({"passed":errors==0&&retirement_errors==0,"original_selected_group_cases":16384,
        "native_selected_actors":actors,"different_selected_group_cases":errors,"production_selected_group_pool_cases":16384,
        "original_retirement_cases":16384,"native_retired_actors":retired_actors,"different_retirement_cases":retirement_errors,
        "production_retirement_pool_cases":16384,"original_Detune_Spread_dispatcher_executed":true,
        "full_original_live_Detune_Spread_callbacks":4,"full_original_live_callback_offsets_banks_indices_random_match":true,
        "full_original_live_count_disable_enable_transitions":3,"production_edit_voice_group_count_disable_enable_matches":true,
        "downstream_tuning_pan_DSP_HPI_and_sequence_callbacks_excluded_from_scalar_corpus":true,"full_live_handler_audio_qualified":false});
    fs::write(
        out.join("voice-group-live-edit-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 || retirement_errors != 0 {
        return Err("Native live Unison handlers differ".into());
    }
    Ok(())
}
