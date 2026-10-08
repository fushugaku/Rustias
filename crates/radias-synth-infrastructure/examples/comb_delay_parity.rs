//! Independent native exchange on original sample inputs; no incoming DMA data replay.
use radias_synth_domain::{Sample, comb::CombDelay};
use std::{fs, path::PathBuf};
fn word(row: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("Repository required")?);
    for name in args {
        let out = root.join("runs/native-clone");
        let raw = fs::read(out.join(format!("{name}-comb-samples.bin")))?;
        if raw.len() % 48 != 0 || raw.len() < 96 {
            return Err("Original Comb exchange inputs incomplete".into());
        }
        let initial = fs::read(out.join(format!("{name}-comb-initial-delay-memory.bin")))?;
        if initial.len() != 8192 {
            return Err("Original delay memory snapshot incomplete".into());
        }
        let mut delay = CombDelay {
            write_cursor_bytes: word(&raw, 2) as u16,
            group_phase: word(&raw, 1) as u8,
            read_samples: [word(&raw, 5) as i16, word(&raw, 6) as i16],
            fraction: word(&raw, 7) as i16,
            ..Default::default()
        };
        for (sample, bytes) in delay.samples.iter_mut().zip(initial.chunks_exact(2)) {
            *sample = i16::from_le_bytes(bytes.try_into().unwrap());
        }
        let mut errors = [0usize; 5];
        let frames = raw.len() / 48;
        for n in 0..frames - 1 {
            let r = &raw[n * 48..n * 48 + 48];
            let a = [
                delay.group_phase as u32,
                delay.write_cursor_bytes as u32,
                delay.read_samples[0] as u16 as u32,
                delay.read_samples[1] as u16 as u32,
                delay.fraction as u16 as u32,
            ];
            let expected = [word(r, 1), word(r, 2), word(r, 5), word(r, 6), word(r, 7)];
            for field in 0..5 {
                if a[field] != expected[field] {
                    if errors[field] < 2 {
                        eprintln!(
                            "{name} frame{n} field{field}:{} vs{}",
                            a[field], expected[field]
                        );
                    }
                    errors[field] += 1;
                }
            }
            delay.begin_frame();
            let next = &raw[(n + 1) * 48..(n + 2) * 48];
            delay.finish_frame(Sample(word(next, 10) as i32), word(r, 4));
        }
        let passed = errors.iter().all(|&n| n == 0);
        let report = serde_json::json!({"passed":passed,"name":name,"frames":frames-1,"errors":errors,"native_four_sample_delay_staging_used":true,"original_initial_delay_memory_and_actor_state_used":true,"original_prepared_output_samples_used_as_exchange_inputs":true,"original_incoming_delay_samples_replayed":false,"complete_voice_audio_parity":false});
        fs::write(
            out.join(format!("{name}-comb-delay-parity.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        if !passed {
            return Err("Native Comb delay exchange mismatch".into());
        }
    }
    Ok(())
}
