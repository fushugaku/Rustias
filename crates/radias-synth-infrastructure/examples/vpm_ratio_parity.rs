use radias_synth_domain::controller_primary::PrimaryControl;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/vpm-ratio.bin"))?;
    if raw.len() != 32768 * 16 {
        return Err("Incomplete VPM ratio corpus".into());
    }
    let mut errors = 0;
    for (i, r) in raw.chunks_exact(16).enumerate() {
        let word = |n: usize| u32::from_le_bytes(r[4 * n..4 * n + 4].try_into().unwrap());
        let actual = PrimaryControl {
            control2: word(0) as u8,
            control2_manual_offset: word(1) as i8,
            control2_modulation: word(2) as i16,
            ..Default::default()
        }
        .vpm_ratio();
        if actual as u16 as u32 != word(3) {
            if errors < 3 {
                eprintln!("VPM ratio{i}: {actual} vs {}", word(3));
            }
            errors += 1;
        }
    }
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    for raw in 0..128 {
        let offset = 0x4286a + 0x1000 + 2 * raw;
        let original = i16::from_be_bytes(system[offset..offset + 2].try_into().unwrap());
        if original
            != (PrimaryControl {
                control2: raw as u8,
                ..Default::default()
            })
            .vpm_ratio()
        {
            errors += 1;
        }
    }
    use radias_synth_domain::{
        pitch::PhaseIncrement,
        primary_oscillator::{PrimaryParameters, PrimaryVpmCarrier},
    };
    let mut initialization_errors = 0;
    for (selection, address) in [(0, 0x421e0), (1, 0x421f8), (2, 0x42210), (3, 0x4223c)] {
        let compiled = PrimaryParameters::vpm_waveform(selection, PhaseIncrement(0), 0).unwrap();
        let mut offset = address + 0x1000;
        loop {
            let entry = u32::from_be_bytes(system[offset..offset + 4].try_into().unwrap());
            offset += 4;
            if entry == u32::MAX {
                break;
            }
            let field = entry >> 16;
            let value = match compiled {
                PrimaryParameters::Vpm(p) => match field {
                    11 => p.offset as u16,
                    12 => p.blend as u16,
                    13 => p.limit as u16,
                    14 => (p.center >> 16) as u16,
                    15 => p.center as u16,
                    _ => return Err("Unknown VPM ramp initialization field".into()),
                },
                PrimaryParameters::VpmCarrier(p) => match field {
                    10 => p.modulator.limit as u16,
                    11 => 0,
                    12 => (p.modulator.center >> 16) as u16,
                    13 => p.modulator.center as u16,
                    _ => match p.carrier {
                        PrimaryVpmCarrier::Triangle(t) => match field {
                            14 => t.center as u16,
                            15 => t.gain as u16,
                            16 => t.upper as u16,
                            17 => t.upper_reflection as u16,
                            18 => t.lower as u16,
                            19 => t.lower_reflection as u16,
                            _ => return Err("Unknown VPM Triangle initialization field".into()),
                        },
                        PrimaryVpmCarrier::Sine { center, polynomial } => match field {
                            14 => center as u16,
                            15 => 0,
                            16 => polynomial[0] as u16,
                            17 => (polynomial[0] >> 16) as u16,
                            18 => (polynomial[1] >> 16) as u16,
                            19 => polynomial[1] as u16,
                            20 => (polynomial[2] >> 16) as u16,
                            21 => polynomial[2] as u16,
                            _ => return Err("Unknown VPM Sine initialization field".into()),
                        },
                    },
                },
                _ => unreachable!(),
            };
            if u32::from(value) != entry & 65535 {
                initialization_errors += 1;
            }
        }
    }
    errors += initialization_errors;
    let report = serde_json::json!({"passed":errors==0,"errors":errors,"initialization_errors":initialization_errors,"original_initialization_descriptors":4,"original_instruction_slices":32768,"original_ratio_table_words":128,"source_range":"SYS020f8e..020fbe;before HPI delivery","original_instructions_modified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/vpm-ratio-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Native VPM ratio mismatch".into());
    }
    Ok(())
}
