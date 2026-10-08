//! Production held-owner restart against original controller metadata.
use radias_synth_application::{
    VoiceRenderer,
    modulation::ModulationProgram,
    polyphony::{ActiveVoice, PolyphonicRenderer},
};
use radias_synth_domain::{
    mono_notes::VoiceMode,
    note_groups::NoteGroups,
    pan::VoiceBus,
    sustain::SustainState,
    voice_allocation::{AllocationOwner, VoiceAllocator, VoiceClaim, VoiceOrder},
};
use radias_synth_infrastructure::prepared::PreparedVoice;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("mono-retrigger.bin"))?;
    if raw.len() != 8192 * 1284 {
        return Err("Original Mono restart corpus incomplete".into());
    }
    let plan =
        PreparedVoice::from_program_json(&fs::read(root.join("assets/native-va/saw.json"))?)?;
    let mut errors = 0;
    let mut pool_cases = 0;
    let mut restarted_actors = 0;
    for (index, row) in raw.chunks_exact(1284).enumerate() {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let mut allocator = VoiceAllocator {
            order: VoiceOrder(core::array::from_fn(|i| w(149 + i) as u8)),
            ..Default::default()
        };
        for i in 0..24 {
            allocator.budget.costs[i] = w(29 + 5 * i) as u16;
            allocator.claims[i] = VoiceClaim {
                owner: AllocationOwner(w(3 + i) + 1),
                note_flags: w(30 + 5 * i) as u8,
                release_flags: w(31 + 5 * i) as u8,
            };
        }
        let before = allocator.clone();
        let selected = allocator.retrigger_mono(AllocationOwner(1), w(0) as u8 & 127, w(1) as u16);
        let expected_costs = core::array::from_fn::<_, 24, _>(|i| w(177 + 5 * i) as u16);
        let expected_flags =
            core::array::from_fn::<_, 24, _>(|i| (w(178 + 5 * i) as u8, w(179 + 5 * i) as u8));
        let expected_order = VoiceOrder(core::array::from_fn(|i| w(297 + i) as u8));
        if selected != w(173)
            || allocator.budget.costs != expected_costs
            || allocator.order != expected_order
            || (0..24).any(|i| {
                (
                    allocator.claims[i].note_flags,
                    allocator.claims[i].release_flags,
                ) != expected_flags[i]
            })
        {
            if errors < 3 {
                eprintln!(
                    "Original restart {index}: selected {selected:x}, expected {:x}",
                    w(173)
                );
                eprintln!(
                    "costs {:?} vs {:?}; order {:?} vs {:?}",
                    allocator.budget.costs, expected_costs, allocator.order, expected_order
                );
                for (i, expected) in expected_flags.iter().enumerate() {
                    let actual = (
                        allocator.claims[i].note_flags,
                        allocator.claims[i].release_flags,
                    );
                    if actual != *expected {
                        eprintln!("claim{i}: {actual:?} vs {expected:?}");
                    }
                }
            }
            errors += 1;
        }
        if selected == 0 {
            continue;
        }
        let make_voice = |timbre, note, held| ActiveVoice {
            uses_program_common: false,
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
            note,
            velocity: 100,
            held,
            program: 0,
            bus: VoiceBus::new(timbre).unwrap(),
        };
        let mut pool = PolyphonicRenderer::default();
        pool.set_voice_mode(0, VoiceMode::from_raw(64));
        for slot in 0..24 {
            let claim = before.claims[slot];
            pool.install(
                slot,
                0,
                make_voice(
                    (claim.owner.0 - 1) as u8,
                    claim.note_flags & 127,
                    claim.note_flags & 128 != 0,
                ),
            );
        }
        pool.allocator = before;
        pool.initialize_sustain_states([
            SustainState { flags: w(27) as u8 },
            Default::default(),
            Default::default(),
            Default::default(),
        ]);
        pool.initialize_note_groups(NoteGroups {
            counter: (w(2) as u16).wrapping_sub(1),
            ages: core::array::from_fn(|i| w(32 + 5 * i) as u16),
            tags: core::array::from_fn(|i| w(33 + 5 * i) as u8),
        });
        pool.begin_note_event((w(0) >> 24) as u8);
        let assignment = pool
            .retrigger_modulated(
                make_voice(0, w(0) as u8 & 127, true),
                w(1) as u16,
                ModulationProgram::default(),
            )
            .ok_or("Held restart absent")?;
        let groups = pool.note_groups();
        if assignment.displaced != 0
            || assignment.slot as u32 != selected.trailing_zeros()
            || pool.allocator.budget.costs != expected_costs
            || pool.allocator.order != expected_order
            || pool.sustain_state(0).unwrap().flags as u32 != w(175)
            || (0..24).any(|i| {
                groups.ages[i] as u32 != w(180 + 5 * i)
                    || groups.tags[i] as u32 != w(181 + 5 * i)
                    || (
                        pool.allocator.claims[i].note_flags,
                        pool.allocator.claims[i].release_flags,
                    ) != expected_flags[i]
                    || pool.active_voice(i).is_none()
            })
        {
            if errors < 3 {
                eprintln!("Production restart {index} metadata differs");
            }
            errors += 1;
        }
        restarted_actors += selected.count_ones();
        pool_cases += 1;
    }
    let report = serde_json::json!({"passed":errors==0,"original_cases":8192,"production_pool_cases":pool_cases,"restarted_physical_actors":restarted_actors,
        "errors":errors,"no_allocation_or_reclamation_in_held_restart":true,"native_actor_restart_and_group_metadata_used":true,
        "source_DSP_HPI_subcalls_excluded":true,"multi_actor_controller_initializer_order_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("mono-retrigger-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Held Mono restart differs".into());
    }
    println!(
        "8192 original held restart metadata cases match, {pool_cases} through production pool"
    );
    Ok(())
}
