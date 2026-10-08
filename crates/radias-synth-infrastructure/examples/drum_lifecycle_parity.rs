use radias_synth_application::{
    VoiceRenderer,
    polyphony::{ActiveVoice, PolyphonicRenderer},
};
use radias_synth_domain::pan::VoiceBus;
use radias_synth_domain::{
    drum_groups::{DrumGroupRequest, DrumVoiceGroups},
    note_groups::NoteGroups,
    voice_allocation::{AllocationOwner, VoiceClaim, VoiceOrder},
};
use radias_synth_infrastructure::prepared::PreparedVoice;
use std::{fs, path::PathBuf};
fn w(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("drum-original-exclusive.bin"))?;
    if raw.len() != 32768 * 1072 {
        return Err("Incomplete original drum lifecycle corpus".into());
    }
    let mut errors = 0;
    let mut selected_actors = 0;
    let plan =
        PreparedVoice::from_program_json(&fs::read(root.join("assets/native-va/sine.json"))?)?;
    let mut pool_cases = 0;
    for (n, r) in raw.chunks_exact(1072).enumerate() {
        let groups = DrumVoiceGroups {
            groups: core::array::from_fn(|i| w(r, 27 + 6 * i + 2) as u8),
        };
        let notes = NoteGroups {
            tags: core::array::from_fn(|i| w(r, 27 + 6 * i + 1) as u8),
            ..Default::default()
        };
        let mut claims = core::array::from_fn(|i| VoiceClaim {
            owner: AllocationOwner(w(r, 27 + 6 * i)),
            note_flags: w(r, 27 + 6 * i + 3) as u8,
            release_flags: w(r, 27 + 6 * i + 4) as u8,
        });
        let mut cached = core::array::from_fn(|i| w(r, 27 + 6 * i + 5) as u16);
        let mut order = VoiceOrder(core::array::from_fn(|i| w(r, 3 + i) as u8));
        let selected = groups.retire(
            DrumGroupRequest {
                owner: AllocationOwner(w(r, 2)),
                event: w(r, 0),
                group: w(r, 1) as u8,
            },
            &notes,
            &mut claims,
            &mut order,
            &mut cached,
        );
        selected_actors += selected.count_ones();
        let different = selected != w(r, 171)
            || order
                .0
                .iter()
                .enumerate()
                .any(|(i, v)| *v != w(r, 172 + i) as u8)
            || claims.iter().enumerate().any(|(i, c)| {
                c.note_flags != w(r, 196 + 3 * i) as u8
                    || c.release_flags != w(r, 197 + 3 * i) as u8
                    || cached[i] != w(r, 198 + 3 * i) as u16
            });
        if different {
            if errors < 3 {
                eprintln!("Exclusive group{n}: mask{selected:x} != {:x}", w(r, 171));
            }
            errors += 1;
        }
        let mut pool = PolyphonicRenderer::default();
        let native_owner = |owner: u32| AllocationOwner(1 + (owner - w(r, 2)) / 4);
        let mut initial_claims = core::array::from_fn(|i| VoiceClaim {
            owner: native_owner(w(r, 27 + 6 * i)),
            note_flags: w(r, 30 + 6 * i) as u8,
            release_flags: w(r, 31 + 6 * i) as u8,
        });
        for (slot, claim) in initial_claims.iter().enumerate() {
            let timbre = claim.owner.0 as u8 - 1;
            let voice = ActiveVoice {
                uses_program_common: true,
                drum_pitch: None,
                drum_instrument: None,
                drum_filter2: None,
                renderer: VoiceRenderer::new(plan.initial, plan.parameters),
                amplifier: None,
                modulation: None,
                auxiliary: None,
                pan: None,
                mixer: None,
                secondary: None,
                primary: None,
                shaper: None,
                comb_program: None,
                timbre,
                note: claim.note_flags & 127,
                velocity: 100,
                held: claim.note_flags & 128 != 0,
                program: 0,
                bus: VoiceBus::new(timbre).unwrap(),
            };
            pool.install(slot, 0, voice);
            pool.bind_drum_group(slot, groups.groups[slot]);
            pool.allocator.budget.costs[slot] = w(r, 32 + 6 * slot) as u16;
        }
        pool.allocator.claims = initial_claims;
        pool.allocator.order = VoiceOrder(core::array::from_fn(|i| w(r, 3 + i) as u8));
        pool.initialize_note_groups(notes);
        let retired = pool.retire_drum_group(0, w(r, 0), w(r, 1) as u8);
        let mut pool_different = retired != w(r, 171) || pool.allocator.order != order;
        for (slot, claim) in initial_claims.iter_mut().enumerate() {
            claim.note_flags = w(r, 196 + 3 * slot) as u8;
            claim.release_flags = w(r, 197 + 3 * slot) as u8;
            pool_different |= pool.allocator.claims[slot] != *claim
                || pool.active_voice(slot).is_none() != (retired & (1 << slot) != 0)
                || pool.allocator.budget.costs[slot] != w(r, 198 + 3 * slot) as u16;
            if retired & (1 << slot) == 0 {
                pool_different |= pool.drum_group(slot)
                    != Some((groups.groups[slot], w(r, 198 + 3 * slot) as u16));
            }
        }
        if pool_different {
            if errors < 3 {
                eprintln!("Production exclusive group{n} differs");
            }
            errors += 1;
        }
        pool_cases += 1;
    }
    let passed = errors == 0;
    let report = serde_json::json!({"passed":passed,"original_exclusive_group_calls":32768,
        "selected_actors":selected_actors,"errors":errors,"physical_flags_cost_reset_and_order_qualified":true,
        "production_pool_cases":pool_cases,"production_pool_actor_removal_and_unselected_context_preserved":true,
        "original_instructions_modified":false,"original_DSP_cleanup_subcall_excluded":true,
        "full_drum_audio_lifecycle_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("drum-lifecycle-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native drum exclusive lifecycle differs".into());
    }
    Ok(())
}
