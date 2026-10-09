//! Full actor/LFO comparison after complete original note publication calls.
use radias_synth_domain::{
    actor_amplifier_preparation::ActorAmplifierPorts,
    actor_control_state::ActorControlState,
    actor_envelope_initialization::ActorEnvelope,
    actor_filter_preparation::FilterPreparation,
    actor_lfo_initialization::{ActorLfo, ActorLfoState},
    actor_pitch_preparation::ActorPitchPorts,
    virtual_patch_live::LiveCompilerTables,
};
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn actor(raw: &[u8], cursor: &mut usize) -> ActorControlState {
    let bytes = raw[*cursor..*cursor + 496].try_into().unwrap();
    *cursor += 496;
    ActorControlState { bytes }
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
    let tables = firmware::lfo_tables(&sys)?;
    let fine = firmware::fine_tune_table(&sys)?;
    let pan = firmware::pan_tables(&sys)?;
    let timing = firmware::envelope_timing_tables(&sys)?;
    let resonance = firmware::live_filter_resonance_tables(&sys)?;
    let amplifier = firmware::amplifier_tables(&sys)?;
    let frequency = firmware::controller_filter_tables(&sys)?;
    let comb = firmware::comb_control_tables(&sys)?;
    let portamento = firmware::portamento_rates(&sys)?;
    let compilers = LiveCompilerTables {
        fine: &fine,
        pan: &pan,
        timing: &timing,
        resonance: &resonance,
        amplifier: &amplifier,
        frequency: &frequency,
        comb: &comb,
        portamento: &portamento,
    };
    let raw = fs::read(out.join("note-publication-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x4e505532 {
        return Err("Unsupported note publication observation".into());
    }
    let (mut cases, mut state_errors, mut clock_errors) = (0, 0, 0);
    let mut coverage = [0u32; 13];
    let mut shapes = [[[false; 128]; 4]; 2];
    let mut syncs = [[false; 256]; 2];
    let mut drum_modes = [0u32; 2];
    let mut wheel_modes = [0u32; 2];
    let mut clamp_endpoints = [0u32; 2];
    let mut first_state_error = None;
    let mut first_clock_error = None;
    let mut invalid_shape_rejections = 0;
    let mut amplifier_context_modes = [[0u32; 2]; 2];
    let mut gain_banks = [false; 128];
    let mut filter2_modes = [[0u32; 2]; 2];
    let mut invalid_gain_rejections = 0;
    while cursor < raw.len() {
        let service = take(&raw, &mut cursor) as usize;
        let variant = take(&raw, &mut cursor);
        let timbre = take(&raw, &mut cursor) as u8;
        let midi_mode = take(&raw, &mut cursor) as u8;
        let bend_q16 = take(&raw, &mut cursor) as i32;
        let wheel = take(&raw, &mut cursor) as u8;
        let receive_flags = take(&raw, &mut cursor) as u8;
        let configuration_mode = take(&raw, &mut cursor) as u8;
        let owner_receive_flags = take(&raw, &mut cursor) as u8;
        let context_gain = take(&raw, &mut cursor) as u16;
        let midi_volume = take(&raw, &mut cursor) as u8;
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let mut state = actor(&raw, &mut cursor);
        let lfos = pair(&raw, &mut cursor);
        let clocks = take(&raw, &mut cursor);
        let expected = actor(&raw, &mut cursor);
        let expected_lfos = pair(&raw, &mut cursor);
        let work = match service {
            0 => state.prepare_note_identity(),
            1..=3 => state.publish_envelope_level(
                [
                    ActorEnvelope::Filter,
                    ActorEnvelope::Amplifier,
                    ActorEnvelope::Modulation,
                ][service - 1],
            ),
            4 | 5 => {
                let index = service - 4;
                let lfo = [ActorLfo::First, ActorLfo::Second][index];
                shapes[index][(body[76 + 5 * index] & 3) as usize][body[77 + 5 * index] as usize] =
                    true;
                syncs[index][body[79 + 5 * index] as usize] = true;
                let result = state
                    .publish_lfo_level(lfo, lfos[index], &body, &tables)
                    .map_err(|e| format!("{e:?}"))?;
                if variant == 0 {
                    let before = state;
                    let mut bad = body;
                    bad[77 + 5 * index] = 128;
                    if state
                        .publish_lfo_level(lfo, lfos[index], &bad, &tables)
                        .is_ok()
                        || state != before
                    {
                        return Err("Malformed LFO shape did not reject atomically".into());
                    }
                    invalid_shape_rejections += 1;
                }
                result.controller_clocks
            }
            6 => {
                let result = state.prepare_primary_pitch(
                    &body,
                    ActorPitchPorts {
                        timbre,
                        midi_mode,
                        bend_q16,
                        wheel,
                        common_receive_flags: receive_flags,
                    },
                    &fine,
                );
                let drum = i32::from(midi_mode >> 5) - 1 == i32::from(timbre & 3);
                drum_modes[usize::from(drum)] += 1;
                wheel_modes[usize::from(receive_flags & 0x10 != 0)] += 1;
                if result.code == 0 {
                    clamp_endpoints[0] += 1;
                }
                if result.code == 32767 {
                    clamp_endpoints[1] += 1;
                }
                result.controller_clocks
            }
            7..=10 => {
                if service == 10 {
                    filter2_modes[usize::from(state.bytes[0x1e2] & 128 != 0)]
                        [usize::from(state.bytes[0x1e2] & 0x30 == 0x30)] += 1;
                }
                state.prepare_filter_control(
                    [
                        FilterPreparation::FirstKey,
                        FilterPreparation::FirstFrequency,
                        FilterPreparation::SecondKey,
                        FilterPreparation::SecondFrequency,
                    ][service - 7],
                    &body,
                    &compilers,
                )
            }
            11 => state.prepare_amplifier_key(&body, &amplifier),
            12 => {
                let ports = ActorAmplifierPorts {
                    configuration_mode,
                    owner_receive_flags,
                    context_gain,
                    midi_volume,
                };
                let enabled =
                    owner_receive_flags & if configuration_mode == 1 { 32 } else { 64 } != 0;
                amplifier_context_modes[usize::from(configuration_mode == 1)]
                    [usize::from(enabled)] += 1;
                gain_banks[state.bytes[0x1ea] as usize] = true;
                let work = state
                    .prepare_amplifier_target(&body, ports, &amplifier)
                    .map_err(|e| format!("{e:?}"))?;
                if variant == 0 {
                    let mut bad = state;
                    bad.bytes[0x1ea] = 128;
                    let before = bad;
                    if bad
                        .prepare_amplifier_target(&body, ports, &amplifier)
                        .is_ok()
                        || bad != before
                    {
                        return Err("Invalid gain bank did not reject atomically".into());
                    }
                    invalid_gain_rejections += 1;
                }
                work
            }
            _ => return Err("Invalid publication service".into()),
        };
        if state != expected || lfos != expected_lfos {
            state_errors += 1;
            let mismatch = state
                .bytes
                .iter()
                .zip(expected.bytes)
                .position(|(a, b)| *a != b);
            first_state_error.get_or_insert(serde_json::json!({"service":service,"variant":variant,"actor_offset":mismatch,"native":mismatch.map(|i|state.bytes[i]),"original":mismatch.map(|i|expected.bytes[i])}));
        }
        if u32::from(work) != clocks {
            clock_errors += 1;
            first_clock_error.get_or_insert(serde_json::json!({"service":service,"variant":variant,"native":work,"original":clocks,"shape1":body[77],"shape2":body[82],"sync1":body[79],"sync2":body[84]}));
        }
        coverage[service] += 1;
        cases += 1;
    }
    let passed = state_errors == 0
        && clock_errors == 0
        && coverage == [4096; 13]
        && shapes == [[[true; 128]; 4]; 2]
        && syncs == [[true; 256]; 2]
        && drum_modes.iter().all(|v| *v != 0)
        && wheel_modes == [2048; 2]
        && clamp_endpoints.iter().all(|v| *v != 0)
        && invalid_shape_rejections == 2
        && invalid_gain_rejections == 1
        && gain_banks == [true; 128]
        && amplifier_context_modes.iter().flatten().all(|v| *v != 0)
        && filter2_modes.iter().flatten().all(|v| *v != 0);
    let report = serde_json::json!({
        "passed":passed,"whole_original_note_publication_calls":cases,"service_coverage":coverage,
        "all128_shapes_per_LFO_waveform":shapes == [[[true; 128]; 4]; 2],"all256_sync_bytes_per_LFO":syncs == [[true; 256]; 2],
        "drum_mode_coverage":drum_modes,"wheel_receive_coverage":wheel_modes,"pitch_clamp_endpoints":clamp_endpoints,
        "invalid_shape_rejections":invalid_shape_rejections,"invalid_gain_rejections":invalid_gain_rejections,
        "all128_gain_banks":gain_banks == [true; 128],"amplifier_context_modes":amplifier_context_modes,
        "filter2_LINK_and_Comb_modes":filter2_modes,"state_errors":state_errors,"clock_errors":clock_errors,
        "first_state_error":first_state_error,"first_clock_error":first_clock_error,
        "voice_bytes_compared":cases * 560,"source_output_used_only_for_assertions":true,
        "complete_SYS014f40_and_production_audio_qualified":false
    });
    fs::write(
        out.join("note-publication-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Native note publication differs or coverage incomplete".into());
    }
    Ok(())
}
