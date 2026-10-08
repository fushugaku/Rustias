use radias_synth_domain::{
    pitch::{PhaseIncrement, PitchCode},
    primary_oscillator::{PrimaryParameters, sine_pitch_coefficient},
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};
fn word(raw: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(raw[index * 4..index * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let tables = MasterTables::from_host_stream(&image)?;
    let pitch = tables.pitch()?;
    let raw = fs::read(root.join("runs/native-clone/primary-pitch.bin"))?;
    if raw.len() != 32768 * 12 {
        return Err("Original Sine pitch corpus truncated".into());
    }
    let mut errors = 0;
    for (i, row) in raw.chunks_exact(12).enumerate() {
        if word(row, 0) != i as u32 {
            return Err("Original Sine pitch corpus order differs".into());
        }
        let increment = pitch.increment(PitchCode::new(i as u16).unwrap());
        if increment.0 != word(row, 1)
            || sine_pitch_coefficient(increment) as u16 as u32 != word(row, 2)
        {
            if errors < 4 {
                eprintln!(
                    "Sine pitch {i}: {increment:?}, {} vs {}, {}",
                    sine_pitch_coefficient(increment),
                    word(row, 1),
                    word(row, 2)
                );
            }
            errors += 1;
        }
    }
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    if system.len() != 0xe0000 {
        return Err("Original SYS image truncated".into());
    }
    let mut initialization_errors = 0;
    for (selection, address) in [(0, 0x42088), (1, 0x42098), (2, 0x420a8), (3, 0x420c4)] {
        let compiled = PrimaryParameters::waveform(selection, PhaseIncrement(0), 0).unwrap();
        let mut offset = address + 0x1000;
        loop {
            let entry = u32::from_be_bytes(system[offset..offset + 4].try_into().unwrap());
            offset += 4;
            if entry == u32::MAX {
                break;
            }
            let field = entry >> 16;
            let actual = match compiled {
                PrimaryParameters::Ramp(p) | PrimaryParameters::Pulse(p) => match field {
                    12 => p.blend as u16,
                    13 => p.target_gain as u16,
                    14 => p.memory_gain as u16,
                    _ => return Err("Unknown original ramp initialization field".into()),
                },
                PrimaryParameters::Triangle(p) => match field {
                    10 => p.center as u16,
                    11 => p.gain as u16,
                    12 => p.upper as u16,
                    13 => p.upper_reflection as u16,
                    14 => p.lower as u16,
                    15 => p.lower_reflection as u16,
                    _ => return Err("Unknown original Triangle initialization field".into()),
                },
                PrimaryParameters::Sine(p) => match field {
                    12 => p.center as u16,
                    // Initial modulated phase is zero, independently of parameters.
                    13 => 0,
                    14 => p.polynomial[0] as u16,
                    15 => (p.polynomial[0] >> 16) as u16,
                    16 => (p.polynomial[1] >> 16) as u16,
                    17 => p.polynomial[1] as u16,
                    18 => (p.polynomial[2] >> 16) as u16,
                    19 => p.polynomial[2] as u16,
                    _ => return Err("Unknown original Sine initialization field".into()),
                },
                _ => unreachable!(),
            };
            if u32::from(actual) != entry & 65535 {
                initialization_errors += 1;
            }
        }
    }
    let report = serde_json::json!({"passed": errors == 0 && initialization_errors == 0, "original_pitch_conversions": 32768,
        "pitch_and_coefficient_errors": errors, "waveform_initialization_errors": initialization_errors,
        "original_waveform_initialization_descriptors":4,"original_instructions_modified": false,
        "whole_bank_audio_qualified": false, "complete_native_engine": false});
    fs::write(
        root.join("runs/native-clone/primary-pitch-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 || initialization_errors != 0 {
        return Err("Primary coefficient compiler differs".into());
    }
    Ok(())
}
