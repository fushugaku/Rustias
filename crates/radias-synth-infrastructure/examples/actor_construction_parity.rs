//! Whole original SYS01e838, evolving24-actor state and both DSP bank images.
use radias_synth_application::{
    actor_construction::ActorConstructionRun,
    synthesis_transport::{DeliveredSynthesisParameter, SynthesisParameterTransport},
};
use radias_synth_domain::{
    actor_amplifier_preparation::ActorAmplifierPorts,
    actor_construction::{
        ActorConstructionRequest, ActorConstructionState, ActorConstructionTables,
    },
    actor_control_state::ActorControlState,
    actor_copy::ActorTemplateBinding,
    actor_descriptors::{DescriptorOperation, DescriptorPlan},
    actor_lfo_initialization::ActorLfoState,
    actor_lifecycle::ActorLifecycle,
    actor_note_initialization::{
        ActorNoteInitializationPorts, ActorNoteInitializationTables, ActorPortamentoInitialization,
    },
    actor_note_preparation::{ActorNotePreparationPorts, ActorNotePreparationTables},
    actor_pitch_preparation::ActorPitchPorts,
    actor_virtual_patch::ActorVirtualPatchPorts,
    complete_actor_startup::CompleteStartupTables,
    dsp_control::{DspEndpoint, ParameterPacket},
    manual_parameters::ManualCompilerTables,
    modulation::ModulationTargets,
    motion_initialization::MotionControlState,
    note_modulators::NoteModulatorTables,
    note_refresh::NoteRefreshTables,
    parameter_template::ParameterTemplate,
    raw_note_scale::RawNoteScaleContext,
    virtual_patch_live::LiveCompilerTables,
};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
struct Reader {
    bytes: Vec<u8>,
    cursor: usize,
}
impl Reader {
    fn word(&mut self) -> u32 {
        u32::from_le_bytes(self.bytes())
    }
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let v = self.bytes[self.cursor..self.cursor + N].try_into().unwrap();
        self.cursor += N;
        v
    }
    fn lfos(&mut self) -> [ActorLfoState; 2] {
        core::array::from_fn(|_| ActorLfoState {
            bytes: self.bytes(),
        })
    }
    fn modulations(&mut self) -> ModulationTargets {
        ModulationTargets {
            values: core::array::from_fn(|_| self.word() as i32),
            linked_pitch: self.word() as i32,
        }
    }
}
fn banks(t: &SynthesisParameterTransport) -> Vec<u16> {
    let mut result = Vec::with_capacity(11776);
    for chip in 0..2 {
        for slot in chip * 12..chip * 12 + 12 {
            result.extend(t.parameter_state(slot));
        }
        for slot in chip * 12..chip * 12 + 12 {
            result.extend(t.physical_parameter_state(slot));
        }
        for index in 0..20 {
            result.extend(
                t.parameter_template_state(
                    if chip == 0 {
                        DspEndpoint::Master
                    } else {
                        DspEndpoint::Slave
                    },
                    index,
                )
                .unwrap()
                .words,
            );
        }
    }
    result
}
fn restore(t: &mut SynthesisParameterTransport, words: &[u16], slave_templates: bool) {
    for chip in 0..2 {
        let start = 5888 * chip;
        for local in 0..12 {
            t.restore_construction_state(
                chip * 12 + local,
                words[start + 160 * local..start + 160 * (local + 1)]
                    .try_into()
                    .unwrap(),
                words[start + 1920 + 64 * local..start + 1920 + 64 * (local + 1)]
                    .try_into()
                    .unwrap(),
            );
        }
        for index in 0..20 {
            if chip == 1 && !slave_templates {
                continue;
            }
            t.install_parameter_template(
                if chip == 0 {
                    DspEndpoint::Master
                } else {
                    DspEndpoint::Slave
                },
                index,
                ParameterTemplate {
                    words: words[start + 2688 + 160 * index..start + 2688 + 160 * (index + 1)]
                        .try_into()
                        .unwrap(),
                },
            )
            .unwrap();
        }
    }
}
fn payloads(slot: usize, plan: &DescriptorPlan) -> impl Iterator<Item = (usize, Vec<u16>)> + '_ {
    plan.operations().iter().filter_map(move |op| {
        if let DescriptorOperation::Send {
            sender,
            offset,
            value,
            ..
        } = *op
        {
            Some((
                slot / 12,
                ParameterPacket::from_sender(
                    sender,
                    0x2000 + 160 * (slot % 12) as u32 + u32::from(offset),
                    value,
                )
                .unwrap()
                .words()
                .to_vec(),
            ))
        } else {
            None
        }
    })
}
fn plan_clocks(slot: usize, plan: &DescriptorPlan) -> u64 {
    use radias_synth_application::{
        dsp_transport::ParameterSendRequest, parameter_transport::OrderedParameterTransport,
    };
    let mut queue = OrderedParameterTransport::<512>::default();
    for operation in plan.operations() {
        match *operation {
            DescriptorOperation::Work(clocks) => queue.enqueue_work(0, clocks).unwrap(),
            DescriptorOperation::Send {
                sender,
                offset,
                value,
                before,
                after,
            } => queue
                .enqueue_with_spacing(
                    ParameterSendRequest {
                        endpoint: if slot < 12 {
                            DspEndpoint::Master
                        } else {
                            DspEndpoint::Slave
                        },
                        sender,
                        address: 0x2000 + 160 * (slot % 12) as u32 + u32::from(offset),
                        value,
                        available_clock: 0,
                    },
                    before,
                    after,
                )
                .unwrap(),
        }
    }
    queue.advance_until(10000000, |_| 0, |_, _| {});
    queue.caller_available_clock()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
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
    let mut input = Reader {
        bytes: fs::read(out.join("actor-construction-original.bin"))?,
        cursor: 0,
    };
    if input.word() != 0x434e5331 {
        return Err("Unsupported constructor observation".into());
    }
    let cases = input.word();
    let (mut state_errors, mut packet_errors, mut transport_errors, mut bank_words, mut receives) =
        (0u32, 0u32, 0u32, 0u64, 0u32);
    let mut first_state = serde_json::Value::Null;
    let mut first_packet = serde_json::Value::Null;
    let mut first_transport = serde_json::Value::Null;
    let mut max_steps = 0;
    let mut max_operations = 0;
    let mut combinations = [[0u32; 13]; 24];
    let mut slots = [0u32; 24];
    let mut timbres = [0u32; 4];
    let mut banks_covered = [0u32; 9];
    let mut busy_covered = [0u32; 4];
    let mut atomic_rejections = 0u32;
    let mut publication_wait_cases = 0u32;
    for _ in 0..cases {
        if input.word() != 40 {
            return Err("Unsupported constructor ports".into());
        }
        let m: [u32; 40] = core::array::from_fn(|_| input.word());
        let mut assignments = [0; 5];
        let mut values = [0; 5];
        for i in 0..5 {
            assignments[i] = input.word() as u8;
            values[i] = input.word() as i8;
        }
        let body: [u8; 104] = input.bytes();
        let group_ordinals = input.bytes();
        let allocation_flags = input.bytes();
        let context = input.bytes();
        let controllers = core::array::from_fn(|_| ActorControlState {
            bytes: input.bytes(),
        });
        let lfos = core::array::from_fn(|_| input.lfos());
        let motion = core::array::from_fn(|_| MotionControlState {
            bytes: input.bytes(),
        });
        let shared: [[_; 2]; 4] = core::array::from_fn(|_| input.lfos());
        let custom = input.bytes::<256>();
        let modulations = input.modulations();
        let before_dsp: Vec<u16> = (0..11776).map(|_| input.word() as u16).collect();
        let returned = u64::from(input.word());
        let expected_seed = input.word() as u16;
        let expected_active = input.word();
        let expected_pending = input.word();
        let expected_flags: [u8; 24] = input.bytes();
        let expected_context: [u8; 100] = input.bytes();
        let expected_controllers: [ActorControlState; 24] =
            core::array::from_fn(|_| ActorControlState {
                bytes: input.bytes(),
            });
        let expected_lfos: [[_; 2]; 24] = core::array::from_fn(|_| input.lfos());
        let expected_motion: [MotionControlState; 24] =
            core::array::from_fn(|_| MotionControlState {
                bytes: input.bytes(),
            });
        let expected_modulations = input.modulations();
        let count = input.word();
        let mut packets = Vec::new();
        for _ in 0..count {
            let chip = input.word() as usize;
            let ack = u64::from(input.word());
            let length = input.word();
            let payload: Vec<u16> = (0..length).map(|_| input.word() as u16).collect();
            let n = input.word();
            let changes: Vec<(usize, u16)> = (0..n)
                .map(|_| (input.word() as usize, input.word() as u16))
                .collect();
            packets.push((chip, ack, payload, changes));
        }
        let prior = ActorConstructionState {
            controllers,
            lfos,
            motion,
            allocation_flags,
            context,
            random_seed: m[6] as u16,
            pending_retirement: m[5],
            lifecycle: ActorLifecycle { active: m[4] },
            modulations,
        };
        let ports = ActorNotePreparationPorts {
            motion_assignments: [m[31] as u8, m[32] as u8, m[33] as u8],
            motion_program_flags: m[29] as u8,
            motion_global_flags: m[30] as u8,
            lfo_clock_rate: m[7],
            midi_pan: m[22] as u8,
            note: ActorNoteInitializationPorts {
                master_tune: m[8] as i32,
                pitch: ActorPitchPorts {
                    timbre: m[14] as u8,
                    midi_mode: m[15] as u8,
                    bend_q16: m[16] as i32,
                    wheel: m[18] as u8,
                    common_receive_flags: m[9] as u8,
                },
                sources: ActorVirtualPatchPorts {
                    bend: m[17] as i16,
                    wheel: m[18] as u8,
                    auxiliary: m[19] as i16,
                    midi_receive_flags: m[10] as u8,
                    assignments,
                    assignable_values: values,
                },
                portamento: ActorPortamentoInitialization {
                    time: m[20] as u8,
                    switch_required: m[9] & 8 != 0,
                    switch: m[21] != 0,
                    context_flags: m[23] as u8,
                    gate_flags: m[24] as u8,
                    context_pitch_q16: m[25] as i32,
                },
                scale: RawNoteScaleContext {
                    selection: m[26] as u8,
                    global_transpose: (m[27] != 0).then_some(m[28] as i8),
                    custom_cents: custom.map(|v| v as i8),
                },
            },
            amplifier: ActorAmplifierPorts {
                configuration_mode: m[11] as u8,
                owner_receive_flags: m[10] as u8,
                context_gain: m[12] as u16,
                midi_volume: m[13] as u8,
            },
        };
        let request = || ActorConstructionRequest {
            selected: m[1],
            midi_word: m[2],
            owner_identity: m[35],
            body: &body,
            ports,
            shared_lfos: shared[m[14] as usize],
            group_ordinals,
            detune_amount: m[38] as u8,
            spread_amount: m[39] as u8,
            binding: ActorTemplateBinding {
                program_kind: m[15] as u8,
                timbre_id: m[14] as u8,
                drum_instrument: m[37] as u8,
                ordinary_address: m[36] as u16,
            },
        };
        if m[1] & 0x00ff_ffff != 0 {
            let primary = usize::from(body[22] & 15) * 4 + usize::from((body[22] >> 4) & 3);
            let shaper = if body[46] & 3 == 0 {
                0
            } else if body[46] & 3 == 1 {
                1
            } else {
                usize::from(body[47] & 15) + 2
            };
            combinations[primary][shaper] += 1;
            for (slot, count) in slots.iter_mut().enumerate() {
                if m[1] & (1 << slot) != 0 {
                    *count += 1;
                }
            }
            timbres[m[14] as usize] += 1;
            banks_covered[m[34] as usize] += 1;
            let busy = [0, 64, 333, 3000].iter().position(|v| *v == m[3]).unwrap();
            busy_covered[busy] += 1;
        }
        let compiled = prior
            .construct(request(), &tables)
            .map_err(|e| format!("Variant{}: {e:?}", m[0]))?;
        max_steps = max_steps.max(compiled.steps().len());
        max_operations = max_operations.max(
            compiled
                .steps()
                .iter()
                .map(|s| s.publication.operations().len())
                .sum::<usize>(),
        );
        let candidate = &compiled.state;
        let state_good = candidate.controllers == expected_controllers
            && candidate.lfos == expected_lfos
            && candidate.motion == expected_motion
            && candidate.allocation_flags == expected_flags
            && candidate.context == expected_context
            && candidate.random_seed == expected_seed
            && candidate.lifecycle.active == expected_active
            && candidate.pending_retirement == expected_pending
            && candidate.modulations == expected_modulations;
        if !state_good {
            state_errors += 1;
            if first_state.is_null() {
                let actor_difference = (0..24).find_map(|s| {
                    (0..496)
                        .find(|i| {
                            candidate.controllers[s].bytes[*i] != expected_controllers[s].bytes[*i]
                        })
                        .map(|i| (s, i))
                });
                first_state = serde_json::json!({"variant":m[0],"actor_difference":actor_difference,
                    "native_actor_byte":actor_difference.map(|(s,i)|candidate.controllers[s].bytes[i]),
                    "original_actor_byte":actor_difference.map(|(s,i)|expected_controllers[s].bytes[i]),
                    "lfos_equal":candidate.lfos==expected_lfos,"motion_equal":candidate.motion==expected_motion,
                    "context_equal":candidate.context==expected_context,"flags_equal":candidate.allocation_flags==expected_flags,
                    "native_seed":candidate.random_seed,"original_seed":expected_seed,"native_active":candidate.lifecycle.active,
                    "original_active":expected_active,"modulations_equal":candidate.modulations==expected_modulations});
            }
        }
        let native: Vec<_> = compiled
            .steps()
            .iter()
            .flat_map(|s| payloads(s.slot, &s.publication))
            .collect();
        let original: Vec<_> = packets.iter().map(|(c, _, p, _)| (*c, p.clone())).collect();
        if native != original {
            packet_errors += 1;
            if first_packet.is_null() {
                let position = native
                    .iter()
                    .zip(&original)
                    .position(|(a, b)| a != b)
                    .unwrap_or(native.len().min(original.len()));
                first_packet = serde_json::json!({"variant":m[0],"position":position,"native_count":native.len(),"original_count":original.len(),
                    "native":native.get(position),"original":original.get(position)});
            }
        }
        if atomic_rejections == 0 && m[1] & 0x00ff_ffff == 0x00ff_ffff {
            use radias_synth_application::dsp_transport::SendQueueError;
            use radias_synth_domain::actor_construction::ActorConstructionError;
            for case in 0..3 {
                let mut transport = SynthesisParameterTransport::default();
                if case != 0 {
                    transport.configure_constructor_filter_mix(
                        radias_synth_domain::filter_control::FilterMixTable {
                            weights: mix.weights,
                        },
                    );
                }
                if case != 1 {
                    transport.configure_pitch_receivers(pitch_rom.clone(), dispatch);
                }
                restore(&mut transport, &before_dsp, case != 2);
                transport.set_actor_lifecycle(prior.lifecycle);
                let mut run = ActorConstructionRun::prepare(0, &prior, request(), &tables)
                    .map_err(|e| format!("{e:?}"))?;
                let expected = [
                    SendQueueError::MissingFilterContext,
                    SendQueueError::MissingPitchContext,
                    SendQueueError::MissingParameterTemplate,
                ][case];
                let result = run.enqueue_available(&mut transport);
                if result != Err(expected)
                    || transport.pending() != 0
                    || transport.actor_lifecycle() != prior.lifecycle
                {
                    return Err(format!(
                        "Non-atomic whole constructor rejection{case}: {result:?}"
                    )
                    .into());
                }
                let mut state = prior;
                if run.finish(0, &mut state, &mut transport) || state != prior {
                    return Err("Rejected constructor committed state".into());
                }
                atomic_rejections += 1;
            }
            let mut bad_body = body;
            bad_body[46] = 2;
            bad_body[47] = 15;
            let mut bad_request = request();
            bad_request.body = &bad_body;
            if ActorConstructionRun::prepare(0, &prior, bad_request, &tables).is_ok() {
                return Err("Unsupported late WS accepted".into());
            }
            atomic_rejections += 1;
            let mut bad_prior = prior;
            bad_prior.context[0x57] = 9;
            if !matches!(
                ActorConstructionRun::prepare(0, &bad_prior, request(), &tables),
                Err(ActorConstructionError::InvalidGroupBank)
            ) {
                return Err("Unsupported group bank accepted".into());
            }
            atomic_rejections += 1;
            let mut transport = SynthesisParameterTransport::default();
            transport.configure_constructor_filter_mix(
                radias_synth_domain::filter_control::FilterMixTable {
                    weights: mix.weights,
                },
            );
            transport.configure_pitch_receivers(pitch_rom.clone(), dispatch);
            restore(&mut transport, &before_dsp, true);
            transport.set_actor_lifecycle(prior.lifecycle);
            let idle = prior.controllers[0]
                .compile_complete_startup(7, 0, &body, prior.lifecycle, &startup)
                .map_err(|e| format!("{e:?}"))?;
            assert_eq!(idle.publication.operations().len(), 1);
            for _ in 0..512 {
                transport
                    .publish_actor_descriptors(0, 0, &idle.publication)
                    .map_err(|e| format!("{e:?}"))?;
            }
            let mut run = ActorConstructionRun::prepare(0, &prior, request(), &tables)
                .map_err(|e| format!("{e:?}"))?;
            run.enqueue_available(&mut transport)
                .map_err(|e| format!("{e:?}"))?;
            let mut state = prior;
            if transport.pending() != 512
                || run.finish(0, &mut state, &mut transport)
                || state != prior
            {
                return Err("Full FIFO changed constructor state or old queue".into());
            }
            publication_wait_cases += 1;
            let mut transport = SynthesisParameterTransport::default();
            if run.finish(0, &mut state, &mut transport) || state != prior {
                return Err("Constructor committed before publication".into());
            }
            publication_wait_cases += 1;
        }
        for partition in [1u64, 31, 3000] {
            let mut transport = SynthesisParameterTransport::default();
            transport.configure_constructor_filter_mix(
                radias_synth_domain::filter_control::FilterMixTable {
                    weights: mix.weights,
                },
            );
            transport.configure_pitch_receivers(pitch_rom.clone(), dispatch);
            transport.set_actor_lifecycle(prior.lifecycle);
            restore(&mut transport, &before_dsp, true);
            let mut run = ActorConstructionRun::prepare(0, &prior, request(), &tables)
                .map_err(|e| format!("{e:?}"))?;
            let mut clock = 0;
            let mut ready_at = [u64::from(m[3]); 2];
            let mut expected_bank = before_dsp.clone();
            let mut native_acks = Vec::new();
            let mut seen_count = 0;
            let mut good = true;
            let mut first_word = None;
            for (chip, ack, payload, changes) in &packets {
                for (i, v) in changes {
                    expected_bank[*i] = *v;
                }
                let mut seen = None;
                while clock < *ack {
                    run.enqueue_available(&mut transport)
                        .map_err(|e| format!("{e:?}"))?;
                    clock = (clock + partition).min(*ack);
                    transport.advance_until_with_readiness(clock,|poll|(u8::from(poll<ready_at[0]))|(u8::from(poll<ready_at[1])<<1),
                        |time,slot,event|{seen_count+=1;native_acks.push(time);seen=Some((time,slot/12,matches!(event,DeliveredSynthesisParameter::ActorState{opcode,..}if opcode==payload[1])));});
                }
                let actual = banks(&transport);
                let difference = actual.iter().zip(&expected_bank).position(|(a, b)| a != b);
                if first_word.is_none() {
                    first_word = difference;
                }
                good &= seen == Some((*ack, *chip, true)) && difference.is_none();
                ready_at[*chip] = *ack + u64::from(m[3]);
                bank_words += 11776;
            }
            while clock < returned {
                run.enqueue_available(&mut transport)
                    .map_err(|e| format!("{e:?}"))?;
                clock = (clock + partition).min(returned);
                transport.advance_until_with_readiness(clock, |_| 0, |_, _, _| good = false);
            }
            let mut committed = prior;
            good &= run.finish(returned, &mut committed, &mut transport)
                && committed == compiled.state
                && seen_count == count
                && transport.pending() == 0
                && transport.caller_available_clock() == returned;
            if !good {
                transport_errors += 1;
                if first_transport.is_null() {
                    let first_ack = native_acks
                        .iter()
                        .zip(&packets)
                        .position(|(a, (_, b, _, _))| a != b);
                    first_transport = serde_json::json!({"variant":m[0],"partition":partition,"first_bank_word":first_word,
                        "native_return":transport.caller_available_clock(),"original_return":returned,"pending":transport.pending(),
                        "seen_count":seen_count,"original_count":count,"first_ack_index":first_ack,
                        "native_ack":first_ack.map(|i|native_acks[i]),"original_ack":first_ack.map(|i|packets[i].1),
                        "functional_steps":compiled.steps().iter().map(|step|(step.slot,plan_clocks(step.slot,&step.publication))).collect::<Vec<_>>()});
                }
            }
        }
        receives += count;
    }
    if input.cursor != input.bytes.len() {
        return Err("Extra constructor observation bytes".into());
    }
    let passed = state_errors == 0 && packet_errors == 0 && transport_errors == 0;
    let report = serde_json::json!({"passed":passed,"whole_SYS01e838_calls":cases,"original_E319_receives":receives,
        "state_errors":state_errors,"packet_errors":packet_errors,"transport_errors":transport_errors,
        "first_state_error":first_state,"first_packet_error":first_packet,"first_transport_error":first_transport,
        "all_actor_physical_and_template_bank_words_compared":bank_words,"max_steps":max_steps,"max_operations":max_operations,
        "common_FIFO_capacity":512,"source_outputs_used_only_as_assertions":true,
        "atomic_rejections":atomic_rejections,"publication_wait_cases":publication_wait_cases,
        "all24_primary_and13_WS_input_combinations_covered":combinations.iter().flatten().all(|v|*v>0),
        "all24_actor_slots_covered":slots.iter().all(|v|*v>0),
        "all4_timbres_covered":timbres.iter().all(|v|*v>0),
        "all9_group_banks_covered":banks_covered.iter().all(|v|*v>0),
        "all4_ready_policies_covered":busy_covered.iter().all(|v|*v>0),
        "full_note_constructor_enabled_in_live_player":false,"independent_production_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("actor-construction-parity.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("{report}");
    if !passed {
        return Err("Whole actor constructor differs".into());
    }
    Ok(())
}
