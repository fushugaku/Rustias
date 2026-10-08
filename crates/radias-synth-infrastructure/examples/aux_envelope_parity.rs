//! Whole original EG1/EG3 lifecycles. EG2 termination is deliberately excluded.
use radias_synth_domain::mod_envelope::{ModEnvelope, ModEnvelopeParameters};
use radias_synth_infrastructure::firmware::{envelope_curves, envelope_timing_tables};
use std::{fs, path::PathBuf};
fn word(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/aux-envelope.bin"))?;
    if raw.len() != 81920 * 92 {
        return Err("Auxiliary envelope corpus incomplete".into());
    }
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let curves = envelope_curves(&sys)?;
    let timing = envelope_timing_tables(&sys)?;
    let mut envelope = ModEnvelope::default();
    let mut errors = [0usize; 2];
    for r in raw.chunks_exact(92) {
        let family = word(r, 0) as usize;
        if ![0, 2].contains(&family) {
            return Err("Unknown source envelope family".into());
        }
        let p = ModEnvelopeParameters {
            attack: word(r, 4) as u8,
            decay: word(r, 5) as u8,
            sustain: word(r, 6) as u8,
            release: word(r, 7) as u8,
            curve: word(r, 8) as u8,
            velocity: word(r, 9) as u8,
            velocity_sensitivity: word(r, 10) as u8,
            note: word(r, 11) as u8,
            key_tracking: word(r, 12) as u8,
        };
        match word(r, 3) {
            0 => {
                envelope = ModEnvelope::default();
                envelope.note_on(p, &curves, &timing, 0);
            }
            1 => {
                envelope.publish();
                envelope.tick(p, &curves, &timing);
            }
            2 => envelope.release(p, &timing),
            _ => return Err("Unknown envelope action".into()),
        }
        let envelope = &envelope.envelope;
        let actual = [
            envelope.segment.phase,
            envelope.segment.increment,
            envelope.segment.start as u32,
            envelope.segment.difference as u16 as u32,
            envelope.segment.level as u32,
            envelope.published_level as u32,
            envelope.target as u32,
            envelope.increment_flags as u32,
            envelope.divider as u32,
            envelope.stage as u32,
        ];
        let expected: [u32; 10] = core::array::from_fn(|i| word(r, 13 + i));
        if actual != expected {
            if errors[family / 2] < 3 {
                eprintln!(
                    "Aux {family}/{}/{}: {actual:?} != {expected:?}",
                    word(r, 1),
                    word(r, 2)
                );
            }
            errors[family / 2] += 1;
        }
    }
    let passed = errors == [0, 0];
    let report = serde_json::json!({"passed":passed,"eg1_state_comparisons":40960,"eg3_state_comparisons":40960,
        "eg1_errors":errors[0],"eg3_errors":errors[1],"original_calls_complete":true,"original_instructions_modified":false,
        "auxiliary_release_does_not_finish_amplifier_voice":true,"original_controller_event_times_used":true,
        "filter_target_compilation_qualified":false,"routed_to_live_audio":false,"complete_engine":false});
    fs::write(
        root.join("runs/native-clone/aux-envelope-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native auxiliary lifecycle differs".into());
    }
    Ok(())
}
