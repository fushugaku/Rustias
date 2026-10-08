//! Joint all-actor EG2/Unison gain and dry-bus qualification.
//! The source supplies actor lifetimes, other graph controls and delivery clocks.
//! Envelope levels, amplifier targets and group gain come from production Rust.
use radias_synth_application::{
    VoiceRenderer,
    amplifier::{AmplifierController, ControllerTables},
    polyphony::{ActiveVoice, PolyphonicRenderer},
    program::TimbreControls,
};
use radias_synth_domain::program::Program;
use radias_synth_infrastructure::{
    firmware::{MasterTables, amplifier_tables, envelope_curves, envelope_timing_tables},
    prepared::PreparedVoice,
    wav,
};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf};

type Error = Box<dyn std::error::Error>;
fn word(raw: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(raw[n * 4..n * 4 + 4].try_into().unwrap())
}
fn number(e: &Value, key: &str) -> Result<u64, Error> {
    e[key]
        .as_u64()
        .ok_or_else(|| format!("Missing {key}: {e}").into())
}
fn field(e: &Value, key: &str, n: usize) -> Result<u32, Error> {
    e[key][n]
        .as_u64()
        .map(|v| v as u32)
        .ok_or_else(|| format!("Missing {key}[{n}]: {e}").into())
}
fn read_events(path: PathBuf) -> Result<Vec<Value>, Error> {
    fs::read_to_string(path)?
        .lines()
        .map(|l| serde_json::from_str(l).map_err(Into::into))
        .collect()
}
fn actor_slot(e: &Value) -> Result<usize, Error> {
    let base = number(e, "voice")?;
    let relative = base
        .checked_sub(0x0c0cea54)
        .ok_or("Actor address below pool")?;
    if !relative.is_multiple_of(0x1f0) || relative / 0x1f0 >= 24 {
        return Err("Invalid original physical actor address".into());
    }
    Ok((relative / 0x1f0) as usize)
}

fn main() -> Result<(), Error> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = PathBuf::from(args.first().ok_or("Repository required")?);
    let name = args.get(1).ok_or("Original capture required")?;
    let raw_program = fs::read(args.get(2).ok_or("Controlled source program required")?)?;
    let program = Program::from_bytes(&raw_program).map_err(|_| "Invalid SYS program")?;
    let controls =
        TimbreControls::from_timbre(program.timbre(0).unwrap()).map_err(|_| "Invalid patch")?;
    let layout = controls.voice_group.layout(controls.oscillator_selection);
    if layout.count < 2 || controls.modulation.routes.iter().any(|r| r.intensity != 64) {
        return Err(
            "This joint gate requires grouped voices and neutral virtual-patch depths".into(),
        );
    }
    let amp_program = controls.amplifier(0x7f00, None, layout.bank);
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let tables = ControllerTables {
        curves: envelope_curves(&sys)?,
        timing: envelope_timing_tables(&sys)?,
        amplifier: amplifier_tables(&sys)?,
    };
    let table = MasterTables::from_host_stream(&fs::read(
        root.join("firmware/dsp-master-host-stream.bin"),
    )?)?
    .waveform()?;
    let output = root.join("runs/native-clone");
    let mut ordered: Vec<_> =
        read_events(output.join(format!("{name}-native-envelope-events.jsonl")))?
            .into_iter()
            .filter(|e| e["kind"] != "commit")
            .collect();
    let mut commits = read_events(output.join(format!("{name}-group-amplifier-commits.jsonl")))?;
    for e in &mut commits {
        e["kind"] = Value::from("delivery");
    }
    ordered.extend(commits);
    if ordered.iter().any(|e| e["group_order"].as_u64().is_none()) {
        return Err("Capture predates all-actor controller ordering; regenerate it".into());
    }
    ordered.sort_by_key(|e| e["group_order"].as_u64().unwrap());
    if ordered
        .windows(2)
        .any(|w| w[0]["group_order"] == w[1]["group_order"])
    {
        return Err("Original controller/delivery ordering is not unique".into());
    }
    let mut controllers = [None; 24];
    let mut published = [0i16; 24];
    let mut deliveries = BTreeMap::<usize, Vec<(usize, i16)>>::new();
    let mut births = 0;
    let mut ticks = 0;
    let mut releases = 0;
    let mut compilations = 0;
    let mut delivery_count = 0;
    let mut observed_mask = 0u32;
    for event in &ordered {
        let kind = event["kind"].as_str().ok_or("Event kind absent")?;
        if kind == "delivery" {
            let slot = number(event, "slot")? as usize;
            let processor = number(event, "processor")? as usize;
            if slot >= 24 || slot / 12 != processor {
                return Err("Delivery processor/slot differs".into());
            }
            let target = if number(event, "pc")? == 0xd534 {
                0
            } else {
                published[slot]
            };
            if target as u16 as u64 != number(event, "expected_target")? {
                return Err(format!(
                    "Native group AMP delivery differs, slot{slot}: {target} vs {event}"
                )
                .into());
            }
            deliveries
                .entry(number(event, "frame")? as usize)
                .or_default()
                .push((slot, target));
            delivery_count += 1;
            continue;
        }
        let slot = actor_slot(event)?;
        let velocity = field(event, "parameters", 7)? as u8;
        let note = field(event, "parameters", 8)? as u8;
        let parameters = amp_program.parameters(note, velocity);
        let expected_program = [
            parameters.attack,
            parameters.decay,
            parameters.sustain,
            parameters.release,
            parameters.curve,
            parameters.velocity_sensitivity,
            parameters.key_tracking,
        ];
        for (i, expected) in expected_program.into_iter().enumerate() {
            if field(event, "parameters", i)? != expected as u32 {
                return Err(format!("Stored EG2 controls differ: {event}").into());
            }
        }
        if kind == "note_on" {
            controllers[slot] = Some(AmplifierController::new_at_phase(
                parameters,
                amp_program.control(velocity),
                &tables,
                number(event, "initial_phase")? as u32,
            ));
            births += 1;
            observed_mask |= 1 << slot;
            continue;
        }
        let controller = controllers[slot]
            .as_mut()
            .ok_or_else(|| format!("Actor{slot} service precedes native note-on: {event}"))?;
        controller.retarget(note, velocity);
        match kind {
            "tick" => {
                controller.service(
                    &tables,
                    event["acknowledged"]
                        .as_bool()
                        .ok_or("Acknowledgement absent")?,
                );
                ticks += 1;
            }
            "release" => {
                controller.release(&tables);
                releases += 1;
            }
            "amplifier" => {
                let control = controller.control();
                let expected = [
                    control.level as u32,
                    control.level_offset as u8 as u32,
                    control.source_gain as u32,
                    controller.envelope.segment.level as u32,
                    control.velocity as u32,
                    control.velocity_sensitivity as u32,
                    0,
                    0,
                    0,
                    raw_program[25] as u32,
                    layout.bank as u32,
                ];
                for (i, value) in expected.into_iter().enumerate() {
                    if field(event, "control", i)? != value {
                        return Err(format!(
                            "Native EG2/group AMP input{i} differs, slot{slot}: {value} vs {event}"
                        )
                        .into());
                    }
                }
                published[slot] = controller.modulations([0; 2], &tables);
                compilations += 1;
            }
            _ => return Err(format!("Unknown controller event: {event}").into()),
        }
    }
    if births < 2 * layout.count as usize
        || releases < 2 * layout.count as usize
        || ticks < 100
        || delivery_count < 100
    {
        return Err("Joint gate did not exercise enough group births/services/releases".into());
    }

    let raw = fs::read(output.join(format!("{name}-poly-voice-inputs.bin")))?;
    let slots = fs::read(output.join(format!("{name}-poly-voice-slots.bin")))?;
    if raw.len() % 704 != 0 || slots.len() != raw.len() / 704 * 16 {
        return Err("Incomplete graph observations".into());
    }
    let mut grouped = BTreeMap::<usize, Vec<u8>>::new();
    for (r, s) in raw.chunks_exact(704).zip(slots.chunks_exact(16)) {
        let processor = word(s, 1) as usize;
        let address = word(s, 2);
        if processor > 1 || address < 0x2000 || !(address - 0x2000).is_multiple_of(160) {
            return Err("Unsupported graph address".into());
        }
        let local = ((address - 0x2000) / 160) as usize;
        if local >= 12 || word(s, 0) != word(r, 0) {
            return Err("Graph slot or clock differs".into());
        }
        grouped
            .entry(local + processor * 12)
            .or_default()
            .extend_from_slice(r);
    }
    let plans: Vec<_> = grouped
        .iter()
        .map(|(&slot, raw)| Ok((slot, PreparedVoice::from_reference_va_parameters(raw)?)))
        .collect::<Result<_, &'static str>>()?;
    if plans.len() != observed_mask.count_ones() as usize {
        return Err("Controller and rendered actor counts differ".into());
    }
    let originals = [
        fs::read(output.join(format!("{name}-original-complete-mix.wav")))?,
        fs::read(output.join(format!("{name}-original-slave-mix.wav")))?,
    ];
    for original in &originals {
        if original.get(36..40) != Some(b"data")
            || original.len() != word(original, 10) as usize + 44
        {
            return Err("Original complete WAV format or extent differs".into());
        }
    }
    let lengths = originals.each_ref().map(|o| (o.len() - 44) / 32);
    let frames = *lengths.iter().max().unwrap();
    let mut resets = BTreeMap::<usize, Vec<(usize, i16, bool)>>::new();
    for event in read_events(output.join(format!("{name}-poly-state-events.jsonl")))? {
        let slot = number(&event, "slot")? as usize + 12 * number(&event, "processor")? as usize;
        let value = number(&event, "value")? as i16;
        let filter = match event["kind"].as_str() {
            Some("amplifier_reset") => false,
            Some("filter_reset") if value == 0 => true,
            _ => return Err("Unqualified actor state reset".into()),
        };
        resets
            .entry(number(&event, "frame")? as usize)
            .or_default()
            .push((slot, value, filter));
    }
    let mut pool = PolyphonicRenderer::default();
    let mut audio = [Vec::with_capacity(frames), Vec::with_capacity(frames)];
    let mut errors = 0;
    let mut active_maximum = 0;
    let mut delivered = [0i16; 24];
    for frame in 0..frames {
        for (program, (slot, plan)) in plans.iter().enumerate() {
            if frame as u64 == plan.reference_start_frame {
                let mut renderer = VoiceRenderer::new(plan.initial, plan.parameters);
                renderer.set_envelope_target(delivered[*slot]);
                pool.install(
                    *slot,
                    0,
                    ActiveVoice {
                        renderer,
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
                        note: 60,
                        velocity: 100,
                        held: true,
                        program,
                        bus: plan.bus,
                    },
                );
            }
            if frame as u64 == plan.reference_start_frame + plan.reference_voice_frames as u64 {
                pool.remove(*slot);
            }
        }
        if let Some(events) = resets.get(&frame) {
            for &(slot, value, filter) in events {
                if filter {
                    pool.reset_filter(slot);
                } else {
                    pool.reset_amplifier(slot, value);
                }
            }
        }
        if let Some(events) = deliveries.get(&frame) {
            for &(slot, target) in events {
                delivered[slot] = target;
                pool.publish_amplifier(slot, target);
            }
        }
        active_maximum = active_maximum.max(pool.active_count());
        let buses = pool.next_buses(&table, None, |p| &plans[p].1.events);
        for processor in 0..2 {
            if frame >= lengths[processor] {
                continue;
            }
            let actual = core::array::from_fn::<_, 8, _>(|c| {
                if c % 2 == 0 {
                    buses[processor][c / 2].left
                } else {
                    buses[processor][c / 2].right
                }
            });
            for (channel, sample) in actual.iter().enumerate() {
                let expected = word(
                    &originals[processor][44 + 32 * frame..44 + 32 * (frame + 1)],
                    channel,
                ) as i32;
                if sample.0 != expected {
                    if errors < 5 {
                        eprintln!(
                            "Processor{processor} frame{frame} channel{channel}: {} != {expected}",
                            sample.0
                        );
                    }
                    errors += 1;
                }
            }
            audio[processor].push(actual);
        }
    }
    for (processor, suffix) in ["mix", "slave"].into_iter().enumerate() {
        wav::write_buses(
            &output.join(format!("{name}-rust-group-controller-{suffix}.wav")),
            &audio[processor],
        )?;
    }
    let report = serde_json::json!({"name":name,"passed":errors==0,"voices_per_note":layout.count,"gain_bank":layout.bank,
        "native_controller_actors":observed_mask.count_ones(),"maximum_simultaneous":active_maximum,
        "native_envelope_births":births,"native_controller_ticks":ticks,"native_releases":releases,
        "native_amplifier_compilations":compilations,"asserted_delivery_targets":delivery_count,
        "complete_wav_frames":lengths,"different_samples":errors,
        "production_amplifier_controller_used":true,"original_amp_targets_used_as_assertions_only":true,
        "original_other_graph_controls_initial_states_actor_lifetimes_delivery_clocks_accepted":true,
        "joint_native_allocation_pitch_pan_lfo_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        output.join(format!("{name}-group-controller-parity.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Native all-actor envelope/group-gain complete WAV differs".into());
    }
    Ok(())
}
