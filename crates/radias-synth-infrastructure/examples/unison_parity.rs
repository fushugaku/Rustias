use radias_synth_domain::{
    Phase,
    pitch::PhaseIncrement,
    unison::{UnisonOscillator, UnisonParameters},
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let raw = fs::read(root.join("runs/native-clone/original-unison.bin"))?;
    if raw.len() != 32768 * 180 {
        return Err("Incomplete original Unison corpus".into());
    }
    let mut errors = 0;
    for (i, r) in raw.chunks_exact(180).enumerate() {
        let word = |n: usize| u32::from_le_bytes(r[n * 4..n * 4 + 4].try_into().unwrap());
        let pair = |n: usize| word(n) << 16 | word(n + 1);
        let mut osc = UnisonOscillator {
            phases: [18, 20, 22, 26, 28].map(|n| Phase(pair(n))),
        };
        let output = osc.next_sample(UnisonParameters {
            detune: 0,
            increments: [0, 6, 8, 12, 14].map(|n| PhaseIncrement(pair(n))),
            correction_gain: word(16) as i16,
            level: word(17) as i16,
        });
        let phases = osc.phases.map(|p| p.0);
        let expected = core::array::from_fn::<_, 5, _>(|n| word(39 + n));
        if output.0 != word(38) as i32 || phases != expected || phases[0] != word(44) {
            if errors < 3 {
                eprintln!(
                    "Unison {i}: {output:?}, {phases:?} != {}, {expected:?}",
                    word(38) as i32
                );
            }
            errors += 1;
        }
    }
    let report = serde_json::json!({"scope":"Original five-phase unison ramp C584..C667","cases":32768,"errors":errors,"passed":errors==0});
    fs::write(
        root.join("runs/native-clone/unison-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Native Unison parity failed".into());
    }
    Ok(())
}
