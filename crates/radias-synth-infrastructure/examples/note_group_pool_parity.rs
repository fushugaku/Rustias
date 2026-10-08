//! Production pool release selection with real actor objects and source ages.
use radias_synth_application::{
    VoiceRenderer,
    polyphony::{ActiveVoice, PolyphonicRenderer},
};
use radias_synth_domain::{
    note_groups::NoteGroups,
    pan::VoiceBus,
    voice_allocation::{AllocationOwner, VoiceClaim, VoiceOrder},
};
use radias_synth_infrastructure::prepared::PreparedVoice;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let plan =
        PreparedVoice::from_program_json(&fs::read(root.join("assets/native-va/saw.json"))?)?;
    let raw = fs::read(out.join("note-groups-select.bin"))?;
    if raw.len() != 32768 * 584 {
        return Err("Original group selection corpus incomplete".into());
    }
    let mut errors = 0;
    let mut released = 0;
    for (index, row) in raw.chunks_exact(584).enumerate() {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let mut pool = PolyphonicRenderer::default();
        for slot in 0..24 {
            let timbre = w(25 + 4 * slot) as u8;
            let nf = w(26 + 4 * slot) as u8;
            pool.install(
                slot,
                0,
                ActiveVoice {
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
                    note: nf & 127,
                    velocity: 100,
                    held: nf & 128 != 0,
                    program: 0,
                    bus: VoiceBus::new(timbre).unwrap(),
                },
            );
            pool.allocator.claims[slot] = VoiceClaim {
                owner: AllocationOwner(timbre as u32 + 1),
                note_flags: nf,
                release_flags: 0,
            };
        }
        pool.allocator.order = VoiceOrder(core::array::from_fn(|i| w(1 + i) as u8));
        pool.initialize_note_groups(NoteGroups {
            counter: 0,
            ages: core::array::from_fn(|i| w(28 + 4 * i) as u16),
            tags: core::array::from_fn(|i| w(27 + 4 * i) as u8),
        });
        pool.begin_note_event((w(0) >> 24) as u8);
        pool.release_note(0, w(0) as u8 & 127, None);
        let mut mask = 0;
        for slot in 0..24 {
            if w(26 + 4 * slot) & 128 != 0 && !pool.active_voice(slot).unwrap().held {
                mask |= 1u32 << slot;
            }
        }
        released += mask.count_ones();
        if mask != w(121)
            || (0..24).any(|i| pool.allocator.claims[i].note_flags as u32 != w(122 + i))
        {
            if errors < 3 {
                eprintln!("Pool group{index} mask {mask:x} expected {:x}", w(121));
            }
            errors += 1;
        }
    }
    let report = serde_json::json!({"passed":errors==0,"original_release_events":32768,"physical_actor_releases":released,"errors":errors,
        "production_pool_actor_and_sustain_release_used":true,"all_owner_tag_age_and_queue_matching_from_original":true,
        "source_envelope_HPI_timing_not_qualified_by_flag_gate":true,"complete_native_engine":false});
    fs::write(
        out.join("note-group-pool-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Production repeated-note release differs".into());
    }
    println!("32768 source repeated-note/group masks match live actor pool");
    Ok(())
}
