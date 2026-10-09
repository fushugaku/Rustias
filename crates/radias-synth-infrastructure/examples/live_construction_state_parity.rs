//! Live booted SYS constructor inputs; no captured output or event time renders audio.
use radias_synth_application::{
    actor_construction::ActorConstructionRun, synthesis_transport::SynthesisParameterTransport,
};
use radias_synth_domain::{
    actor_amplifier_preparation::ActorAmplifierPorts,
    actor_construction::{
        ActorConstructionRequest, ActorConstructionState, ActorConstructionTables,
    },
    actor_control_state::ActorControlState,
    actor_copy::ActorTemplateBinding,
    actor_descriptors::DescriptorOperation,
    actor_lfo_initialization::ActorLfoState,
    actor_lifecycle::ActorLifecycle,
    actor_note_initialization::{
        ActorNoteInitializationPorts, ActorNoteInitializationTables, ActorPortamentoInitialization,
    },
    actor_note_preparation::{ActorNotePreparationPorts, ActorNotePreparationTables},
    actor_pitch_preparation::ActorPitchPorts,
    actor_virtual_patch::ActorVirtualPatchPorts,
    complete_actor_startup::CompleteStartupTables,
    construction_first_pass::{ConstructionFirstPass, FirstPassStore},
    dsp_control::ParameterPacket,
    manual_parameters::ManualCompilerTables,
    modulation::ModulationTargets,
    motion_initialization::MotionControlState,
    note_modulators::NoteModulatorTables,
    note_refresh::NoteRefreshTables,
    raw_note_scale::RawNoteScaleContext,
    virtual_patch_live::LiveCompilerTables,
};
use radias_synth_domain::{dsp_control::DspEndpoint, parameter_template::ParameterTemplate};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
struct Ram(Vec<u8>);
impl Ram {
    fn bytes<const N: usize>(&self, address: u32) -> [u8; N] {
        let o = (address - 0xc000000) as usize;
        self.0[o..o + N].try_into().unwrap()
    }
    fn byte(&self, address: u32) -> u8 {
        self.bytes::<1>(address)[0]
    }
    fn word(&self, address: u32) -> u16 {
        u16::from_be_bytes(self.bytes(address))
    }
    fn long(&self, address: u32) -> u32 {
        u32::from_be_bytes(self.bytes(address))
    }
    fn pair(&self, address: u32) -> [ActorLfoState; 2] {
        core::array::from_fn(|i| ActorLfoState {
            bytes: self.bytes(address + 32 * i as u32),
        })
    }
    fn state(&self, context: u32) -> ActorConstructionState {
        let actors = 0xc0cea54;
        ActorConstructionState {
            controllers: core::array::from_fn(|s| ActorControlState {
                bytes: self.bytes(actors + 496 * s as u32),
            }),
            lfos: core::array::from_fn(|s| self.pair(self.long(actors + 496 * s as u32 + 0x2c))),
            motion: core::array::from_fn(|s| MotionControlState {
                bytes: self.bytes(self.long(actors + 496 * s as u32 + 0x28)),
            }),
            allocation_flags: self.bytes(0xc0d18d4),
            context: self.bytes(context),
            random_seed: self.word(0xc0d210c),
            pending_retirement: self.long(0xc0ce2c4),
            lifecycle: ActorLifecycle {
                active: self.long(0xc147e70),
            },
            modulations: ModulationTargets {
                values: core::array::from_fn(|i| self.long(0xc147d48 + 4 * i as u32) as i32),
                linked_pitch: self.long(0xc147e68) as i32,
            },
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let prefix = "construction-visible-state-source";
    let runtime: serde_json::Value = serde_json::from_slice(&fs::read(
        out.join(format!("{prefix}-construction-runtime.json")),
    )?)?;
    let number = |name: &str| runtime[name].as_u64().unwrap();
    let before = Ram(fs::read(
        out.join(format!("{prefix}-construction-before-ram.bin")),
    )?);
    let after = Ram(fs::read(
        out.join(format!("{prefix}-construction-after-ram.bin")),
    )?);
    let context = number("context") as u32;
    let actor = 0xc0cea54;
    let prior = before.state(context);
    let expected = after.state(context);
    let body: [u8; 104] = before.bytes(before.long(actor + 4));
    let common = before.long(actor + 8);
    let owner = before.long(context);
    let midi = before.long(0xc0cea3c);
    let config = before.long(0xc0cea38);
    let timbre = before.byte(context + 0x47);
    let receive = before.byte(common + 5);
    let request = ActorConstructionRequest {
        selected: number("selected") as u32,
        midi_word: number("midi_word") as u32,
        owner_identity: before.long(context + 0x1c),
        body: &body,
        group_ordinals: before.bytes(0xc0ce396),
        detune_amount: before.byte(owner + 9),
        spread_amount: before.byte(owner + 10),
        shared_lfos: before.pair(before.long(0xc03ddf8 + 4 * u32::from(timbre))),
        binding: ActorTemplateBinding {
            program_kind: before.byte(midi + 0x18),
            timbre_id: timbre,
            drum_instrument: before.byte(0xc0ce6ba) & 15,
            ordinary_address: before.word(context + 0x34),
        },
        ports: ActorNotePreparationPorts {
            motion_assignments: core::array::from_fn(|i| {
                before.byte(common + 0xae + 18 * i as u32)
            }),
            motion_program_flags: before.byte(common + 0xac),
            motion_global_flags: before.byte(0xc14e37b),
            lfo_clock_rate: before.long(0xc0cea4c),
            midi_pan: before.byte(midi + 0x1a),
            amplifier: ActorAmplifierPorts {
                configuration_mode: before.byte(config + 0xf),
                owner_receive_flags: before.byte(owner + 5),
                context_gain: before.word(context + 0x32),
                midi_volume: before.byte(midi + 0x19),
            },
            note: ActorNoteInitializationPorts {
                master_tune: before.long(0xc147e6c) as i32,
                pitch: ActorPitchPorts {
                    timbre,
                    midi_mode: before.byte(midi + 0x18),
                    bend_q16: before.long(context + 0xc) as i32,
                    wheel: before.byte(context + 0x4b),
                    common_receive_flags: receive,
                },
                sources: ActorVirtualPatchPorts {
                    bend: before.word(context + 0x30) as i16,
                    wheel: before.byte(context + 0x4b),
                    auxiliary: before.word(0xc147ff6) as i16,
                    midi_receive_flags: before.byte(owner + 5),
                    assignments: core::array::from_fn(|i| before.byte(config + 9 + i as u32)),
                    assignable_values: core::array::from_fn(|i| {
                        before.byte(context + 0x4c + i as u32) as i8
                    }),
                },
                portamento: ActorPortamentoInitialization {
                    time: before.byte(common + 12),
                    switch_required: receive & 8 != 0,
                    switch: before.byte(context + 0x51) != 0,
                    context_flags: before.byte(context + 0x42),
                    gate_flags: before.byte(context + 0x60),
                    context_pitch_q16: before.long(context + 0x18) as i32,
                },
                scale: RawNoteScaleContext {
                    selection: before.byte(midi + 0x13),
                    global_transpose: (before.byte(config + 2) & 4 != 0)
                        .then_some(before.byte(config + 1) as i8),
                    custom_cents: core::array::from_fn(|i| {
                        before.byte((i64::from(config) + 0x12 + i as i64 - 128) as u32) as i8
                    }),
                },
            },
        },
    };
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let pitch = firmware::note_pitch_tables(&sys)?;
    let scale = firmware::raw_note_scale_tables(&sys)?;
    let fine = firmware::fine_tune_table(&sys)?;
    let pan = firmware::pan_tables(&sys)?;
    let timing = firmware::envelope_timing_tables(&sys)?;
    let curves = firmware::envelope_curves(&sys)?;
    let resonance = firmware::live_filter_resonance_tables(&sys)?;
    let amplifier = firmware::amplifier_tables(&sys)?;
    let frequency = firmware::controller_filter_tables(&sys)?;
    let comb = firmware::comb_control_tables(&sys)?;
    let portamento = firmware::portamento_rates(&sys)?;
    let modulation = firmware::modulation_tables(&sys)?;
    let lfo = firmware::lfo_tables(&sys)?;
    let tempo = firmware::lfo_tempo_tables(&sys)?;
    let mixer = firmware::mixer_scales(&sys)?;
    let dispatch = firmware::primary_pitch_sender_table(&sys)?;
    let live = LiveCompilerTables {
        fine: &fine,
        pan: &pan,
        timing: &timing,
        resonance: &resonance,
        amplifier: &amplifier,
        frequency: &frequency,
        comb: &comb,
        portamento: &portamento,
    };
    let manual = ManualCompilerTables {
        live: &live,
        primary_pitch: dispatch,
    };
    let modulators = NoteModulatorTables {
        curves: &curves,
        timing: &timing,
        amplifier: &amplifier,
        modulation: &modulation,
    };
    let refresh = NoteRefreshTables {
        lfo: &lfo,
        compilers: &live,
        modulation: &modulation,
    };
    let note = ActorNoteInitializationTables {
        pitch: &pitch,
        scale: &scale,
        portamento: &portamento,
        amplifier: &amplifier,
        modulation: &modulation,
    };
    let note_preparation = ActorNotePreparationTables {
        manual: &manual,
        tempo: &tempo,
        modulators: &modulators,
        refresh: &refresh,
        note: &note,
        mixer: &mixer,
    };
    let master_data = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let slave_data = fs::read(root.join("firmware/dsp-slave-host-stream.bin"))?;
    let master = MasterTables::from_host_stream(&master_data)?;
    let slave = MasterTables::from_host_stream(&slave_data)?;
    let pitch_rom = [master.pitch_receiver_rom()?, slave.pitch_receiver_rom()?];
    let mix = master.filter_mix()?;
    let descriptors = firmware::parameter_template_tables(&sys, master.filter_mix()?)?;
    let phases = firmware::physical_phase_tables(&sys)?;
    let callbacks = firmware::phase_callback_tables(&sys)?;
    let counters = firmware::formant_counter_seeds(&sys)?;
    let amplifier_rate = firmware::amplifier_rate_table(&sys)?;
    let groups = firmware::construction_group_tables(&sys)?;
    let addresses = firmware::parameter_template_addresses(&sys)?;
    let startup = CompleteStartupTables {
        descriptors: &descriptors,
        phases: &phases,
        callbacks: &callbacks,
        counters: &counters,
        pitch: dispatch,
        pan: &pan,
        amplifier: &amplifier_rate,
        timing: &timing,
    };
    let tables = ActorConstructionTables {
        note: &note_preparation,
        startup: &startup,
        groups: &groups,
        addresses: &addresses,
    };
    let compiled = prior
        .construct(request, &tables)
        .map_err(|e| format!("{e:?}"))?;
    // An all-ready diagnostic isolates the missing receiver/job schedule. Its
    // timestamps are computed from native work, never used to render audio.
    let mut transport = SynthesisParameterTransport::default();
    transport.configure_constructor_filter_mix(
        radias_synth_domain::filter_control::FilterMixTable {
            weights: mix.weights,
        },
    );
    transport.configure_pitch_receivers(pitch_rom, dispatch);
    transport.set_actor_lifecycle(prior.lifecycle);
    for chip in 0..2 {
        let raw = fs::read(out.join(format!("{prefix}-construction-dsp{chip}.bin")))?;
        let word = |address: usize| {
            u32::from_le_bytes(raw[address * 4..address * 4 + 4].try_into().unwrap()) as u16
        };
        for local in 0..12 {
            transport.restore_construction_state(
                chip * 12 + local,
                core::array::from_fn(|i| word(0x2000 + 160 * local + i)),
                core::array::from_fn(|i| word(0x3000 + 64 * local + i)),
            );
        }
        for index in 0..20 {
            transport
                .install_parameter_template(
                    if chip == 0 {
                        DspEndpoint::Master
                    } else {
                        DspEndpoint::Slave
                    },
                    index,
                    ParameterTemplate {
                        words: core::array::from_fn(|i| word(0x780 + 160 * index + i)),
                    },
                )
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    let mut run =
        ActorConstructionRun::prepare(0, &prior, request, &tables).map_err(|e| format!("{e:?}"))?;
    let mut clock = 0u64;
    let mut state = prior;
    let mut native_acks = Vec::new();
    loop {
        run.enqueue_available(&mut transport)
            .map_err(|e| format!("{e:?}"))?;
        clock += 3000;
        transport.advance_until_with_readiness(clock, |_| 0, |time, _, _| native_acks.push(time));
        if run.finish(clock, &mut state, &mut transport) {
            break;
        }
        if clock > 1000000 {
            return Err("All-ready diagnostic failed to return".into());
        }
    }
    let native_return = transport.caller_available_clock();
    let events: Vec<serde_json::Value> =
        fs::read_to_string(out.join(format!("{prefix}-constructor-timeline.jsonl")))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
    let returned: serde_json::Value = serde_json::from_slice(&fs::read(
        out.join(format!("{prefix}-construction-return.json")),
    )?)?;
    let original: Vec<Vec<u16>> = events
        .iter()
        .filter(|e| {
            e["kind"] == "packet_ack"
                && e["cpu_clock"].as_u64().unwrap() >= number("clock")
                && e["cpu_clock"].as_u64().unwrap() < returned["clock"].as_u64().unwrap()
        })
        .map(|e| {
            e["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16)
                .collect()
        })
        .collect();
    let source_acks: Vec<u64> = events
        .iter()
        .filter(|e| {
            e["kind"] == "packet_ack"
                && e["cpu_clock"].as_u64().unwrap() >= number("clock")
                && e["cpu_clock"].as_u64().unwrap() < returned["clock"].as_u64().unwrap()
        })
        .map(|e| e["cpu_clock"].as_u64().unwrap() - number("clock"))
        .collect();
    let native: Vec<Vec<u16>> = compiled
        .steps()
        .iter()
        .flat_map(|step| {
            step.publication
                .operations()
                .iter()
                .filter_map(move |operation| {
                    if let DescriptorOperation::Send {
                        sender,
                        offset,
                        value,
                        ..
                    } = *operation
                    {
                        Some(
                            ParameterPacket::from_sender(
                                sender,
                                0x2000 + 160 * (step.slot % 12) as u32 + u32::from(offset),
                                value,
                            )
                            .unwrap()
                            .words()
                            .to_vec(),
                        )
                    } else {
                        None
                    }
                })
        })
        .collect();
    let selected = number("selected") as u32;
    let first_pass =
        ConstructionFirstPass::compile(&prior.controllers, selected, before.long(context + 0x1c));
    let stores: Vec<serde_json::Value> =
        fs::read_to_string(out.join(format!("{prefix}-construction-controller-stores.jsonl")))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
    let original_first: Vec<(u16, u32, u32, u32)> = stores
        .iter()
        .filter(|e| {
            e["clock"].as_u64().unwrap() < number("clock") + u64::from(first_pass.return_clock)
        })
        .map(|e| {
            (
                (e["clock"].as_u64().unwrap() - number("clock")) as u16,
                e["address"].as_u64().unwrap() as u32,
                e["value"].as_u64().unwrap() as u32,
                e["width"].as_u64().unwrap() as u32,
            )
        })
        .collect();
    let native_first: Vec<_> = first_pass
        .stores()
        .iter()
        .map(|e| match e.store {
            FirstPassStore::ControllerWord {
                slot,
                offset,
                value,
            } => (
                e.clock,
                actor + 496 * slot as u32 + offset as u32,
                u32::from(value),
                2u32,
            ),
            FirstPassStore::AllocationFlag { slot, value } => {
                (e.clock, 0xc0d18d4 + slot as u32, u32::from(value), 1u32)
            }
        })
        .collect();
    let controller_difference = (0..24).find_map(|s| {
        (0..496)
            .find(|i| compiled.state.controllers[s].bytes[*i] != expected.controllers[s].bytes[*i])
            .map(|i| (s, i))
    });
    let state_equal = compiled.state == expected;
    let packets_equal = native == original;
    let first_equal = native_first == original_first;
    let passed = state_equal && packets_equal && first_equal;
    let report = serde_json::json!({"passed":passed,"all24_controllers_LFO_motion_context_flags_seed_and_lifecycle_equal":state_equal,
        "all_constructor_packets_equal":packets_equal,"first_pass_store_values_order_and_clocks_equal":first_equal,
        "source_packets":original.len(),"native_packets":native.len(),"first_pass_visible_stores":original_first.len(),
        "controller_difference":controller_difference,"native_controller_byte":controller_difference.map(|(s,i)|compiled.state.controllers[s].bytes[i]),
        "original_controller_byte":controller_difference.map(|(s,i)|expected.controllers[s].bytes[i]),
        "LFO_equal":compiled.state.lfos==expected.lfos,"motion_equal":compiled.state.motion==expected.motion,
        "context_equal":compiled.state.context==expected.context,"native_seed":compiled.state.random_seed,"original_seed":expected.random_seed,
        "modulations_equal":compiled.state.modulations==expected.modulations,
        "native_all_ready_return_clock_diagnostic":native_return,
        "original_return_clock":returned["clock"].as_u64().unwrap()-number("clock"),
        "native_all_ready_ACK_clocks_diagnostic":native_acks,"original_ACK_clocks_assertions":source_acks,
        "all_ready_diagnostic_is_not_audio_or_timing_qualification":true,
        "input_snapshot_is_before_whole_constructor":true,"source_outputs_used_only_as_assertions":true,
        "independent_IRQ_receiver_DMA_and_complete_WAV_qualified":false,"full_constructor_enabled_in_live_player":false,"complete_native_engine":false});
    fs::write(
        out.join("live-construction-state-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("{report}");
    if !passed {
        return Err("Live constructor state or packets differ".into());
    }
    Ok(())
}
