use radias_synth_domain::{Sample, pan::StereoFrame, stereo_cache::StereoCache};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/stereo-cache.bin"))?;
    if raw.len() != 65536 * 36 {
        return Err("Original stereo cache corpus incomplete".into());
    }
    let mut errors = [0usize; 4];
    for (index, row) in raw.chunks_exact(36).enumerate() {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mut cache = StereoCache {
            gain: w(0) as i16,
            samples: StereoFrame {
                left: Sample(w(1) as i32),
                right: Sample(w(2) as i32),
            },
        };
        let output = cache.advance(StereoFrame {
            left: Sample(w(3) as i32),
            right: Sample(w(4) as i32),
        });
        let actual = [
            cache.samples.left.0 as u32,
            cache.samples.right.0 as u32,
            output.left.0 as u32,
            output.right.0 as u32,
        ];
        for (field, error) in errors.iter_mut().enumerate() {
            if actual[field] != w(5 + field) {
                if *error < 3 {
                    eprintln!(
                        "Stereo cache case{index} field{field}:{} vs{}",
                        actual[field],
                        w(5 + field)
                    );
                }
                *error += 1;
            }
        }
    }
    let gains = fs::read(root.join("runs/native-clone/stereo-cache-gain.bin"))?;
    if gains.len() != 65536 * 8 {
        return Err("Original cache gain corpus incomplete".into());
    }
    let mut gain_errors = 0usize;
    for row in gains.chunks_exact(8) {
        let input = u32::from_le_bytes(row[..4].try_into().unwrap());
        let expected = u32::from_le_bytes(row[4..].try_into().unwrap());
        let mut cache = StereoCache::default();
        cache.set_decay(input as i16);
        if cache.gain as u16 as u32 != expected {
            gain_errors += 1;
        }
    }
    let initialize = fs::read(root.join("runs/native-clone/stereo-cache-initialize.bin"))?;
    if initialize.len() != 65536 * 36 {
        return Err("Original cache initialization corpus incomplete".into());
    }
    let mut initialize_errors = [0usize; 6];
    for row in initialize.chunks_exact(36) {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let mut cache = StereoCache::default();
        cache.initialize(Sample(w(2) as i32), w(1) as i16, w(0) as i16);
        let left_gain = (32767 - i32::from(w(1) as i16)).min(32767) as i16;
        let actual = [
            cache.gain as u16 as u32,
            cache.samples.left.0 as u32,
            cache.samples.right.0 as u32,
            left_gain as u16 as u32,
            0,
            0,
        ];
        for (field, error) in initialize_errors.iter_mut().enumerate() {
            if actual[field] != w(3 + field) {
                if *error < 2 {
                    eprintln!(
                        "Cache initialize field{field}:{} vs{}",
                        actual[field],
                        w(3 + field)
                    );
                }
                *error += 1;
            }
        }
    }
    let passed = errors == [0; 4] && gain_errors == 0 && initialize_errors == [0; 6];
    let report = serde_json::json!({"passed":passed,"original_calls":65536,"errors":errors,"source_entry":"MasterA0EC",
        "cached_stereo_transition_and_bus_saturation_exact":passed,"original_gain_compiler_calls":65536,
        "gain_errors":gain_errors,"DSP_gain_compilation_qualified":gain_errors==0,
        "original_cache_initializer_calls":65536,"cache_initializer_errors":initialize_errors,
        "SH_controller_gain_input_compilation_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/stereo-cache-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native stereo cache mismatch".into());
    }
    Ok(())
}
