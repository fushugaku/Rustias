//! Production pool on an explicit 24-sample service/CC65 scalar contract.
use radias_synth_application::{
    VoiceRenderer,
    amplifier::{AmplifierController, AmplifierProgram, ControllerTables},
    modulation::{ModulationProgram, VoiceModulationTables},
    polyphony::{ActiveVoice, PolyphonicRenderer},
    portamento::PortamentoTables,
};
use radias_synth_domain::{pan::VoiceBus, portamento::PortamentoProgram};
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
    let raw = fs::read(root.join("runs/native-clone/portamento-pool.bin"))?;
    if raw.len() != 32768 * 44 {
        return Err("Original pool contract incomplete".into());
    }
    let mut errors = 0;
    let mut cases = 0;
    for curve in 0..16 {
        let mut pool = PolyphonicRenderer::default();
        pool.configure_note_pitch(firmware::note_pitch_tables(&sys)?, Default::default(), 0);
        pool.configure_portamento(PortamentoTables {
            rates: firmware::portamento_rates(&sys)?,
            curves: firmware::portamento_curves(&sys)?,
        });
        for timbre in 0..4 {
            pool.edit_pitch_program(timbre, Default::default());
            pool.edit_portamento_program(
                timbre,
                PortamentoProgram {
                    time: [32, 80, 96, 127][timbre as usize],
                    curve,
                    switch_required: true,
                },
            );
            pool.set_portamento_switch(timbre, true);
            let note = [48, 55, 64, 72][timbre as usize];
            let voice = ActiveVoice {
                uses_program_common: false,
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
                pan: None,
                mixer: None,
                secondary: None,
                primary: None,
                shaper: None,
                comb_program: None,
                timbre,
                note,
                velocity: 100,
                held: true,
                program: 0,
                bus: VoiceBus::new(timbre).unwrap(),
            };
            let assigned = pool
                .trigger_modulated(voice, 4283, ModulationProgram::default())
                .ok_or("Allocation failed")?;
            if assigned.slot != timbre {
                return Err("Pool fixture slot assignment differs".into());
            }
        }
        for frame in 0..512 {
            if frame == 192 || frame == 288 {
                for t in 0..4 {
                    pool.set_portamento_switch(t, frame == 288);
                }
            }
            let _ =
                pool.next_buses_with_modulation(&wave, Some(&tables), Some(&modulation), |_| &[]);
            for t in 0..4 {
                let index = (curve as usize * 4 + t) * 512 + frame;
                let row = &raw[index * 44..(index + 1) * 44];
                let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
                let port = pool
                    .voice_portamento(t)
                    .ok_or("Native portamento not connected")?;
                let note = pool.voice_note_pitch(t).unwrap().note.wrapped;
                let actual = [
                    port.state.phase,
                    port.state.rate,
                    port.state.start_q16 as u32,
                    port.state.current_q16 as u32,
                    port.state.assigned_note_q16(note) as u32,
                    pool.active_voice(t).unwrap().renderer.primary_pitch_code() as u32,
                ];
                let expected = core::array::from_fn::<_, 6, _>(|i| w(i + 5));
                if actual != expected {
                    if errors < 3 {
                        eprintln!(
                            "Pool curve{curve}/t{t}/frame{frame}: {actual:?} vs {expected:?}"
                        );
                    }
                    errors += 1;
                }
                cases += 1;
            }
        }
    }
    let report = serde_json::json!({"passed":errors==0,"original_controller_rows":cases,"errors":errors,
        "production_fixed_pool_pitch_and_portamento_used":true,"original_24_sample_service_contract_and_CC65_events_used":true,
        "original_portamento_states_or_pitch_targets_replayed":false,"independent_controller_hpi_timing_qualified":false,
        "monophonic_allocation_policy_qualified":false,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/portamento-pool-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if errors != 0 {
        return Err("Live pool differs from original controller contract".into());
    }
    println!("32768 original four-timbre controller states/pitch codes match the live pool");
    Ok(())
}
