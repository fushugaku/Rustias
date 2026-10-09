//! Original whole initial-rate/LFO note procedures; results are assertions only.
use radias_synth_domain::actor_lfo_initialization::{ActorLfoState, initialize_lfo_rates};
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    v
}
fn pair(raw: &[u8], cursor: &mut usize) -> [ActorLfoState; 2] {
    core::array::from_fn(|_| {
        let bytes = raw[*cursor..*cursor + 32].try_into().unwrap();
        *cursor += 32;
        ActorLfoState { bytes }
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = firmware::lfo_tempo_tables(&sys)?;
    let raw = fs::read(out.join("actor-lfo-initialization-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x4c464931 {
        return Err("Unsupported LFO initialization observation".into());
    }
    let (mut cases, mut state_errors, mut clock_errors, mut random_errors) = (0, 0, 0, 0);
    let mut coverage = [0u32; 3];
    let mut modes = [[0u32; 4]; 2];
    let mut divisions = [[false; 32]; 2];
    let mut timbres = [[false; 4]; 3];
    let mut first_error = None;
    while cursor < raw.len() {
        let service = take(&raw, &mut cursor) as usize;
        let variant = take(&raw, &mut cursor);
        let timbre = take(&raw, &mut cursor) as usize;
        let mut seed = take(&raw, &mut cursor) as u16;
        let clock = take(&raw, &mut cursor);
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let mut states = pair(&raw, &mut cursor);
        let shared = pair(&raw, &mut cursor);
        let clocks = take(&raw, &mut cursor);
        let expected_seed = take(&raw, &mut cursor) as u16;
        let expected = pair(&raw, &mut cursor);
        let work = if service == 0 {
            for (index, row) in divisions.iter_mut().enumerate() {
                row[(body[80 + 5 * index] & 31) as usize] = true;
            }
            initialize_lfo_rates(&mut states, &body, clock, &tables)
        } else {
            let index = service - 1;
            let sync = body[79 + 5 * index];
            modes[index][((sync & 0x60) >> 5) as usize] += 1;
            states[index].initialize_note(sync, body[80 + 5 * index], shared[index], &mut seed)
        };
        if states != expected {
            state_errors += 1;
            if first_error.is_none() {
                let offset = states
                    .iter()
                    .flat_map(|s| s.bytes)
                    .zip(expected.iter().flat_map(|s| s.bytes))
                    .position(|(a, b)| a != b)
                    .unwrap();
                first_error = Some(format!(
                    "service {service} case {variant}: state byte {offset:x}: {} != {}",
                    states[offset / 32].bytes[offset % 32],
                    expected[offset / 32].bytes[offset % 32]
                ));
            }
        }
        if seed != expected_seed {
            random_errors += 1;
            if first_error.is_none() {
                first_error = Some(format!(
                    "service {service} case {variant}: random seed {seed:x} != {expected_seed:x}"
                ));
            }
        }
        if u32::from(work) != clocks {
            clock_errors += 1;
            if first_error.is_none() {
                first_error = Some(format!(
                    "service {service} case {variant}: clocks {work} != {clocks}"
                ));
            }
        }
        coverage[service] += 1;
        timbres[service][timbre] = true;
        cases += 1;
    }
    let passed = state_errors == 0
        && clock_errors == 0
        && random_errors == 0
        && coverage == [4096; 3]
        && modes == [[1024; 4]; 2]
        && divisions == [[true; 32]; 2]
        && timbres == [[true; 4]; 3];
    let report = serde_json::json!({
        "passed":passed,"whole_original_calls":cases,"service_coverage":coverage,
        "sync_mode_coverage":modes,"all32_divisions_per_LFO":divisions == [[true; 32]; 2],
        "all4_timbres_per_service":timbres == [[true; 4]; 3],
        "state_errors":state_errors,"random_errors":random_errors,"clock_errors":clock_errors,
        "first_error":first_error,"state_bytes_compared":cases * 64,
        "source_output_used_only_for_assertions":true,
        "complete_constructor_and_production_audio_qualified":false
    });
    fs::write(
        out.join("actor-lfo-initialization-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Native LFO note service differs or coverage incomplete".into());
    }
    Ok(())
}
