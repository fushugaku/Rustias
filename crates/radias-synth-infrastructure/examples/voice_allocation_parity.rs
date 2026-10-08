use radias_synth_domain::voice_allocation::VoiceAllocator;
use radias_synth_domain::voice_allocation::{
    AllocationOwner, AllocationPass, VoiceClaim, VoiceOrder,
};
use radias_synth_domain::voice_allocation::{ProcessorBudget, VoiceCostParameters};
use radias_synth_infrastructure::firmware::voice_cost_tables;
use std::{fs, path::PathBuf};
fn word(raw: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(raw[4 * i..4 * i + 4].try_into().unwrap())
}
fn pass(value: u32) -> AllocationPass {
    if value == 0 {
        AllocationPass::Reuse
    } else if value == 1 {
        AllocationPass::Replace
    } else {
        AllocationPass::Fresh
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository path required")?);
    let raw = fs::read(root.join("runs/native-clone/allocation-scores.bin"))?;
    if raw.len() != 98304 * 32 {
        return Err("Incomplete allocation priority corpus".into());
    }
    let mut errors = 0;
    for (i, r) in raw.chunks_exact(32).enumerate() {
        let claim = VoiceClaim {
            owner: AllocationOwner(word(r, 2)),
            note_flags: word(r, 3) as u8,
            release_flags: word(r, 4) as u8,
        };
        let mut a = claim.priority(AllocationOwner(1), word(r, 1) as u8, pass(word(r, 0)));
        if word(r, 0) == 2 && word(r, 6) & (1 << (word(r, 5) / 12)) == 0 {
            a = 0;
        }
        if a != word(r, 7) {
            if errors < 3 {
                eprintln!("Priority {i}: {a:08x} != {:08x}", word(r, 7));
            }
            errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/allocation-selections.bin"))?;
    const SIZE: usize = 127 * 4;
    if raw.len() != 12288 * SIZE {
        return Err("Incomplete allocation selection corpus".into());
    }
    for (i, r) in raw.chunks_exact(SIZE).enumerate() {
        let mut order = VoiceOrder(core::array::from_fn(|n| word(r, 2 + n) as u8));
        let claims = core::array::from_fn(|n| VoiceClaim {
            owner: AllocationOwner(word(r, 26 + 3 * n)),
            note_flags: word(r, 27 + 3 * n) as u8,
            release_flags: word(r, 28 + 3 * n) as u8,
        });
        let selected = order.select_with_processors(
            &claims,
            AllocationOwner(1),
            word(r, 1),
            pass(word(r, 0)),
            word(r, 126) as u8,
        );
        let actual = selected.map_or([u32::MAX, u32::MAX, 0], |s| {
            [s.slot as u32, s.position as u32, s.priority]
        });
        let expected = [word(r, 98), word(r, 99), word(r, 100)];
        if actual != expected {
            if errors < 3 {
                eprintln!("Selection {i}: {actual:?} != {expected:?}");
            }
            errors += 1;
        }
        order.move_to_back(word(r, 101) as u8);
        let expected = core::array::from_fn::<_, 24, _>(|n| word(r, 102 + n) as u8);
        if order.0 != expected {
            if errors < 3 {
                eprintln!("Voice order {i} differs");
            }
            errors += 1;
        }
    }
    let tables = voice_cost_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let raw = fs::read(root.join("runs/native-clone/allocation-costs.bin"))?;
    if raw.len() != 32768 * 24 {
        return Err("Incomplete voice cost corpus".into());
    }
    for (i, r) in raw.chunks_exact(24).enumerate() {
        let actual = tables
            .cost(VoiceCostParameters {
                primary: word(r, 0) as u8,
                secondary: word(r, 1) as u8,
                filter_route: word(r, 2) as u8,
                drive_mode: word(r, 3) as u8,
                shaper_type: word(r, 4) as u8,
            })
            .ok_or("Invalid original cost selector")?;
        if actual != word(r, 5) {
            if errors < 3 {
                eprintln!("Voice cost {i}: {actual} != {}", word(r, 5));
            }
            errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/allocation-budgets.bin"))?;
    if raw.len() != 8192 * 108 {
        return Err("Incomplete processor budget corpus".into());
    }
    for (i, r) in raw.chunks_exact(108).enumerate() {
        let actual = ProcessorBudget {
            master_overhead: word(r, 0) as u16,
            costs: core::array::from_fn(|n| word(r, 1 + n) as u16),
        }
        .remaining();
        let expected = [word(r, 25) as i32, word(r, 26) as i32];
        if actual != expected {
            if errors < 3 {
                eprintln!("Budget {i}: {actual:?} != {expected:?}");
            }
            errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/allocation-reclaims.bin"))?;
    if raw.len() != 4096 * 175 * 4 {
        return Err("Incomplete reclamation corpus".into());
    }
    for (i, r) in raw.chunks_exact(175 * 4).enumerate() {
        let mut allocator = VoiceAllocator {
            order: VoiceOrder(core::array::from_fn(|n| word(r, 27 + n) as u8)),
            claims: core::array::from_fn(|n| VoiceClaim {
                owner: AllocationOwner(1),
                note_flags: word(r, 51 + n) as u8,
                release_flags: word(r, 75 + n) as u8,
            }),
            budget: ProcessorBudget {
                master_overhead: word(r, 2) as u16,
                costs: core::array::from_fn(|n| word(r, 3 + n) as u16),
            },
        };
        let (processors, displaced) = allocator.reclaim(word(r, 0) as i32, word(r, 1) as u8);
        let expected_costs = core::array::from_fn::<_, 24, _>(|n| word(r, 101 + n) as u16);
        let expected_notes = core::array::from_fn::<_, 24, _>(|n| word(r, 125 + n) as u8);
        let expected_flags = core::array::from_fn::<_, 24, _>(|n| word(r, 149 + n) as u8);
        let notes = allocator.claims.map(|c| c.note_flags);
        let flags = allocator.claims.map(|c| c.release_flags);
        if processors != word(r, 99) as u8
            || displaced != word(r, 100)
            || allocator.budget.costs != expected_costs
            || notes != expected_notes
            || flags != expected_flags
        {
            if errors < 3 {
                eprintln!("Reclaim state {i} differs");
            }
            errors += 1;
        }
    }
    let mut instrument = VoiceAllocator::default();
    let mut pressure_steps = 0;
    for line in
        fs::read_to_string(root.join("runs/native-clone/poly-pressure25-allocation-states.jsonl"))?
            .lines()
    {
        let state: serde_json::Value = serde_json::from_str(line)?;
        if state["on"].as_bool() != Some(true) {
            return Err("Pressure gate requires held notes".into());
        }
        let note = state["note"].as_u64().ok_or("Missing original note")? as u8;
        instrument
            .allocate_poly(AllocationOwner(1), note, 4283)
            .ok_or("Native allocation failed")?;
        for slot in 0..24 {
            let expected = &state["slots"][slot];
            let claim = instrument.claims[slot];
            if claim.note_flags != expected[0].as_u64().ok_or("Missing note flags")? as u8
                || claim.release_flags != expected[1].as_u64().ok_or("Missing release flags")? as u8
                || instrument.budget.costs[slot]
                    != expected[3].as_u64().ok_or("Missing cost")? as u16
            {
                if errors < 3 {
                    eprintln!("Held-note pressure {note}, slot {slot} differs");
                }
                errors += 1;
            }
        }
        let expected =
            core::array::from_fn::<_, 24, _>(|n| state["order"][n].as_u64().unwrap() as u8);
        if expected != instrument.order.0 {
            errors += 1;
        }
        pressure_steps += 1;
    }
    if pressure_steps != 25 {
        return Err("Incomplete original pressure sequence".into());
    }
    let report = serde_json::json!({"priority_cases":98304,"selection_order_cases":12288,"voice_cost_cases":32768,"budget_cases":8192,
        "reclaim_state_cases":4096,"reclaim_host_cleanup_qualified":false,"original_live_held_note_transitions":pressure_steps,
        "errors":errors,"original_sh3_bytes_executed":true,"complete_polyphony":false});
    fs::write(
        root.join("runs/native-clone/voice-allocation-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Native voice allocation priority differs".into());
    }
    Ok(())
}
