use radias_synth_domain::{
    bandlimit::edge_coefficient,
    pitch::{PhaseIncrement, PitchCode},
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};
fn word(raw: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.bandwidth()?;
    let raw = fs::read(root.join("runs/native-clone/original-bandwidth.bin"))?;
    let mut errors = 0;
    for (i, r) in raw.chunks_exact(8).enumerate() {
        let actual = table.coefficient(PhaseIncrement(word(r, 0)));
        if actual != word(r, 1) as i16 {
            if errors < 3 {
                eprintln!("Bandwidth {i}: {actual} != {}", word(r, 1) as i16);
            }
            errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/original-edges.bin"))?;
    for (i, r) in raw.chunks_exact(12).enumerate() {
        let actual = edge_coefficient(
            PitchCode::new(word(r, 0) as u16).ok_or("Invalid pitch")?,
            word(r, 1) != 0,
        );
        if actual != word(r, 2) as i16 {
            if errors < 3 {
                eprintln!("Edge {i}: {actual} != {}", word(r, 2) as i16);
            }
            errors += 1;
        }
    }
    println!("{{\"bandwidth_cases\":32768,\"edge_cases\":65536,\"errors\":{errors}}}");
    if errors != 0 {
        return Err("Bandwidth parity failed".into());
    }
    Ok(())
}
