use radias_synth_domain::{
    Sample,
    pan::{StereoFrame, route},
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or(".".into()));
    let raw = fs::read(root.join("runs/native-clone/original-pan.bin"))?;
    let mut errors = 0;
    for (i, r) in raw.chunks_exact(24).enumerate() {
        let word = |i: usize| u32::from_le_bytes(r[i * 4..i * 4 + 4].try_into().unwrap()) as i32;
        let frame = route(
            Sample(word(0)),
            word(1),
            StereoFrame {
                left: Sample(word(2)),
                right: Sample(word(3)),
            },
        );
        if frame.left.0 != word(4) || frame.right.0 != word(5) {
            if errors < 3 {
                eprintln!("Pan {i}: {frame:?} != {}, {}", word(4), word(5));
            }
            errors += 1;
        }
    }
    println!("{{\"pan_cases\":32768,\"errors\":{errors}}}");
    if errors != 0 {
        return Err("Pan parity failed".into());
    }
    Ok(())
}
