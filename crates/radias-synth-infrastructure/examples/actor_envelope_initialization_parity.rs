//! Complete actor state after unchanged original EG1/EG2/EG3 note-on calls.
use radias_synth_domain::{
    actor_control_state::{ACTOR_CONTROLLER_BYTES, ActorControlState},
    actor_envelope_initialization::ActorEnvelope,
};
use radias_synth_infrastructure::firmware;
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u32 {
    let value = u32::from_le_bytes(raw[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    value
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let curves = firmware::envelope_curves(&system)?;
    let timing = firmware::envelope_timing_tables(&system)?;
    let raw = fs::read(out.join("actor-envelope-initialization-original.bin"))?;
    let mut cursor = 0;
    if take(&raw, &mut cursor) != 0x45474e31 {
        return Err("Unsupported EG observation format".into());
    }
    let (mut cases, mut state_errors, mut clock_errors) = (0, 0, 0);
    let mut coverage = [0u32; 3];
    let mut attacks = [[false; 128]; 3];
    let mut wrapped_phases = [0; 3];
    let mut first_error = None;
    while cursor < raw.len() {
        let index = take(&raw, &mut cursor) as usize;
        let variant = take(&raw, &mut cursor);
        let body: [u8; 104] = raw[cursor..cursor + 104].try_into()?;
        cursor += 104;
        let mut state = ActorControlState {
            bytes: raw[cursor..cursor + ACTOR_CONTROLLER_BYTES].try_into()?,
        };
        cursor += ACTOR_CONTROLLER_BYTES;
        let clocks = take(&raw, &mut cursor);
        let expected = &raw[cursor..cursor + ACTOR_CONTROLLER_BYTES];
        cursor += ACTOR_CONTROLLER_BYTES;
        let envelope = [
            ActorEnvelope::Filter,
            ActorEnvelope::Amplifier,
            ActorEnvelope::Modulation,
        ][index];
        let result = state.initialize_envelope(envelope, &body, &curves, &timing);
        if state.bytes != expected {
            state_errors += 1;
            if first_error.is_none() {
                let offset = state
                    .bytes
                    .iter()
                    .zip(expected)
                    .position(|(a, b)| a != b)
                    .unwrap();
                first_error = Some(format!(
                    "EG{} case {variant}: byte {offset:x}: native {} original {}",
                    index + 1,
                    state.bytes[offset],
                    expected[offset]
                ));
            }
        }
        if u32::from(result.controller_clocks) != clocks {
            clock_errors += 1;
            if first_error.is_none() {
                first_error = Some(format!(
                    "EG{} case {variant}: clocks {} != {clocks}",
                    index + 1,
                    result.controller_clocks
                ));
            }
        }
        coverage[index] += 1;
        attacks[index][result.attack as usize] = true;
        wrapped_phases[index] += u32::from(result.phase & 0x8000_0000 != 0);
        cases += 1;
    }
    let attack_codes = attacks.map(|row| row.into_iter().filter(|v| *v).count());
    let passed = state_errors == 0
        && clock_errors == 0
        && coverage == [4096; 3]
        && attack_codes == [128; 3]
        && wrapped_phases.iter().all(|v| *v != 0);
    let report = serde_json::json!({
        "passed":passed,"whole_original_note_on_calls":cases,"envelope_coverage":coverage,
        "attack_codes_covered":attack_codes,"wrapped_phase_cases":wrapped_phases,
        "state_errors":state_errors,"clock_errors":clock_errors,"first_error":first_error,
        "controller_bytes_compared":cases * ACTOR_CONTROLLER_BYTES,
        "source_output_used_only_for_assertions":true,
        "published_levels_and_other_envelopes_preserved":true,
        "complete_constructor_and_production_audio_qualified":false
    });
    fs::write(
        out.join("actor-envelope-initialization-parity.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!("{report}");
    if !passed {
        return Err("Native EG note-on differs or coverage incomplete".into());
    }
    Ok(())
}
