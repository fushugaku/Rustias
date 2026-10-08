//! Original additional-cost/reclamation bookkeeping; host cleanup is separate.
use radias_synth_domain::voice_allocation::{
    AllocationOwner, VoiceAllocator, VoiceClaim, VoiceOrder,
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/mono-notes-budget.bin"))?;
    if raw.len() != 8192 * 784 {
        return Err("Original Mono budget corpus incomplete".into());
    }
    let mut errors = 0;
    let mut reclaims = 0;
    for (index, row) in raw.chunks_exact(784).enumerate() {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let mut allocator = VoiceAllocator {
            order: VoiceOrder(core::array::from_fn(|i| w(27 + i) as u8)),
            claims: core::array::from_fn(|i| VoiceClaim {
                owner: AllocationOwner(1),
                note_flags: w(51 + i) as u8,
                release_flags: w(75 + i) as u8,
            }),
            budget: radias_synth_domain::voice_allocation::ProcessorBudget {
                costs: core::array::from_fn(|i| w(3 + i) as u16),
                master_overhead: w(2) as u16,
            },
        };
        let mask = allocator.prepare_mono_budget(w(0) as usize, w(1) as u16);
        if mask != 0 {
            reclaims += 1;
        }
        let mut actual = [0; 97];
        actual[0] = mask;
        for i in 0..24 {
            actual[1 + i] = allocator.budget.costs[i] as u32;
            actual[25 + i] = allocator.claims[i].note_flags as u32;
            actual[49 + i] = allocator.claims[i].release_flags as u32;
            actual[73 + i] = allocator.order.0[i] as u32;
        }
        let expected = core::array::from_fn::<_, 97, _>(|i| w(99 + i));
        if actual != expected {
            if errors < 3 {
                eprintln!("Mono budget{index}: {actual:?} vs {expected:?}");
            }
            errors += 1;
        }
    }
    let report = serde_json::json!({"passed":errors==0,"original_budget_cases":8192,"cases_with_reclamation":reclaims,"errors":errors,
        "production_mono_additional_cost_and_reclaim_bookkeeping_used":true,
        "unsigned_selected_old_cost_and_signed_reclaimed_costs_preserved":true,
        "original_host_cleanup_calls_excluded_from_boundary_fixture":true,"controller_dsp_cleanup_and_pressure_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/mono-budget-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Original Mono budget bookkeeping differs".into());
    }
    println!("8192 original Mono additional-cost/reclamation cases match");
    Ok(())
}
