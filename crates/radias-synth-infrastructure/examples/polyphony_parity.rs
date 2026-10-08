//! Original multi-voice control schedules, independently rendered native buses.
use radias_synth_application::{
    VoiceRenderer,
    polyphony::{ActiveVoice, PolyphonicRenderer},
};
use radias_synth_infrastructure::{firmware::MasterTables, prepared::PreparedVoice, wav};
use std::{collections::BTreeMap, fs, path::PathBuf};
fn word(raw: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(raw[n * 4..n * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = PathBuf::from(args.first().ok_or("Repository required")?);
    let name = args.get(1).ok_or("Original capture required")?;
    let output = root.join("runs/native-clone");
    let table = MasterTables::from_host_stream(&fs::read(
        root.join("firmware/dsp-master-host-stream.bin"),
    )?)?
    .waveform()?;
    let dual = output
        .join(format!("{name}-poly-voice-inputs.bin"))
        .exists();
    let raw = fs::read(output.join(format!(
        "{name}-{}",
        if dual {
            "poly-voice-inputs.bin"
        } else {
            "voice-va-inputs.bin"
        }
    )))?;
    let slots = fs::read(output.join(format!(
        "{name}-{}",
        if dual {
            "poly-voice-slots.bin"
        } else {
            "voice-slots.bin"
        }
    )))?;
    if raw.len() % 704 != 0 || slots.len() != raw.len() / 704 * 16 {
        return Err("Incomplete voice/slot capture".into());
    }
    let mut grouped = BTreeMap::<usize, Vec<u8>>::new();
    for (r, s) in raw.chunks_exact(704).zip(slots.chunks_exact(16)) {
        if word(s, 1) > 1 || word(s, 2) < 0x2000 || !(word(s, 2) - 0x2000).is_multiple_of(160) {
            return Err("Unsupported processor/parameter address".into());
        }
        let local = ((word(s, 2) - 0x2000) / 160) as usize;
        let slot = local + word(s, 1) as usize * 12;
        if local >= 12 || word(s, 0) != word(r, 0) {
            return Err("Voice slot or clock differs".into());
        }
        grouped.entry(slot).or_default().extend_from_slice(r);
    }
    if grouped.len() < 2 {
        return Err("A polyphony gate requires multiple voices".into());
    }
    let plans: Vec<_> = grouped
        .iter()
        .map(|(&slot, raw)| Ok((slot, PreparedVoice::from_reference_va_parameters(raw)?)))
        .collect::<Result<_, &'static str>>()?;
    let originals = if dual {
        vec![
            fs::read(output.join(format!("{name}-original-complete-mix.wav")))?,
            fs::read(output.join(format!("{name}-original-slave-mix.wav")))?,
        ]
    } else {
        vec![fs::read(
            output.join(format!("{name}-original-complete-mix.wav")),
        )?]
    };
    for original in &originals {
        if original.get(36..40) != Some(b"data")
            || original.len() != word(original, 10) as usize + 44
        {
            return Err("Original complete WAV format or extent differs".into());
        }
    }
    let lengths: Vec<_> = originals.iter().map(|o| (o.len() - 44) / 32).collect();
    let frames = *lengths.iter().max().ok_or("No original processor")?;
    let mut pool = PolyphonicRenderer::default();
    let mut audio = [Vec::with_capacity(frames), Vec::with_capacity(frames)];
    let mut errors = 0;
    let mut maximum_active = 0;
    let state_path = output.join(format!("{name}-poly-state-events.jsonl"));
    let mut resets = BTreeMap::<usize, Vec<(usize, i16, bool)>>::new();
    if state_path.exists() {
        for line in fs::read_to_string(state_path)?.lines() {
            let event: serde_json::Value = serde_json::from_str(line)?;
            let frame = event["frame"].as_u64().ok_or("Missing state event frame")? as usize;
            let slot = event["slot"].as_u64().ok_or("Missing state event slot")? as usize
                + 12 * event["processor"]
                    .as_u64()
                    .ok_or("Missing state event processor")? as usize;
            let value = event["value"].as_u64().ok_or("Missing state event value")? as i16;
            if slot >= 24 {
                return Err("Invalid state event slot".into());
            }
            let filter = match event["kind"].as_str() {
                Some("amplifier_reset") => false,
                Some("filter_reset") if value == 0 => true,
                _ => return Err("Unqualified state-reset event".into()),
            };
            resets.entry(frame).or_default().push((slot, value, filter));
        }
    }
    for frame in 0..frames {
        for (program, (slot, plan)) in plans.iter().enumerate() {
            if frame as u64 == plan.reference_start_frame {
                pool.install(
                    *slot,
                    0,
                    ActiveVoice {
                        modulation: None,
                        auxiliary: None,
                        pan: None,
                        mixer: None,
                        secondary: None,
                        primary: None,
                        shaper: None,
                        comb_program: None,
                        renderer: VoiceRenderer::new(plan.initial, plan.parameters),
                        amplifier: None,
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
                // The source controller retires this voice at this boundary.
                // Lifetimes here qualify the graph; MIDI scheduling is separate.
                pool.remove(*slot);
            }
        }
        maximum_active = maximum_active.max(pool.active_count());
        if let Some(events) = resets.get(&frame) {
            for &(slot, value, filter) in events {
                if filter {
                    pool.reset_filter(slot);
                } else {
                    pool.reset_amplifier(slot, value);
                }
            }
        }
        let buses = pool.next_buses(&table, None, |p| &plans[p].1.events);
        for (processor, original) in originals.iter().enumerate() {
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
                let expected =
                    word(&original[44 + 32 * frame..44 + 32 * (frame + 1)], channel) as i32;
                if sample.0 != expected {
                    if errors < 5 {
                        eprintln!(
                            "Processor {processor}, frame {frame}, channel {channel}: {} != {expected}",
                            sample.0
                        );
                    }
                    errors += 1;
                }
            }
            audio[processor].push(actual);
        }
    }
    wav::write_buses(
        &output.join(format!("{name}-rust-polyphonic-mix.wav")),
        &audio[0],
    )?;
    if dual {
        wav::write_buses(
            &output.join(format!("{name}-rust-polyphonic-slave.wav")),
            &audio[1],
        )?;
    }
    let report = serde_json::json!({"name":name,"original_graphs":plans.len(),"maximum_simultaneous":maximum_active,
        "complete_wav_frames":lengths,"different_samples":errors,"passed":errors==0,"original_controller_schedule":true,
        "recorded_audio_used_by_renderer":false,"complete_instrument_parity":false});
    fs::write(
        output.join(format!("{name}-polyphony-parity.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Native polyphonic complete WAV differs".into());
    }
    Ok(())
}
