use radias_synth_domain::modulation::{
    ControllerSources, ModulationDestination, ModulationSource, VirtualPatch,
};
use radias_synth_infrastructure::firmware::{amplifier_tables, modulation_tables};
use std::{fs, path::PathBuf};
fn word(raw: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(raw[n * 4..n * 4 + 4].try_into().unwrap())
}
fn sources(r: &[u8]) -> ControllerSources {
    ControllerSources {
        envelope_levels: core::array::from_fn(|i| word(r, i) as u16),
        envelope_velocity_sensitivity: core::array::from_fn(|i| word(r, 3 + i) as u8),
        lfo: [word(r, 6) as i16, word(r, 7) as i16],
        velocity: word(r, 8) as u8,
        bend: word(r, 9) as i16,
        wheel: word(r, 10) as u8,
        relative_pitch: word(r, 11) as i16,
        auxiliary: word(r, 12) as i16,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let output = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = modulation_tables(&sys)?;
    let gain = amplifier_tables(&sys)?;
    let raw = fs::read(output.join("modulation-scales.bin"))?;
    if raw.len() != 163840 * 24 {
        return Err("Incomplete modulation scale corpus".into());
    }
    let mut scale_errors = 0;
    for (i, r) in raw.chunks_exact(24).enumerate() {
        let a = tables.scale(
            ModulationSource {
                selector: if word(r, 0) == 0 { 3 } else { 8 },
                value: word(r, 2) as i32,
            },
            ModulationDestination::new(word(r, 1) as u8).ok_or("Invalid destination")?,
            word(r, 3) as i8,
        );
        if a.amount != word(r, 4) as i32 || a.linked_pitch != word(r, 5) as i32 {
            if scale_errors < 3 {
                eprintln!(
                    "Mod scale {i}, destination {}, depth {}: {a:?} != [{},{}]",
                    word(r, 1),
                    word(r, 3) as i32,
                    word(r, 4) as i32,
                    word(r, 5) as i32
                );
            }
            scale_errors += 1;
        }
    }
    let raw = fs::read(output.join("modulation-depths.bin"))?;
    if raw.len() != 65536 * 16 {
        return Err("Incomplete modulation depth corpus".into());
    }
    let mut depth_errors = 0;
    for (i, r) in raw.chunks_exact(16).enumerate() {
        let p = VirtualPatch {
            source: ModulationSource {
                selector: 3,
                value: 0,
            },
            destination: ModulationDestination::new(0).unwrap(),
            intensity: word(r, 0) as u8,
            manual_offset: word(r, 1) as i8,
            dynamic_offset: word(r, 2) as i16,
        };
        if p.depth() as i32 != word(r, 3) as i32 {
            if depth_errors < 3 {
                eprintln!("Mod depth {i} differs");
            }
            depth_errors += 1;
        }
    }
    let raw = fs::read(output.join("modulation-sources.bin"))?;
    if raw.len() != 32768 * 92 {
        return Err("Incomplete modulation source corpus".into());
    }
    let mut source_errors = 0;
    for (i, r) in raw.chunks_exact(92).enumerate() {
        let a = sources(r).normalized(&gain);
        let e = core::array::from_fn::<_, 10, _>(|n| word(r, 13 + n) as i32);
        for n in 0..10 {
            if a[n] != e[n] {
                if source_errors < 3 {
                    eprintln!("Mod source {i}/{n}: {} != {}", a[n], e[n]);
                }
                source_errors += 1;
            }
        }
    }
    let raw = fs::read(output.join("modulation-matrices.bin"))?;
    if raw.len() != 32768 * 336 {
        return Err("Incomplete six-patch matrix corpus".into());
    }
    let mut matrix_errors = 0;
    for (i, r) in raw.chunks_exact(336).enumerate() {
        let signals = sources(r).normalized(&gain);
        let patches = core::array::from_fn(|p| {
            let n = 13 + 5 * p;
            let selector = word(r, n) as u8;
            VirtualPatch {
                source: ModulationSource {
                    selector,
                    value: signals[selector as usize],
                },
                destination: ModulationDestination::new(word(r, n + 1) as u8).unwrap(),
                intensity: word(r, n + 2) as u8,
                manual_offset: word(r, n + 3) as i8,
                dynamic_offset: word(r, n + 4) as i16,
            }
        });
        let a = tables.route(&patches);
        let e = core::array::from_fn::<_, 40, _>(|n| word(r, 43 + n) as i32);
        if a.values != e || a.linked_pitch != word(r, 83) as i32 {
            if matrix_errors < 3 {
                eprintln!(
                    "Six-patch matrix {i} differs: {:?}, pitch {} != {}",
                    a.values,
                    a.linked_pitch,
                    word(r, 83) as i32
                );
            }
            matrix_errors += 1;
        }
    }
    let raw = fs::read(output.join("modulation-pitches.bin"))?;
    if raw.len() != 32768 * 20 {
        return Err("Incomplete controller pitch corpus".into());
    }
    let mut pitch_errors = 0;
    for (i, r) in raw.chunks_exact(20).enumerate() {
        let a = radias_synth_domain::controller_pitch::ControllerPitch {
            base_q16: word(r, 0) as i32,
            vibrato_depth: word(r, 1) as i32,
            lfo2: word(r, 2) as i16,
            virtual_patch_q16: word(r, 3) as i32,
        }
        .code();
        if a as u32 != word(r, 4) {
            if pitch_errors < 3 {
                eprintln!("Controller pitch {i} differs");
            }
            pitch_errors += 1;
        }
    }
    let raw = fs::read(output.join("modulation-apply.bin"))?;
    if raw.len() != 40960 * 20 {
        return Err("Incomplete destination storage corpus".into());
    }
    let mut apply_errors = 0;
    for (i, r) in raw.chunks_exact(20).enumerate() {
        let d = word(r, 0) as usize;
        let mut targets = radias_synth_domain::modulation::ModulationTargets::default();
        targets.values[d] = word(r, 1) as i32;
        targets.linked_pitch = word(r, 2) as i32;
        let a = targets.applied();
        let value = if d < 2 {
            a.oscillator_pitch_q16[d] as u32
        } else {
            a.controls[d - 2] as u16 as u32
        };
        if value != word(r, 3) || (d == 2 && a.linked_oscillator_pitch as u16 as u32 != word(r, 4))
        {
            if apply_errors < 3 {
                eprintln!(
                    "Destination store {i}, destination {d}: {value} != {}",
                    word(r, 3)
                );
            }
            apply_errors += 1;
        }
    }
    let passed = scale_errors == 0
        && depth_errors == 0
        && source_errors == 0
        && matrix_errors == 0
        && pitch_errors == 0
        && apply_errors == 0;
    let report = serde_json::json!({"route_scale_cases":163840,"route_scale_errors":scale_errors,"depth_cases":65536,"depth_errors":depth_errors,
        "source_signal_comparisons":327680,"source_errors":source_errors,"six_patch_matrix_cases":32768,"matrix_errors":matrix_errors,
        "controller_pitch_cases":32768,"controller_pitch_errors":pitch_errors,"destination_store_cases":40960,"destination_store_errors":apply_errors,
        "passed":passed,"original_sh3_bytes_executed":true,"destination_storage_qualified":apply_errors==0,"destination_dispatch_qualified":false,"native_audio_routing_complete":false,
        "all_midi_assignable_sources_complete":false});
    fs::write(
        output.join("modulation-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native modulation algorithms differ".into());
    }
    Ok(())
}
