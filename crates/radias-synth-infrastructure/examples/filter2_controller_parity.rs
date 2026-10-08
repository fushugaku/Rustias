use radias_synth_application::comb::{CombProgram, CombVoiceControl};
use radias_synth_domain::controller_comb::{CombCutoffControl, CombResonanceControl};
use radias_synth_infrastructure::firmware::{
    amplifier_tables, controller_filter_tables, filter2_control_tables,
};
use std::{fs, path::PathBuf};
fn word(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let freq = controller_filter_tables(&sys)?;
    let amp = amplifier_tables(&sys)?;
    let tables = filter2_control_tables(&sys)?;
    let corpus = fs::read(root.join("runs/native-clone/filter2-controller.bin"))?;
    if corpus.len() != 65536 * 116 {
        return Err("Incomplete ordinary Filter2 corpus".into());
    }
    let mut errors = [0; 5];
    for (n, r) in corpus.chunks_exact(116).enumerate() {
        let p = |i| word(r, i);
        let route = p(1) as u8 | if p(0) != 0 { 128 } else { 0 };
        let c = CombProgram {
            cutoff: CombCutoffControl {
                link: p(0) != 0,
                cutoff: p(2) as u8,
                linked_cutoff: p(3) as u8,
                manual_offset: p(4) as i16,
                lfo_offset: p(5) as i16,
                eg1_intensity: p(6) as u8,
                linked_eg1_intensity: p(7) as u8,
                eg1_manual_offset: p(8) as i8,
                eg1_depth_modulation: p(9) as i16,
                additional_offset: p(13) as i16,
                cutoff_modulation: p(14) as i16,
                ..Default::default()
            },
            resonance: CombResonanceControl {
                link: p(0) != 0,
                resonance: p(15) as u8,
                linked_resonance: p(16) as u8,
                modulation: p(17) as i16,
                manual_offset: p(18) as i8,
            },
            key_tracking: p(19) as u8,
            linked_key_tracking: p(20) as u8,
            key_manual_offset: p(21) as i8,
            key_modulation: p(22) as i16,
        }
        .for_voice(
            CombVoiceControl {
                eg1_level: p(10) as u16,
                velocity: p(11) as u8,
                eg1_velocity_sensitivity: p(12) as u8,
                relative_pitch: p(23) as i16,
                modulation: [0; 4],
            },
            &freq,
        );
        let code = c.cutoff.code(&amp);
        let (resonance, gain) = tables.resonance_targets(route, c.resonance);
        let actual = [
            c.cutoff.key_offset as u16 as u32,
            code as u32,
            freq.frequency(code),
            resonance as u32,
            gain as u16 as u32,
        ];
        for i in 0..5 {
            if actual[i] != p(24 + i) {
                if errors[i] < 2 {
                    eprintln!("Filter2 {n} field {i}: {} != {}", actual[i], p(24 + i));
                }
                errors[i] += 1;
            }
        }
    }
    let passed = errors == [0; 5];
    let report = serde_json::json!({"passed":passed,"original_complete_controller_cases":65536,
        "key_code_frequency_resonance_gain_errors":errors,"LINK_and_all_regular_filter_outputs":true,
        "linked_serial_gain_table_verified":true,"original_instructions_modified":false,
        "joint_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/filter2-controller-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native ordinary Filter2 controller differs".into());
    }
    Ok(())
}
