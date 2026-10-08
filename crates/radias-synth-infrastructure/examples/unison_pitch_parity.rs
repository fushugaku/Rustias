use radias_synth_domain::{
    pitch::PhaseIncrement,
    unison_pitch::{UnisonDetuneTable, unison_bandwidth, unison_phases},
};
use radias_synth_infrastructure::firmware::MasterTables;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&image)?;
    let mut table = UnisonDetuneTable {
        coefficients: [0; 5],
    };
    for (i, v) in table.coefficients.iter_mut().enumerate() {
        *v = master.word(0x4788 + i)? as i16;
    }
    if table != UnisonDetuneTable::ORIGINAL {
        return Err("Original Unison detune constants differ".into());
    }
    for index in 0..128 {
        if unison_bandwidth(PhaseIncrement((index as u32) << 24)).0 as u16
            != master.word(0x47cb + index)?
        {
            return Err("Original Unison bandwidth lookup differs".into());
        }
    }
    let raw = fs::read(root.join("runs/native-clone/unison-pitch.bin"))?;
    if raw.len() != 32768 * 28 {
        return Err("Unison pitch corpus incomplete".into());
    }
    let mut errors = 0;
    for (i, row) in raw.chunks_exact(28).enumerate() {
        let word = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        if word(1) != i as u32 {
            return Err("Unison corpus detune order differs".into());
        }
        let p = table
            .compile(PhaseIncrement(word(0)), word(1) as u16)
            .ok_or("Invalid detune")?;
        let actual = [
            p.increments[1].0,
            p.increments[2].0,
            p.averaging_center.0,
            p.increments[3].0,
            p.increments[4].0,
        ];
        let expected = core::array::from_fn::<_, 5, _>(|j| word(j + 2));
        if actual != expected {
            if errors < 3 {
                eprintln!(
                    "Unison detune {i}, increment {}: {actual:?} vs {expected:?}",
                    word(0)
                );
            }
            errors += 1;
        }
    }
    let bandwidth = fs::read(root.join("runs/native-clone/unison-bandwidth.bin"))?;
    let phases = fs::read(root.join("runs/native-clone/unison-phase-init.bin"))?;
    if bandwidth.len() != 32768 * 12 || phases.len() != 65536 * 32 {
        return Err("Unison bandwidth/phase corpus incomplete".into());
    }
    let mut bandwidth_errors = 0;
    let mut phase_errors = 0;
    for row in bandwidth.chunks_exact(12) {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let actual = unison_bandwidth(PhaseIncrement(w(0)));
        if [actual.0 as u16 as u32, actual.1 as u16 as u32] != [w(1), w(2)] {
            bandwidth_errors += 1;
        }
    }
    for row in phases.chunks_exact(32) {
        let w = |n: usize| u32::from_le_bytes(row[n * 4..n * 4 + 4].try_into().unwrap());
        let actual = unison_phases(w(0) as u16, w(1) != 0)
            .ok_or("Invalid phase control")?
            .map(|p| p.0);
        let expected = core::array::from_fn::<_, 5, _>(|i| w(i + 2));
        if actual != expected || actual[0] != w(7) {
            if phase_errors < 3 {
                eprintln!(
                    "Unison phase {},{}: {actual:?} vs {expected:?}, mirror {}",
                    w(0),
                    w(1),
                    w(7)
                );
            }
            phase_errors += 1;
        }
    }
    let report = serde_json::json!({"passed":errors==0&&bandwidth_errors==0&&phase_errors==0,"original_compiler_calls":32768,"coefficient_errors":errors,"original_bandwidth_compiler_calls":32768,"bandwidth_errors":bandwidth_errors,"original_phase_initializer_calls":65536,"phase_errors":phase_errors,"original_instructions_modified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/unison-pitch-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 || bandwidth_errors != 0 || phase_errors != 0 {
        return Err("Unison pitch/phase compiler differs".into());
    }
    Ok(())
}
