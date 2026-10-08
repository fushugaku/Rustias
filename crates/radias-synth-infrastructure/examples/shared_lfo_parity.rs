use radias_synth_application::{
    lfo::LfoParameters,
    shared_lfo::{EffectLfoController, EffectLfoParameters, SharedTimbreLfo},
};
use radias_synth_domain::{lfo::LfoState, lfo_tempo::LfoTempoState};
use radias_synth_infrastructure::firmware::{lfo_tables, lfo_tempo_tables};
use std::{fs, path::PathBuf};
fn word(r: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(r[n * 4..n * 4 + 4].try_into().unwrap())
}
fn state(r: &[u8], b: usize) -> LfoState {
    LfoState {
        phase: word(r, b),
        previous_random: word(r, b + 1) as i16,
        random: word(r, b + 2) as i16,
        half_cycle: word(r, b + 3) as u8,
    }
}
fn tempo(r: &[u8], b: usize) -> LfoTempoState {
    LfoTempoState {
        phase: word(r, b),
        previous_increment: word(r, b + 4),
        reference_phase: word(r, b + 5),
        clock_count: word(r, b + 6) as u16,
        observed_clock_count: word(r, b + 7) as u16,
        correction_active: word(r, b + 8) as u8,
        correction_hold: word(r, b + 9) as u8,
        division: word(r, b + 10) as u8,
    }
}
fn parameters(r: &[u8], b: usize) -> EffectLfoParameters {
    EffectLfoParameters {
        mode: word(r, b) as u8,
        frequency: word(r, b + 1) as u8,
        phase_sync: word(r, b + 2) as u8,
        beat: word(r, b + 3) as u8,
        alternate_phase: word(r, b + 4) as u8,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/shared-lfo.bin"))?;
    if raw.len() != 16384 * 612 {
        return Err("Incomplete shared LFO corpus".into());
    }
    let source = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let lfo = lfo_tables(&source)?;
    let tables = lfo_tempo_tables(&source)?;
    let mut errors = [0usize; 2];
    for (n, r) in raw.chunks_exact(612).enumerate() {
        let kind = word(r, 0) as usize;
        let mut seed = word(r, 2) as u16;
        let mut shared = SharedTimbreLfo::default();
        let mut global = EffectLfoController {
            state: state(r, 32 + 48),
            tempo: tempo(r, 32 + 48),
        };
        let mut divisions = [0u8; 2];
        for (i, division) in divisions.iter_mut().enumerate() {
            let b = 3 + 7 * i;
            shared.synthesis.parameters[i] = LfoParameters {
                waveform: word(r, b) as u8,
                shape: word(r, b + 1) as u8,
                frequency: word(r, b + 2) as u8,
                phase_sync: word(r, b + 3) as u8,
                frequency_offset: word(r, b + 4) as i8,
                frequency_modulation: word(r, b + 5) as i16,
            };
            *division = word(r, b + 6) as u8;
            shared.synthesis.states[i] = state(r, 32 + 12 * i);
            shared.tempo[i] = tempo(r, 32 + 12 * i);
            shared.effects[i] = EffectLfoController {
                state: state(r, 32 + 12 * (i + 2)),
                tempo: tempo(r, 32 + 12 * (i + 2)),
            };
        }
        match kind {
            0 => shared.tick(
                word(r, 1) != 0,
                divisions,
                [parameters(r, 17), parameters(r, 22)],
                &lfo,
                &tables,
                &mut seed,
            ),
            1 => global.tick(parameters(r, 27), &lfo, &tables, &mut seed),
            _ => return Err("Unknown shared source routine".into()),
        }
        let pairs = [
            (shared.synthesis.states[0], shared.tempo[0]),
            (shared.synthesis.states[1], shared.tempo[1]),
            (shared.effects[0].state, shared.effects[0].tempo),
            (shared.effects[1].state, shared.effects[1].tempo),
            (global.state, global.tempo),
        ];
        let different = pairs.iter().enumerate().any(|(i, (s, t))| {
            *s != state(r, 92 + 12 * i)
                || *t != tempo(r, 92 + 12 * i)
                || word(r, 32 + 12 * i + 11) != word(r, 92 + 12 * i + 11)
        }) || seed as u32 != word(r, 152);
        if different {
            if errors[kind] < 3 {
                eprintln!("Shared {kind}/{n}: {pairs:?}, seed {seed} differs");
            }
            errors[kind] += 1;
        }
    }
    let passed = errors == [0, 0];
    let report = serde_json::json!({"passed":passed,"shared_timbre_calls":8192,"global_effect_calls":8192,"shared_errors":errors[0],"global_errors":errors[1],"all_four_shared_lfo_states_compared":true,"effect_half_rate_and_alternate_phase_compared":true,"extended_tempo_divisions":64,"global_prng_compared":true,"original_sys_calls_complete":true,"effect_audio_execution_qualified":false,"complete_engine":false});
    fs::write(
        root.join("runs/native-clone/shared-lfo-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native shared LFO differs".into());
    }
    Ok(())
}
