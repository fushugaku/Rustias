//! The desktop's Noise compiler and physical pool against unchanged source WAVs.
//! Other voice controls, initial actors and the source note clock are fixtures.
use radias_synth_application::{
    VoiceRenderer,
    noise::NoiseTables,
    polyphony::{ActiveVoice, PolyphonicRenderer},
    primary::PrimaryProgram,
};
use radias_synth_domain::{Sample, controller_primary::PrimaryControl, noise::NoiseFrameSeeds};
use radias_synth_infrastructure::{
    firmware::{self, MasterTables},
    prepared::PreparedVoice,
    wav,
};
use std::{fs, path::PathBuf};

fn word(raw: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(raw[index * 4..index * 4 + 4].try_into().unwrap())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let master_image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master_image)?;
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let table = master.waveform()?;
    let boot: serde_json::Value = serde_json::from_slice(&fs::read(
        out.join("live-mixer-noise-lifecycle-reference-noise-boot-inputs.json"),
    )?)?;
    let accepted_seeds = NoiseFrameSeeds::from_inputs(
        boot["input602"].as_u64().ok_or("Boot input absent")? as i16,
        boot["input603"].as_u64().ok_or("Boot input absent")? as i16,
    );
    let mut tracks = Vec::new();
    for mode in ["noise", "formant"] {
        let name = format!("live-osc1-{mode}-reference");
        let raw = fs::read(out.join(format!("{name}-voice-va-inputs.bin")))?;
        let plan = PreparedVoice::from_reference_va_parameters(&raw)?;
        let program = fs::read(out.join(format!("osc1-{mode}.program.bin")))?;
        let source = fs::read(out.join(format!("{name}-original-complete-mix.wav")))?;
        let data = source
            .windows(4)
            .position(|w| w == b"data")
            .ok_or("WAV data absent")?
            + 8;
        let length = (source.len() - data) / 32;
        if length != plan.reference_start_frame as usize + plan.reference_voice_frames {
            return Err("Unqualified source note/tail timeline".into());
        }
        let mut pool = PolyphonicRenderer::default();
        pool.initialize_physical_frames([accepted_seeds; 2]);
        pool.configure_noise(NoiseTables {
            pitch: master.pitch()?,
            noise: master.noise_pitch()?,
            counters: firmware::formant_counter_seeds(&system)?,
        });
        let mut output = Vec::with_capacity(length);
        let mut errors = 0;
        for frame in 0..length {
            if frame == plan.reference_start_frame as usize {
                let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
                renderer.control_slew(plan.control_slew, 3);
                pool.install(
                    0,
                    0,
                    ActiveVoice {
                        uses_program_common: false,
                        drum_pitch: None,
                        drum_instrument: None,
                        drum_filter2: None,
                        renderer,
                        amplifier: None,
                        modulation: None,
                        auxiliary: None,
                        pan: None,
                        mixer: None,
                        secondary: None,
                        primary: Some(PrimaryProgram {
                            selection: program[86],
                            control: PrimaryControl {
                                control1: program[87],
                                control2: program[88],
                                ..Default::default()
                            },
                        }),
                        shaper: None,
                        comb_program: None,
                        timbre: 0,
                        note: 60,
                        velocity: 100,
                        held: true,
                        program: 0,
                        bus: plan.bus,
                    },
                );
            }
            let buses = pool.next_buses(&table, None, |_| &plan.events);
            let samples: [Sample; 8] = core::array::from_fn(|channel| {
                if channel & 1 == 0 {
                    buses[0][channel / 2].left
                } else {
                    buses[0][channel / 2].right
                }
            });
            let expected = &source[data + frame * 32..data + (frame + 1) * 32];
            for (channel, sample) in samples.iter().enumerate() {
                if sample.0 != word(expected, channel) as i32 {
                    if errors < 3 {
                        eprintln!(
                            "{name} frame{frame} ch{channel}:{} vs{}",
                            sample.0,
                            word(expected, channel) as i32
                        );
                    }
                    errors += 1;
                }
            }
            output.push(samples);
        }
        wav::write_buses(&out.join(format!("{name}-rust-live-pool.wav")), &output)?;
        tracks.push(
            serde_json::json!({"name":name,"frames":length,"errors":errors,"passed":errors==0}),
        );
        if errors != 0 {
            return Err("Live Noise pool differs from original audio".into());
        }
    }
    let report = serde_json::json!({"passed":true,"tracks":tracks,
        "native_live_pool_noise_compiler_and_boot_frames_used":true,
        "original_noise_coefficients_phases_counter_or_samples_replayed":false,
        "other_initial_actors_controls_and_note_times_are_fixture_inputs":true,
        "independent_controller_hpi_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("noise-live-pool-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
