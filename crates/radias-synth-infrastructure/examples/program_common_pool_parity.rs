//! Original common-level arithmetic and production24-actor live context edits.
use radias_synth_application::{
    VoiceRenderer,
    amplifier::{AmplifierController, AmplifierProgram, ControllerTables},
    polyphony::{ActiveVoice, PolyphonicRenderer},
};
use radias_synth_domain::{
    controller_pan::PanControl,
    pan::VoiceBus,
    performance::{ExpressionState, GlobalPerformance},
    program_binding::ProgramCommon,
};
use radias_synth_infrastructure::{firmware, prepared::PreparedVoice};
use std::{fs, path::PathBuf};
fn w(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = ControllerTables {
        curves: firmware::envelope_curves(&sys)?,
        timing: firmware::envelope_timing_tables(&sys)?,
        amplifier: firmware::amplifier_tables(&sys)?,
    };
    let raw = fs::read(out.join("expression-amplifier-context.bin"))?;
    if raw.len() != 32768 * 60 {
        return Err("Original amplifier contexts missing".into());
    }
    let mut scalar_errors = 0;
    for (n, r) in raw.chunks_exact(60).enumerate() {
        let mut expression = ExpressionState::default();
        expression.set(w(r, 13) as u8, w(r, 2) as u8);
        let common = ProgramCommon {
            level: w(r, 10) as u8,
            pan: 64,
        };
        let program = AmplifierProgram {
            level: w(r, 3) as u8,
            level_offset: w(r, 12) as i8,
            source_gain: expression.gain(
                w(r, 13) as u8,
                w(r, 1) as u8,
                GlobalPerformance {
                    channel: 0,
                    amplitude_receive_mode: w(r, 0) as u8,
                },
            ),
            program_volume: w(r, 11) as u8,
            ..Default::default()
        };
        let mut control = program.control(w(r, 5) as u8);
        control.velocity_sensitivity = w(r, 6) as u8;
        let mut parameters = program.parameters(60, w(r, 5) as u8);
        parameters.velocity = w(r, 5) as u8;
        let mut amp = AmplifierController::new(parameters, control, &tables);
        amp.envelope.segment.level = w(r, 4) as u16;
        amp.program_common(common.level_for(w(r, 9) as u8), &tables);
        let target = amp.modulations([w(r, 7) as i16, w(r, 8) as i16], &tables) as u16 as u32;
        if target != w(r, 14) {
            if scalar_errors < 3 {
                eprintln!("Common amplitude{n}: {target} != {}", w(r, 14));
            }
            scalar_errors += 1;
        }
    }
    let plan =
        PreparedVoice::from_program_json(&fs::read(root.join("assets/native-va/sine.json"))?)?;
    let mut pool = PolyphonicRenderer::default();
    let common = ProgramCommon {
        level: 100,
        pan: 64,
    };
    pool.edit_program_common(common, &tables);
    for slot in 0..24 {
        let timbre = slot % 4;
        let note = 48 + slot;
        let alternate = slot % 3 != 0;
        let voice = ActiveVoice {
            uses_program_common: alternate,
            drum_pitch: None,
            drum_instrument: None,
            drum_filter2: None,
            renderer: VoiceRenderer::new(plan.initial, plan.parameters),
            amplifier: Some(AmplifierController::from_program(
                AmplifierProgram::default(),
                note,
                100,
                &tables,
            )),
            modulation: None,
            auxiliary: None,
            pan: Some(PanControl::default()),
            mixer: None,
            secondary: None,
            primary: None,
            shaper: None,
            comb_program: None,
            timbre,
            note,
            velocity: 100,
            held: true,
            program: slot as usize,
            bus: VoiceBus::new(timbre).unwrap(),
        };
        pool.install(slot as usize, 0, voice);
    }
    let mut edits = 0;
    for value in 0..128 {
        let context = ProgramCommon {
            level: value,
            pan: 127 - value,
        };
        let envelopes = core::array::from_fn::<_, 24, _>(|slot| {
            pool.active_voice(slot).unwrap().amplifier.unwrap().envelope
        });
        pool.edit_program_common(context, &tables);
        for (slot, envelope) in envelopes.iter().enumerate() {
            let voice = pool.active_voice(slot).unwrap();
            let amp = voice.amplifier.unwrap();
            let expected = voice.uses_program_common.then_some(context.level);
            if amp.control().midi_volume != expected
                || voice.pan.unwrap().midi_pan != voice.uses_program_common.then_some(context.pan)
                || amp.envelope != *envelope
                || !voice.held
            {
                return Err("Common edit replaced an envelope or affected ordinary actor".into());
            }
            edits += 1;
        }
        // A per-timbre frontend edit must retain the common context on alternate actors.
        for timbre in 0..4 {
            pool.edit_amplifier_program(timbre, AmplifierProgram::default(), &tables);
            pool.edit_pan(timbre, PanControl::default());
        }
        for slot in 0..24 {
            let voice = pool.active_voice(slot).unwrap();
            if voice.amplifier.unwrap().control().midi_volume
                != voice.uses_program_common.then_some(context.level)
                || voice.pan.unwrap().midi_pan != voice.uses_program_common.then_some(context.pan)
            {
                return Err("Frontend edit dropped common context".into());
            }
        }
    }
    let passed = scalar_errors == 0;
    let report = serde_json::json!({"passed":passed,"original_amplifier_context_calls":32768,"scalar_errors":scalar_errors,
        "physical_actors":24,"live_common_actor_edits":edits,"ordinary_actors_unchanged":true,"envelopes_and_note_ownership_preserved":true,
        "frontend_amplifier_and_pan_edits_preserve_context":true,"allocation_and_drum_note_lifecycle_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("program-common-pool-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native application common amplifier differs".into());
    }
    Ok(())
}
