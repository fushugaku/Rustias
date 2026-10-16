//! Silent, firmware-free native rendering benchmark. Startup is not timed.
//! Optionally write PCM32 evidence for an exact before/after comparison.
use radias_synth_infrastructure::standalone::StandaloneSynth;
use std::{hint::black_box, io::Write, time::Instant};

fn fixture(name: &str, voices: usize) -> StandaloneSynth {
    let mut synth = StandaloneSynth::new();
    if name == "comb24" {
        for (id, value) in [(20, 1), (21, 3), (22, 75), (23, 40)] {
            assert!(synth.control(0, id, value));
        }
    } else if name == "shaper24" {
        for (id, value) in [(29, 2), (154, 8), (31, 87), (30, 1)] {
            assert!(synth.control(0, id, value));
        }
    }
    for note in 0..voices {
        assert!(synth.note(0, 36 + note as u8, 100));
    }
    assert_eq!(
        synth.engine.active_count(),
        voices,
        "{name}: voice admission"
    );
    synth
}

fn main() {
    let output = std::env::args().nth(1);
    if let Some(dir) = &output {
        std::fs::create_dir_all(dir).unwrap();
    }
    for (name, voices) in [
        ("idle", 0),
        ("saw1", 1),
        ("saw8", 8),
        ("saw24", 24),
        ("comb24", 24),
        ("shaper24", 24),
    ] {
        if let Some(dir) = &output {
            let mut synth = fixture(name, voices);
            let mut file = std::io::BufWriter::new(
                std::fs::File::create(format!("{dir}/{name}.pcm32")).unwrap(),
            );
            for frame in 0..24_000 {
                // Held edits, sustain, release and a same-slot retrigger.
                if frame == 4096 {
                    assert!(synth.control(0, 1, 35));
                }
                if frame == 6144 {
                    assert!(synth.control(0, 66, 1));
                }
                if frame == 8192 {
                    for note in 0..voices {
                        synth.note(0, 36 + note as u8, 0);
                    }
                }
                if frame == 12_288 {
                    assert!(synth.control(0, 66, 0));
                }
                if frame == 16_384 && voices > 0 {
                    synth.note(0, 36, 101);
                }
                if frame == 20_480 {
                    synth.stop();
                }
                let sample = synth.engine.sample();
                file.write_all(&sample.left.0.to_le_bytes()).unwrap();
                file.write_all(&sample.right.0.to_le_bytes()).unwrap();
            }
        }
        let mut times = Vec::new();
        for _ in 0..5 {
            let mut synth = fixture(name, voices);
            for _ in 0..4096 {
                black_box(synth.engine.sample());
            }
            let start = Instant::now();
            for _ in 0..24_000 {
                black_box(synth.engine.sample());
            }
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{}",
            serde_json::json!({"scene":name,"voices":voices,"frames":24_000,"median_ms":times[2],"times_ms":times})
        );
    }
}
