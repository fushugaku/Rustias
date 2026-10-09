//! Complete note filter state/work compared with the immutable SYS execution.
use radias_synth_domain::{
    actor_control_state::ActorControlState, virtual_patch_live::LiveCompilerTables,
};
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let v = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    v
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let fine = firmware::fine_tune_table(&sys)?;
    let pan = firmware::pan_tables(&sys)?;
    let timing = firmware::envelope_timing_tables(&sys)?;
    let resonance = firmware::live_filter_resonance_tables(&sys)?;
    let amplifier = firmware::amplifier_tables(&sys)?;
    let frequency = firmware::controller_filter_tables(&sys)?;
    let comb = firmware::comb_control_tables(&sys)?;
    let portamento = firmware::portamento_rates(&sys)?;
    let tables = LiveCompilerTables {
        fine: &fine,
        pan: &pan,
        timing: &timing,
        resonance: &resonance,
        amplifier: &amplifier,
        frequency: &frequency,
        comb: &comb,
        portamento: &portamento,
    };
    let raw = fs::read(out.join("note-filters-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x46494c31 {
        return Err("Unsupported note filter observation".into());
    }
    let (mut calls, mut state_errors, mut clock_errors) = (0u32, 0u32, 0u32);
    let mut coverage = [0u32; 5];
    let mut first_error = [
        serde_json::Value::Null,
        serde_json::Value::Null,
        serde_json::Value::Null,
        serde_json::Value::Null,
        serde_json::Value::Null,
    ];
    let mut filter1_velocity = [[0u32; 128]; 128];
    let mut filter2_velocity = [[[0u32; 128]; 128]; 2];
    let mut flags = [0u32; 256];
    while cursor < raw.len() {
        let service = take(&raw, &mut cursor);
        let variant = take(&raw, &mut cursor);
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let before: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let original_work = take(&raw, &mut cursor);
        let after: [u8; 496] = raw[cursor..cursor + 496].try_into()?;
        cursor += 496;
        let mut controller = ActorControlState { bytes: before };
        let work = match service {
            0 => controller.prepare_filter_velocity(&body, false),
            1 => controller.prepare_filter_velocity(&body, true),
            2 => controller.prepare_filter_resonance(&body, false, &tables),
            3 => controller.prepare_filter_resonance(&body, true, &tables),
            4 => controller.initialize_note_filters(&body, &tables),
            _ => return Err("Invalid filter service".into()),
        };
        let state_error = controller.bytes != after;
        let clock_error = u32::from(work) != original_work;
        state_errors += u32::from(state_error);
        clock_errors += u32::from(clock_error);
        if (state_error || clock_error) && first_error[service as usize].is_null() {
            let offset = controller
                .bytes
                .iter()
                .zip(after)
                .position(|(a, b)| *a != b);
            first_error[service as usize] = serde_json::json!({"variant":variant,"offset":offset,"native_value":offset.map(|i|controller.bytes[i]),"original_value":offset.map(|i|after[i]),"native_work":work,"original_work":original_work});
        }
        let link = usize::from(before[0x1e2] & 128 != 0);
        let velocity = usize::from(before[0x37] & 127);
        if service == 0 {
            filter1_velocity[usize::from(body[39] & 127)][velocity] += 1;
        }
        if service == 1 {
            filter2_velocity[link][usize::from(body[if link == 1 { 39 } else { 44 }] & 127)]
                [velocity] += 1;
        }
        if service == 4 {
            flags[usize::from(before[0x1e2])] += 1;
        }
        coverage[service as usize] += 1;
        calls += 1;
    }
    let velocity_coverage = filter1_velocity.iter().all(|r| r.iter().all(|n| *n == 1))
        && filter2_velocity
            .iter()
            .all(|v| v.iter().all(|r| r.iter().all(|n| *n == 1)));
    let flag_coverage = flags.iter().all(|n| *n == 32);
    let passed = state_errors + clock_errors == 0
        && coverage == [16384, 32768, 8192, 8192, 8192]
        && velocity_coverage
        && flag_coverage;
    let report = serde_json::json!({"passed":passed,"whole_original_filter_preparation_calls":calls,"procedure_coverage":coverage,"whole_SYS01c05c_calls":coverage[4],"all128_velocity_and128_depth_pairs_per_filter_and_link_qualified":velocity_coverage,"all256_cached_filter_flags_covered":flag_coverage,"controller_bytes_compared":u64::from(calls)*496,"state_errors":state_errors,"clock_errors":clock_errors,"first_error_by_service":first_error,"source_outputs_used_only_for_assertions":true,"Comb_resonance_repeated_in_source_order":true,"complete_native_engine":false});
    fs::write(
        out.join("note-filters-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Complete native filter note preparation differs".into());
    }
    Ok(())
}
