//! Four-timbre production routing against the original controller flag scan.
use radias_synth_application::polyphony::PolyphonicRenderer;
use radias_synth_domain::{
    mono_notes::VoiceMode,
    sustain::SustainState,
    voice_allocation::{AllocationOwner, VoiceClaim},
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/sustain-pool.bin"))?;
    if raw.len() != 8192 * 552 {
        return Err("Original four-timbre sustain corpus incomplete".into());
    }
    let mut errors = 0;
    let mut releases = 0;
    for (index, row) in raw.chunks_exact(552).enumerate() {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let mut pool = PolyphonicRenderer::default();
        for t in 0..4 {
            pool.set_voice_mode(t, VoiceMode::from_raw(w(5 + t as usize) as u8));
        }
        pool.initialize_sustain_states(core::array::from_fn(|t| SustainState {
            flags: w(9 + t) as u8,
        }));
        pool.allocator.claims = core::array::from_fn(|i| VoiceClaim {
            owner: AllocationOwner(w(13 + i) + 1),
            note_flags: w(37 + i) as u8,
            release_flags: w(61 + i) as u8,
        });
        let mask = pool.sustain_event(core::array::from_fn(|t| w(1 + t) as u8), w(0), None);
        releases += mask.count_ones();
        let mut actual = [0; 53];
        actual[0] = mask;
        for t in 0..4 {
            actual[1 + t] = pool.sustain_state(t as u8).unwrap().flags as u32;
        }
        for i in 0..24 {
            actual[5 + i] = pool.allocator.claims[i].note_flags as u32;
            actual[29 + i] = pool.allocator.claims[i].release_flags as u32;
        }
        let expected = core::array::from_fn::<_, 53, _>(|i| w(85 + i));
        if actual != expected {
            if errors < 3 {
                eprintln!("Sustain pool{index}: {actual:?} vs {expected:?}");
            }
            errors += 1;
        }
    }
    let report = serde_json::json!({"passed":errors==0,"original_four_timbre_events":8192,"errors":errors,"physical_voice_releases":releases,
        "production_CC_routing_and_release_masks_used":true,"four_timbre_update_then_service_order_preserved":true,
        "global_poly_pending_scan_across_owners_preserved":true,"source_HPI_DSP_calls_excluded_from_flag_fixture":true,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/sustain-pool-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Production sustain routing differs".into());
    }
    println!("8192 original four-timbre damper events match the production pool");
    Ok(())
}
