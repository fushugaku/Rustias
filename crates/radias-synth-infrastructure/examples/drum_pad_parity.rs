use radias_synth_domain::{
    drum::DrumProgram,
    drum_pad::{DrumPadInput, DrumPadState},
};
use std::{fs, path::PathBuf};
fn w(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("drum-original-pad.bin"))?;
    if raw.len() != 65536 * 296 {
        return Err("Incomplete original direct-pad corpus".into());
    }
    let mut errors = 0;
    let mut emitted = 0;
    for (n, r) in raw.chunks_exact(296).enumerate() {
        let mut state = DrumPadState {
            note_flags: core::array::from_fn(|i| w(r, 7 + 2 * i) as u8),
            channels: core::array::from_fn(|i| w(r, 8 + 2 * i) as u8),
        };
        let event = state.input(
            DrumProgram::from_raw(w(r, 0) as u8, 100, 64, w(r, 2) as u8),
            DrumPadInput {
                instrument: w(r, 1) as u8,
                velocity: w(r, 5) as u8,
                owning_timbre_enabled: w(r, 3) != 0,
                owning_channel: w(r, 4) as u8,
                key: w(r, 6) as u8,
            },
        );
        let native = [
            u32::from(event.is_some()),
            event.map_or(0, |e| e.event),
            event.map_or(255, |e| e.instrument as u32),
        ];
        let different = native != [w(r, 39), w(r, 40), w(r, 41)]
            || (0..16).any(|i| {
                state.note_flags[i] != w(r, 42 + 2 * i) as u8
                    || state.channels[i] != w(r, 43 + 2 * i) as u8
            });
        if different {
            if errors < 3 {
                eprintln!(
                    "Drum pad{n}: {native:?} != {:?}",
                    [w(r, 39), w(r, 40), w(r, 41)]
                );
            }
            errors += 1;
        }
        emitted += usize::from(event.is_some());
    }
    let passed = errors == 0;
    let report = serde_json::json!({"passed":passed,"original_direct_pad_cases":65536,
        "emitted_events":emitted,"errors":errors,"retained_key_channel_and_all16_slot_state_qualified":true,
        "original_instructions_modified":false,"original_note_action_subcall_excluded":true,
        "original_complete_audio_parity_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("drum-pad-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native direct-pad state/event differs".into());
    }
    Ok(())
}
