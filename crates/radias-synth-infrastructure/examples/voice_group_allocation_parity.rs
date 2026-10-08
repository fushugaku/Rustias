//! Original complete Poly/Mono group allocation bookkeeping boundaries.
use radias_synth_application::{
    VoiceRenderer,
    modulation::ModulationProgram,
    polyphony::{ActiveVoice, PolyphonicRenderer},
};
use radias_synth_domain::pan::VoiceBus;
use radias_synth_domain::{
    note_groups::NoteGroups,
    voice_allocation::{AllocationOwner, VoiceAllocator, VoiceClaim, VoiceOrder},
    voice_group::{VoiceGroupProgram, VoiceGroupSlots},
};
use radias_synth_infrastructure::{firmware, prepared::PreparedVoice};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("voice-group-allocation.bin"))?;
    if raw.len() != 8192 * 1964 {
        return Err("Original voice-group allocation corpus incomplete".into());
    }
    let mut errors = 0;
    let mut selected = 0;
    let mut pressure = 0;
    let mut pool_cases = 0;
    let plan =
        PreparedVoice::from_program_json(&fs::read(root.join("assets/native-va/saw.json"))?)?;
    let tables =
        firmware::voice_group_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    for (index, row) in raw.chunks_exact(1964).enumerate() {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let mut allocator = VoiceAllocator {
            order: VoiceOrder(core::array::from_fn(|i| w(224 + i) as u8)),
            ..Default::default()
        };
        allocator.budget.master_overhead = w(6) as u16;
        let mut slots = VoiceGroupSlots {
            timbres: core::array::from_fn(|i| w(12 + 9 * i) as u8),
            indices: core::array::from_fn(|i| w(13 + 9 * i) as u8),
            stereo: core::array::from_fn(|i| w(14 + 9 * i) as u8),
        };
        let mut groups = NoteGroups {
            counter: w(4) as u16,
            ages: core::array::from_fn(|i| w(15 + 9 * i) as u16),
            tags: core::array::from_fn(|i| w(16 + 9 * i) as u8),
        };
        for i in 0..24 {
            allocator.budget.costs[i] = w(8 + 9 * i) as u16;
            allocator.claims[i] = VoiceClaim {
                owner: AllocationOwner(w(9 + 9 * i) + 1),
                note_flags: w(10 + 9 * i) as u8,
                release_flags: w(11 + 9 * i) as u8,
            };
        }
        let original_allocator = allocator.clone();
        let original_slots = slots;
        let original_groups = groups;
        let layout = VoiceGroupProgram {
            raw: w(1) as u8,
            ..Default::default()
        }
        .layout(w(2) as u8);
        let assignment = if w(0) == 0 {
            allocator.allocate_poly_group(
                AllocationOwner(1),
                w(7) as u8,
                w(3) as u8 & 127,
                w(5) as u16,
                layout,
                &mut slots,
            )
        } else {
            allocator.allocate_mono_group(
                AllocationOwner(1),
                w(7) as u8,
                w(3) as u8 & 127,
                w(5) as u16,
                layout,
                &groups.ages,
                &mut slots,
            )
        }
        .ok_or("Native group allocation absent")?;
        groups.assign(assignment.selected, w(3));
        selected += assignment.selected.count_ones();
        pressure += u32::from(assignment.displaced != 0);
        let mut actual = Vec::with_capacity(241);
        let mut expected = Vec::with_capacity(241);
        for i in 0..24 {
            actual.extend_from_slice(&[
                allocator.budget.costs[i] as u32,
                allocator.claims[i].owner.0 - 1,
                allocator.claims[i].note_flags as u32,
                allocator.claims[i].release_flags as u32,
                slots.timbres[i] as u32,
                slots.indices[i] as u32,
                slots.stereo[i] as u32,
                groups.ages[i] as u32,
                groups.tags[i] as u32,
            ]);
            expected.extend((0..9).map(|n| w(251 + 9 * i + n)));
        }
        actual.extend(allocator.order.0.map(u32::from));
        expected.extend((0..24).map(|i| w(467 + i)));
        let masks_match =
            assignment.selected == w(248) && (w(0) != 0 || assignment.displaced == w(249));
        if actual != expected || !masks_match || assignment.bank as u32 != w(250) {
            if errors < 3 {
                eprintln!(
                    "group{index}/mode{} masks{:x}/{:x} vs{:x}/{:x}, bank{} vs{}",
                    w(0),
                    assignment.selected,
                    assignment.displaced,
                    w(248),
                    w(249),
                    assignment.bank,
                    w(250)
                );
                for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
                    if a != e {
                        eprintln!(" field{i}: {a} vs {e}");
                    }
                }
            }
            errors += 1;
        }
        let timbre = w(7) as u8;
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
        pool.configure_voice_groups(tables);
        pool.edit_voice_group(
            timbre,
            VoiceGroupProgram {
                raw: w(1) as u8,
                ..Default::default()
            },
        );
        pool.set_voice_mode(
            timbre,
            radias_synth_domain::mono_notes::VoiceMode::from_raw(if w(0) == 0 { 128 } else { 64 }),
        );
        for slot in 0..24 {
            let c = original_allocator.claims[slot];
            let owner = ((c.owner.0 - 1 + timbre as u32) % 4) as u8;
            pool.install(
                slot,
                0,
                make_voice(owner, c.note_flags & 127, c.note_flags & 128 != 0),
            );
        }
        pool.allocator = original_allocator;
        for c in &mut pool.allocator.claims {
            c.owner = AllocationOwner((c.owner.0 - 1 + timbre as u32) % 4 + 1);
        }
        pool.initialize_voice_group_slots(original_slots);
        pool.initialize_note_groups(NoteGroups {
            counter: original_groups.counter.wrapping_sub(1),
            ..original_groups
        });
        pool.begin_note_event((w(3) >> 24) as u8);
        let mut template = make_voice(timbre, w(3) as u8 & 127, true);
        template.primary = Some(radias_synth_application::primary::PrimaryProgram {
            selection: w(2) as u8,
            ..Default::default()
        });
        let produced = pool
            .trigger_modulated(template, w(5) as u16, ModulationProgram::default())
            .ok_or("Production group constructor absent")?;
        let meta = pool.voice_group_slots();
        let ng = pool.note_groups();
        if produced.slot as u32 != assignment.selected.trailing_zeros()
            || pool.allocator.order != allocator.order
            || meta != slots
            || ng != groups
            || pool.allocator.budget.costs != allocator.budget.costs
            || (0..24).any(|i| {
                let c = pool.allocator.claims[i];
                let e = allocator.claims[i];
                (c.owner.0 - 1 + 4 - timbre as u32) % 4 != e.owner.0 - 1
                    || c.note_flags != e.note_flags
                    || c.release_flags != e.release_flags
                    || (assignment.selected & (1 << i) != 0
                        && (pool.voice_group_bank(i) != assignment.bank
                            || pool
                                .active_voice(i)
                                .is_none_or(|v| v.note != w(3) as u8 & 127 || !v.held)))
            })
        {
            if errors < 3 {
                eprintln!("Production group{index}/mode{} differs", w(0));
            }
            errors += 1;
        }
        pool_cases += 1;
    }
    let report = serde_json::json!({"passed":errors==0,"original_group_allocations":8192,"selected_physical_actors":selected,"pressure_cases":pressure,"errors":errors,
        "original_costs_order_ages_tags_claims_ordinals_stereo_and_masks_compared":true,"source_DSP_HPI_cleanup_subcalls_excluded":true,
        "Mono_selected_actor_displacement_mask_not_qualified_by_metadata_boundary":true,"production_pool_group_cases":pool_cases,"complete_native_engine":false});
    fs::write(
        out.join("voice-group-allocation-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Original group allocation bookkeeping differs".into());
    }
    println!("8192 original Poly/Mono group allocation cases match");
    Ok(())
}
