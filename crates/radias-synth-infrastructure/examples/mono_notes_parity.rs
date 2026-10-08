//! Exact original six-note queue storage and packed event results.
use radias_synth_domain::mono_notes::{MonoAction, MonoNotes, NotePriority, VoiceMode};
use std::{fs, path::PathBuf};
fn operation(queue: &mut MonoNotes, action: u32, event: u32) -> [u32; 9] {
    let result = if action == 3 {
        queue.remove(event)
    } else {
        queue.insert(
            match action {
                0 => NotePriority::Last,
                1 => NotePriority::Lowest,
                2 => NotePriority::Highest,
                _ => unreachable!(),
            },
            event,
        )
    };
    let mut state = [0; 9];
    state[0] = result.event;
    state[1] = result.previous;
    for (i, word) in queue.entries.iter().enumerate() {
        state[i + 2] = *word as u32;
    }
    state[8] = queue.velocity as u32;
    state
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let mut errors = [0; 2];
    for (family, width, count) in [("scalar", 18, 131072), ("lifecycle", 14, 65536)] {
        let raw = fs::read(out.join(format!("mono-notes-{family}.bin")))?;
        if raw.len() != width * 4 * count {
            return Err("Original mono queue corpus incomplete".into());
        }
        let mut queue = MonoNotes::default();
        for (index, row) in raw.chunks_exact(width * 4).enumerate() {
            let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
            let (action, event, start, group) = if family == "scalar" {
                queue = MonoNotes {
                    entries: core::array::from_fn(|i| w(i + 2) as u16),
                    velocity: w(8) as u8,
                };
                (w(0), w(1), 9, 0)
            } else {
                if w(2) == 0 {
                    queue = Default::default();
                }
                (w(3), w(4), 5, 1)
            };
            let actual = operation(&mut queue, action, event);
            let expected = core::array::from_fn::<_, 9, _>(|i| w(start + i));
            if actual != expected {
                if errors[group] < 3 {
                    eprintln!("{family} {index}: {actual:?} vs {expected:?}");
                }
                errors[group] += 1;
            }
        }
    }
    let raw = fs::read(out.join("mono-notes-decisions.bin"))?;
    if raw.len() != 65536 * 76 {
        return Err("Original mono action corpus incomplete".into());
    }
    let mut decision_errors = 0;
    for (index, row) in raw.chunks_exact(76).enumerate() {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let mut queue = MonoNotes {
            entries: core::array::from_fn(|i| w(3 + i) as u16),
            velocity: w(9) as u8,
        };
        let mode = VoiceMode::from_raw(w(0) as u8);
        let decision = if w(1) == 0 {
            queue.note_on(mode, w(2))
        } else {
            queue.note_off(mode, w(2))
        };
        let action = match decision.action {
            MonoAction::Ignore => 0,
            MonoAction::Allocate => 1,
            MonoAction::Legato => 2,
            MonoAction::Retrigger => 3,
            MonoAction::Release => 4,
        };
        let mut actual = [0; 9];
        actual[0] = action;
        actual[1] = decision.event;
        for (i, entry) in queue.entries.iter().enumerate() {
            actual[2 + i] = *entry as u32;
        }
        actual[8] = queue.velocity as u32;
        let expected = core::array::from_fn::<_, 9, _>(|i| w(10 + i));
        if actual != expected {
            if decision_errors < 3 {
                eprintln!("Decision{index}: {actual:?} vs {expected:?}");
            }
            decision_errors += 1;
        }
    }
    let passed = errors == [0; 2] && decision_errors == 0;
    let r = serde_json::json!({"passed":passed,"scalar_cases":131072,"lifecycle_steps":65536,"errors":errors,
        "original_six_note_capacity_last_low_high_priority_and_tagged_removal":true,
        "original_action_boundary_cases":65536,"decision_errors":decision_errors,
        "live_mono_policy_connected":false,"complete_native_engine":false});
    fs::write(
        out.join("mono-notes-parity.json"),
        serde_json::to_vec_pretty(&r)?,
    )?;
    if !passed {
        return Err("Original mono queue differs".into());
    }
    println!("131072 scalar and65536 lifecycle mono queue cases match");
    Ok(())
}
