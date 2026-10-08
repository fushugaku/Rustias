//! Allocation/LFO lifetime regressions using the actual native instrument pool.
use radias_synth_application::{
    VoiceRenderer,
    amplifier::ControllerTables,
    modulation::{ModulationProgram, VoiceModulationTables},
    polyphony::{ActiveVoice, PolyphonicRenderer},
};
use radias_synth_domain::pan::VoiceBus;
use radias_synth_infrastructure::{
    firmware::{
        MasterTables, amplifier_tables, envelope_curves, envelope_timing_tables, lfo_tables,
        modulation_tables,
    },
    prepared::PreparedVoice,
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let tempo = std::env::args().any(|argument| argument == "--tempo");
    let plan =
        PreparedVoice::from_program_json(&fs::read(root.join("assets/native-va/saw.json"))?)?;
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let master = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let data = MasterTables::from_host_stream(&master)?;
    let wave = data.waveform()?;
    let tables = VoiceModulationTables {
        lfo: lfo_tables(&sys)?,
        matrix: modulation_tables(&sys)?,
        pitch: data.pitch()?,
        bandwidth: data.bandwidth()?,
    };
    let controller = ControllerTables {
        curves: envelope_curves(&sys)?,
        timing: envelope_timing_tables(&sys)?,
        amplifier: amplifier_tables(&sys)?,
    };
    let make_voice = |note| ActiveVoice {
        renderer: VoiceRenderer::new(plan.initial, plan.parameters),
        amplifier: None,
        modulation: None,
        auxiliary: None,
        pan: None,
        mixer: None,
        secondary: None,
        primary: None,
        shaper: None,
        comb_program: None,
        timbre: 0,
        note,
        velocity: 100,
        held: true,
        program: 0,
        bus: VoiceBus::new(0).unwrap(),
    };
    let mut program = ModulationProgram::default();
    for p in &mut program.lfo {
        p.phase_sync = if tempo { 0xc0 } else { 0x40 };
        p.waveform = 3;
        p.frequency = 100;
        p.shape = 90;
    }
    let mut pool = PolyphonicRenderer::default();
    if tempo {
        pool.enable_tempo_clock(
            radias_synth_infrastructure::firmware::lfo_tempo_tables(&sys)?,
            1200,
        );
        program.tempo_divisions = [13, 16];
    }
    pool.edit_modulation(0, program)
        .map_err(|_| "Unexpected tempo")?;
    for note in 48..72 {
        pool.trigger_modulated(make_voice(note), 4283, program)
            .ok_or("Voice allocation failed")?;
    }
    for _ in 0..12000 {
        pool.next_sample_with_modulation(&wave, Some(&controller), Some(&tables), |_| &[]);
    }
    let prior = pool
        .retained_modulation_state(0)
        .ok_or("Physical slot state absent")?;
    let assignment = pool
        .trigger_modulated(make_voice(72), 4283, program)
        .ok_or("Replacement failed")?;
    if assignment.slot != 0 {
        return Err("Physical oldest slot was not replaced".into());
    }
    let actual = pool
        .retained_modulation_state(0)
        .ok_or("Replacement state absent")?;
    if let Some(clock) = pool.tempo_clock() {
        for i in 0..2 {
            let state = clock.bank.voices[assignment.slot as usize][i];
            if state.phase != 0
                || state.reference_phase != 0
                || state.clock_count != 0
                || state.observed_clock_count != 0
                || state.division != program.tempo_divisions[i]
                || state.previous_increment
                    != clock
                        .tables
                        .compile_increment(
                            program.tempo_divisions[i] as i32,
                            0,
                            clock.receiver.tempo.clock_rate(),
                        )
                        .1
            {
                return Err("Tempo note initialization retained stale phase/counters/rate".into());
            }
        }
    }
    for i in 0..2 {
        if actual[i].previous_random != prior[i].random
            || actual[i].half_cycle != prior[i].half_cycle
            || actual[i].phase != 0
        {
            return Err("S&H replacement discarded physical controller history".into());
        }
    }
    pool.remove(0);
    pool.trigger_modulated(make_voice(73), 4283, program)
        .ok_or("Free slot reactivation failed")?;
    let reactivated = pool
        .retained_modulation_state(0)
        .ok_or("Freed physical state absent")?;
    for i in 0..2 {
        if reactivated[i].previous_random != actual[i].random
            || reactivated[i].half_cycle != actual[i].half_cycle
        {
            return Err("Freeing a voice erased physical LFO history".into());
        }
    }
    let mut rejected = PolyphonicRenderer::default();
    let before = rejected.modulation_random;
    let before_allocator = rejected.allocator.clone();
    program.lfo[0].phase_sync |= 128;
    if rejected
        .trigger_modulated(make_voice(60), 4283, program)
        .is_some()
        || rejected.modulation_random != before
        || rejected.allocator != before_allocator
    {
        return Err("Unsupported tempo changed allocation/controller state".into());
    }
    let report = serde_json::json!({"passed":true,"physical_slot_reuse_qualified":true,"retained_sample_hold_history":true,"unsupported_note_preserves_seed":true,"allocation_resource_rejection_exercised":false,"freed_slot_history_preserved":true,"unsupported_tempo_preserves_allocation":true,"maximum_voices":24,"full_engine_complete":false,"tempo_enabled":tempo,"tempo_note_reinitialization_verified":tempo});
    fs::write(
        root.join(if tempo {
            "runs/native-clone/lfo-tempo-slot-initialization.json"
        } else {
            "runs/native-clone/lfo-slot-initialization.json"
        }),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
