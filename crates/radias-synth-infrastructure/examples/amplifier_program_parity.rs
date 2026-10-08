//! The application program/controller on original scalar lifecycle/target data.
use radias_synth_application::{
    amplifier::{AmplifierController, AmplifierProgram, ControllerTables},
    program::TimbreControls,
    voice_envelopes::ModEnvelopeProgram,
};
use radias_synth_domain::amp_envelope::AmpEnvelopeParameters;
use radias_synth_infrastructure::{firmware, rdl};
use std::{fs, path::PathBuf};
fn word(raw: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = ControllerTables {
        curves: firmware::envelope_curves(&system)?,
        timing: firmware::envelope_timing_tables(&system)?,
        amplifier: firmware::amplifier_tables(&system)?,
    };
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("envelope-lifecycle.bin"))?;
    if raw.len() != 40960 * 100 {
        return Err("Original lifecycle corpus incomplete".into());
    }
    let mut controller = None::<AmplifierController>;
    let mut lifecycle_errors = 0;
    for (index, row) in raw.chunks_exact(100).enumerate() {
        let w = |i| word(row, i);
        let program = AmplifierProgram {
            envelope: ModEnvelopeProgram {
                adsr: core::array::from_fn(|i| w(4 + i) as u8),
                curve: w(8) as u8,
                velocity_time_sensitivity: w(10) as u8,
                key_tracking: w(12) as u8,
                velocity_level_sensitivity: 127,
            },
            ..Default::default()
        };
        match w(2) {
            0 => {
                controller = Some(AmplifierController::from_program(
                    program,
                    w(11) as u8,
                    w(9) as u8,
                    &tables,
                ))
            }
            1 => {
                controller.as_mut().unwrap().service(&tables, w(3) != 0);
            }
            2 => controller.as_mut().unwrap().release(&tables),
            3 => controller.as_mut().unwrap().edit_program(program, &tables),
            _ => return Err("Unknown source lifecycle operation".into()),
        }
        let ctrl = controller.as_ref().unwrap();
        let s = ctrl.envelope.segment;
        let e = &ctrl.envelope;
        let actual = [
            s.phase,
            s.increment,
            s.start as u32,
            s.difference as u16 as u32,
            s.level as u32,
            e.published_level as u32,
            e.target as u32,
            e.increment_flags as u32,
            e.divider as u32,
            e.release_hold as u32,
            e.stage as u8 as u32,
            e.dirty as u32,
        ];
        if actual != core::array::from_fn::<_, 12, _>(|i| w(13 + i)) {
            if lifecycle_errors < 3 {
                eprintln!("Application lifecycle{index}:{actual:?}");
            }
            lifecycle_errors += 1;
        }
    }
    let raw = fs::read(out.join("envelope-amplifier.bin"))?;
    if raw.len() != 32768 * 48 {
        return Err("Original target corpus incomplete".into());
    }
    let mut target_errors = 0;
    for (index, row) in raw.chunks_exact(48).enumerate() {
        let w = |i| word(row, i);
        let program = AmplifierProgram {
            level: w(0) as u8,
            level_offset: w(1) as i8,
            key_tracking: 64,
            source_gain: w(2) as u16,
            envelope: ModEnvelopeProgram {
                velocity_level_sensitivity: w(5) as u8,
                ..Default::default()
            },
            midi_volume: if w(8) == 0 { None } else { Some(w(9) as u8) },
            program_volume: w(10) as u8,
        };
        let mut ctrl = AmplifierController::from_program(program, 60, w(4) as u8, &tables);
        // This is the scalar function's accepted input level, not audio replay.
        ctrl.envelope.segment.level = w(3) as u16;
        let actual = ctrl.modulations([w(6) as i16, w(7) as i16], &tables) as u16 as u32;
        if actual != w(11) {
            if target_errors < 3 {
                eprintln!("Application target{index}:{actual} vs{}", w(11));
            }
            target_errors += 1;
        }
        let mut ctrl = AmplifierController::from_program(
            AmplifierProgram {
                program_volume: 0,
                ..program
            },
            60,
            w(4) as u8,
            &tables,
        );
        ctrl.envelope.segment.level = w(3) as u16;
        ctrl.modulations([w(6) as i16, w(7) as i16], &tables);
        let before = ctrl.envelope;
        ctrl.set_group_gain_bank(w(10) as u8);
        if ctrl.next_target(&tables) as u16 as u32 != w(11) || ctrl.envelope != before {
            target_errors += 1;
        }
    }
    let bank = rdl::programs(&fs::read(root.join("firmware/Radias-backup.rdl"))?)?;
    let mut input_blocks = 0;
    for program in &bank {
        for index in 0..4 {
            let timbre = program.timbre(index).unwrap();
            let p = timbre.synthesis();
            let controls =
                TimbreControls::from_timbre(timbre).map_err(|_| "Invalid stored route")?;
            let compiled = controls.amplifier(0x7f00, None, 0);
            let note = 36 + index as u8 * 12;
            let velocity = 32 + index as u8 * 30;
            let expected = AmpEnvelopeParameters {
                attack: p[0x3c],
                decay: p[0x3d],
                sustain: p[0x3e],
                release: p[0x3f],
                curve: p[0x40],
                velocity_sensitivity: p[0x42],
                key_tracking: p[0x43],
                velocity,
                note,
            };
            if compiled.parameters(note, velocity) != expected
                || compiled.control(velocity).velocity_sensitivity != p[0x41]
                || compiled.level != p[0x2d]
            {
                return Err("Stored EG2 application binding differs".into());
            }
            input_blocks += 1;
        }
    }
    let passed = lifecycle_errors == 0 && target_errors == 0 && input_blocks == 1024;
    let report = serde_json::json!({"passed":passed,"original_lifecycle_steps":40960,"lifecycle_errors":lifecycle_errors,
        "original_target_cases":32768,"group_gain_bank_updates_without_envelope_clock_advance":32768,"target_errors":target_errors,"source_timbre_input_blocks":input_blocks,
        "source_programs":bank.len(),"native_application_factory_and_edit_program_used":true,
        "native_bank_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("amplifier-program-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native amplifier application mismatch".into());
    }
    Ok(())
}
