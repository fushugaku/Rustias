//! Production Mono queue/voice use cases against an explicit source service clock.
use radias_synth_application::{
    VoiceRenderer,
    amplifier::{AmplifierController, AmplifierProgram, ControllerTables},
    modulation::{ModulationProgram, VoiceModulationTables},
    polyphony::{ActiveVoice, PolyphonicRenderer},
    portamento::PortamentoTables,
    voice_envelopes::VoiceEnvelopes,
};
use radias_synth_domain::{
    mono_notes::{MonoAction, VoiceMode},
    pan::VoiceBus,
    portamento::PortamentoProgram,
};
use radias_synth_infrastructure::{
    firmware::{self, MasterTables},
    prepared::PreparedVoice,
};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let host = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&host)?;
    let wave = master.waveform()?;
    let tables = ControllerTables {
        curves: firmware::envelope_curves(&sys)?,
        timing: firmware::envelope_timing_tables(&sys)?,
        amplifier: firmware::amplifier_tables(&sys)?,
    };
    let modulation = VoiceModulationTables {
        lfo: firmware::lfo_tables(&sys)?,
        matrix: firmware::modulation_tables(&sys)?,
        pitch: master.pitch()?,
        bandwidth: master.bandwidth()?,
    };
    let plan =
        PreparedVoice::from_program_json(&fs::read(root.join("assets/native-va/saw.json"))?)?;
    let raw = fs::read(root.join("runs/native-clone/mono-notes-controller.bin"))?;
    if raw.len() != 24576 * 84 {
        return Err("Original Mono controller contract incomplete".into());
    }
    let mut errors = 0;
    for mode_index in 0..8 {
        for timbre in 0..4u8 {
            let mode = VoiceMode::from_raw((mode_index % 4) | ((mode_index / 4) * 64));
            let mut pool = PolyphonicRenderer::default();
            pool.set_voice_mode(timbre, mode);
            pool.configure_note_pitch(firmware::note_pitch_tables(&sys)?, Default::default(), 0);
            pool.edit_pitch_program(timbre, Default::default());
            pool.configure_portamento(PortamentoTables {
                rates: firmware::portamento_rates(&sys)?,
                curves: firmware::portamento_curves(&sys)?,
            });
            pool.edit_portamento_program(
                timbre,
                PortamentoProgram {
                    time: 80,
                    curve: [0, 3, 8, 15][timbre as usize],
                    switch_required: false,
                },
            );
            pool.set_timbre_modulation_active(timbre, true);
            let mut slot = None;
            for frame in 0..768usize {
                let index = (mode_index as usize * 4 + timbre as usize) * 768 + frame;
                let row = &raw[index * 84..(index + 1) * 84];
                let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
                let mut action = 0;
                if w(3) != 0 {
                    let decision = pool
                        .mono_event(timbre, w(3))
                        .ok_or("Mono use case absent")?;
                    action = match decision.action {
                        MonoAction::Ignore => 0,
                        MonoAction::Allocate => 1,
                        MonoAction::Legato => 2,
                        MonoAction::Retrigger => 3,
                        MonoAction::Release => 4,
                    };
                    let note = decision.event as u8 & 127;
                    let velocity = (decision.event >> 8) as u8 & 127;
                    match decision.action {
                        MonoAction::Ignore => {}
                        MonoAction::Legato => {
                            pool.legato(timbre, note, velocity);
                        }
                        MonoAction::Release => pool.release_note(timbre, note, Some(&tables)),
                        MonoAction::Allocate | MonoAction::Retrigger => {
                            let voice = ActiveVoice {
                                uses_program_common: false,
                                drum_pitch: None,
                                drum_instrument: None,
                                drum_filter2: None,
                                renderer: VoiceRenderer::new(plan.initial, plan.parameters),
                                amplifier: Some(AmplifierController::from_program(
                                    AmplifierProgram::default(),
                                    note,
                                    velocity,
                                    &tables,
                                )),
                                modulation: None,
                                auxiliary: Some(VoiceEnvelopes::new(
                                    [Default::default(); 2],
                                    None,
                                    note,
                                    velocity,
                                    &tables,
                                )),
                                pan: None,
                                mixer: None,
                                secondary: None,
                                primary: None,
                                shaper: None,
                                comb_program: None,
                                timbre,
                                note,
                                velocity,
                                held: true,
                                program: 0,
                                bus: VoiceBus::new(timbre).unwrap(),
                            };
                            let assignment = if decision.action == MonoAction::Retrigger {
                                pool.retrigger_modulated(voice, 4283, ModulationProgram::default())
                            } else {
                                pool.trigger_modulated(voice, 4283, ModulationProgram::default())
                            }
                            .ok_or("Mono allocation failed")?;
                            slot = Some(assignment.slot as usize);
                        }
                    }
                }
                pool.next_buses_with_modulation(&wave, Some(&tables), Some(&modulation), |_| &[]);
                let active = pool
                    .active_voice(slot.ok_or("Mono actor absent")?)
                    .ok_or("Mono actor retired early")?;
                let port = pool
                    .voice_portamento(slot.unwrap())
                    .ok_or("Mono glide absent")?;
                let mut actual = [0; 17];
                actual[..10].copy_from_slice(&[
                    action,
                    active.note as u32,
                    active.velocity as u32,
                    active.held as u32,
                    port.state.phase,
                    port.state.rate,
                    port.state.start_q16 as u32,
                    port.state.current_q16 as u32,
                    port.state.assigned_note_q16(active.note) as u32,
                    active.renderer.primary_pitch_code() as u32,
                ]);
                let queue = pool.mono_notes(timbre).unwrap();
                for (i, entry) in queue.entries.iter().enumerate() {
                    actual[i + 10] = *entry as u32;
                }
                actual[16] = queue.velocity as u32;
                let expected = core::array::from_fn::<_, 17, _>(|i| w(i + 4));
                if actual != expected {
                    if errors < 5 {
                        eprintln!(
                            "Mono mode{mode_index}/t{timbre}/frame{frame}: {actual:?} vs {expected:?}"
                        );
                    }
                    errors += 1;
                }
            }
        }
    }
    let report = serde_json::json!({"passed":errors==0,"original_controller_rows":24576,"errors":errors,
        "production_mono_queue_voice_selection_and_portamento_used":true,
        "original_24_sample_service_contract_used":true,"original_mono_decisions_or_glide_states_replayed":false,
        "pressure_budget_and_multi_voice_unison_qualified":false,"independent_hpi_midi_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/mono-pool-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Production Mono controller contract differs".into());
    }
    println!("24576 source Mono controller/queue rows match the production pool");
    Ok(())
}
