use radias_synth_domain::filter_control::compile;
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};
fn word(raw: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.filter_mix()?;
    let raw = fs::read(root.join("runs/native-clone/original-filter-controls.bin"))?;
    let mut errors = 0;
    for (i, r) in raw.chunks_exact(28).enumerate() {
        let actual = compile(word(r, 0) as i32, word(r, 1) as i32, word(r, 2) as i32);
        if actual.feedback != word(r, 3) as i32
            || actual.integrator_gain != word(r, 4) as i32
            || actual.post_gain != word(r, 5) as i16
            || actual.post_feedback != word(r, 6) as i16
        {
            if errors < 3 {
                eprintln!(
                    "Filter control {i}: {actual:?} != {:?}",
                    [word(r, 3), word(r, 4), word(r, 5), word(r, 6)]
                );
            }
            errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/original-filter-mix.bin"))?;
    for (i, r) in raw.chunks_exact(24).enumerate() {
        let a = table.weights(word(r, 0) as u16);
        let e = core::array::from_fn(|n| word(r, 1 + n) as i16);
        if a != e {
            if errors < 3 {
                eprintln!("Filter mix {i}: {a:?} != {e:?}");
            }
            errors += 1;
        }
    }
    println!("{{\"filter_control_cases\":32768,\"filter_mix_cases\":32768,\"errors\":{errors}}}");
    if errors != 0 {
        return Err("Filter control parity failed".into());
    }
    Ok(())
}
