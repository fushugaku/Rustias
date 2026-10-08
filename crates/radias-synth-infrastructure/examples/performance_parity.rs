use radias_synth_domain::{
    amplifier_control::AmplifierControl,
    performance::{ExpressionState, GlobalPerformance, expression_channel_mask},
};
use radias_synth_infrastructure::{firmware, rdl};
use std::{fs, path::PathBuf};
fn w(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let bank = fs::read(root.join("firmware/Radias-backup.rdl"))?;
    if rdl::global(&bank)? != fs::read(out.join("performance-original-global.bin"))? {
        return Err("Native Global extraction differs".into());
    }
    let global = rdl::global_performance(&bank)?;
    let original_global = rdl::global(&bank)?;
    if global.channel != original_global[6] & 15
        || global.amplitude_receive_mode != original_global[15]
    {
        return Err("Native Global fields differ".into());
    }
    let state = fs::read(out.join("expression-state.bin"))?;
    let amp = fs::read(out.join("expression-amplifier-context.bin"))?;
    if state.len() != 65536 * 116 || amp.len() != 32768 * 60 {
        return Err("Incomplete performance corpus".into());
    }
    let mut state_errors = 0;
    let mut amplitude_errors = 0;
    for (n, r) in state.chunks_exact(116).enumerate() {
        let channels = core::array::from_fn(|i| w(r, i + 3) as u8);
        let mut expression = ExpressionState::default();
        expression.set(w(r, 1) as u8, w(r, 0) as u8);
        let mask = expression_channel_mask(w(r, 1) as u8, channels, w(r, 2) as u8);
        let flags = 0x8000 | if mask != 0 { 2 } else { 0 };
        let values = core::array::from_fn::<_, 16, _>(|i| expression.value(i as u8) as u32);
        let gains = channels.map(|c| (expression.value(c) as u32) << 8);
        if mask as u32 != w(r, 7)
            || flags != w(r, 8)
            || values != core::array::from_fn(|i| w(r, 9 + i))
            || gains != core::array::from_fn(|i| w(r, 25 + i))
        {
            if state_errors < 2 {
                eprintln!("Expression state {n} differs");
            }
            state_errors += 1;
        }
    }
    let tables = firmware::amplifier_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    for (n, r) in amp.chunks_exact(60).enumerate() {
        let channel = w(r, 13) as u8;
        let mut expression = ExpressionState::default();
        expression.set(channel, w(r, 2) as u8);
        let source_gain = expression.gain(
            channel,
            w(r, 1) as u8,
            GlobalPerformance {
                channel: 0,
                amplitude_receive_mode: w(r, 0) as u8,
            },
        );
        let actual = tables.target(AmplifierControl {
            source_gain,
            level: w(r, 3) as u8,
            envelope_level: w(r, 4) as u16,
            velocity: w(r, 5) as u8,
            velocity_sensitivity: w(r, 6) as u8,
            modulation: [w(r, 7) as i16, w(r, 8) as i16],
            midi_volume: if w(r, 9) != 0 {
                Some(w(r, 10) as u8)
            } else {
                None
            },
            program_volume: w(r, 11) as u8,
            level_offset: w(r, 12) as i8,
        }) as u16 as u32;
        if actual != w(r, 14) {
            if amplitude_errors < 2 {
                eprintln!("Expression amplifier {n}: {actual} != {}", w(r, 14));
            }
            amplitude_errors += 1;
        }
    }
    let mut bindings = 0;
    for program in rdl::programs(&bank)? {
        let c = radias_synth_application::program::StoredProgram::from_program(
            &program,
            global.channel,
        )
        .map_err(|_| "Invalid stored controls")?;
        for (i, t) in c.timbres.iter().enumerate() {
            if t.receive_flags != program.timbre(i).unwrap().bytes()[5] {
                return Err("Native receive flags differ".into());
            }
            bindings += 1;
        }
    }
    let passed = state_errors == 0 && amplitude_errors == 0;
    let report = serde_json::json!({"passed":passed,"original_Expression_state_and_cache_cases":65536,
        "original_amplifier_context_cases":32768,"state_errors":state_errors,"amplitude_errors":amplitude_errors,
        "receive_flag_bindings":bindings,"Global_record_matches_original":true,"original_instructions_modified":false,
        "program_common_application_lifecycle_qualified":false,"full_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("performance-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native Expression/performance differs".into());
    }
    Ok(())
}
