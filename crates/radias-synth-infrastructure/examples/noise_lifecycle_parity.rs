//! Physical frame state across original note retirement, reuse and stealing.
use radias_synth_application::{
    VoiceRenderer,
    polyphony::{ActiveVoice, PolyphonicRenderer},
};
use radias_synth_domain::{
    Sample,
    noise::{FormantState, NoiseFrameSeeds},
};
use radias_synth_infrastructure::{
    firmware::{self, MasterTables},
    prepared::PreparedVoice,
    wav,
};
use std::{collections::BTreeMap, fs, path::PathBuf};

fn w(row: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(row[index * 4..index * 4 + 4].try_into().unwrap())
}
struct Segment {
    slot: usize,
    raw: Vec<u8>,
    noise: Vec<u8>,
    plan: PreparedVoice,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let image = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&image)?.waveform()?;
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let counters = firmware::formant_counter_seeds(&system)?;
    for name in args {
        let raw = fs::read(out.join(format!("{name}-poly-voice-inputs.bin")))?;
        let slots = fs::read(out.join(format!("{name}-poly-voice-slots.bin")))?;
        let noise = fs::read(out.join(format!("{name}-poly-noise-inputs.bin")))?;
        if raw.len() % 704 != 0
            || slots.len() != raw.len() / 704 * 16
            || noise.len() != raw.len() / 704 * 20
        {
            return Err("Incomplete physical voice capture".into());
        }
        let events: Vec<serde_json::Value> =
            fs::read_to_string(out.join(format!("{name}-noise-lifecycle-events.jsonl")))?
                .lines()
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?;
        let mut resets = BTreeMap::<usize, Vec<u64>>::new();
        let mut gain_commands = BTreeMap::<usize, Vec<(usize, i16)>>::new();
        let gain_path = out.join(format!("{name}-stereo-cache-gain-events.jsonl"));
        if gain_path.exists() {
            for line in fs::read_to_string(gain_path)?.lines() {
                let event: serde_json::Value = serde_json::from_str(line)?;
                if event["pc"].as_u64() != Some(0xe23a) {
                    continue;
                }
                let slot = event["processor"].as_u64().unwrap() as usize * 12
                    + event["slot"].as_u64().unwrap() as usize;
                let delta = event["input_delta"]
                    .as_u64()
                    .ok_or("Accepted stereo cache delta absent")?
                    as i16;
                let mut cache = radias_synth_domain::stereo_cache::StereoCache::default();
                cache.set_decay(delta);
                if event["value"].as_u64() != Some(cache.gain as u16 as u64) {
                    return Err("Native stereo cache gain differs".into());
                }
                gain_commands
                    .entry(event["frame"].as_u64().unwrap() as usize)
                    .or_default()
                    .push((slot, delta));
            }
        }
        for event in &events {
            let slot = event["processor"]
                .as_u64()
                .ok_or("Reset processor absent")? as usize
                * 12
                + event["slot"].as_u64().ok_or("Reset slot absent")? as usize;
            if event["kind"] == "parameter_reset" {
                resets
                    .entry(slot)
                    .or_default()
                    .push(event["frame"].as_u64().ok_or("Reset frame absent")?);
            }
            if event["kind"] == "formant_counter_reset"
                && event["expected_value"].as_u64()
                    != Some(counters.for_slot(slot as u8) as u16 as u64)
            {
                return Err("Formant counter controller slot mapping differs".into());
            }
        }
        let mut grouped = BTreeMap::<usize, Vec<(Vec<u8>, Vec<u8>)>>::new();
        for ((row, slot), input) in raw
            .chunks_exact(704)
            .zip(slots.chunks_exact(16))
            .zip(noise.chunks_exact(20))
        {
            let local = (w(slot, 2) - 0x2000) / 160;
            let id = w(slot, 1) as usize * 12 + local as usize;
            if local >= 12
                || id >= 24
                || w(row, 0) != w(slot, 0)
                || w(row, 0) != w(input, 0)
                || w(input, 4) != 0
            {
                return Err("Unqualified physical slot, clock or excitation input".into());
            }
            grouped
                .entry(id)
                .or_default()
                .push((row.to_vec(), input.to_vec()));
        }
        let mut segments = Vec::<Segment>::new();
        for (&slot, rows) in &grouped {
            let mut current = Vec::new();
            let mut inputs = Vec::new();
            let mut previous = None;
            for (row, input) in rows {
                let frame = w(row, 0) as u64;
                let new = previous.is_some_and(|p| {
                    frame != p + 1
                        || resets
                            .get(&slot)
                            .is_some_and(|list| list.iter().any(|&r| r > p && r <= frame))
                });
                if new {
                    let plan = PreparedVoice::from_reference_va_parameters(&current)?;
                    segments.push(Segment {
                        slot,
                        raw: core::mem::take(&mut current),
                        noise: core::mem::take(&mut inputs),
                        plan,
                    });
                }
                current.extend_from_slice(row);
                inputs.extend_from_slice(input);
                previous = Some(frame);
            }
            if !current.is_empty() {
                let plan = PreparedVoice::from_reference_va_parameters(&current)?;
                segments.push(Segment {
                    slot,
                    raw: current,
                    noise: inputs,
                    plan,
                });
            }
        }
        let seeds: [NoiseFrameSeeds; 2] = [0, 1].map(|chip| {
            let suffix = if chip == 0 { "" } else { "-slave" };
            let boot: serde_json::Value = serde_json::from_slice(
                &fs::read(out.join(format!("{name}{suffix}-noise-boot-inputs.json"))).unwrap(),
            )
            .unwrap();
            NoiseFrameSeeds::from_inputs(
                boot["input602"].as_u64().unwrap() as i16,
                boot["input603"].as_u64().unwrap() as i16,
            )
        });
        let originals = [
            fs::read(out.join(format!("{name}-original-complete-mix.wav")))?,
            fs::read(out.join(format!("{name}-original-slave-mix.wav")))?,
        ];
        let lengths = originals.each_ref().map(|o| (o.len() - 44) / 32);
        let frames = *lengths.iter().max().unwrap();
        let mut pool = PolyphonicRenderer::default();
        pool.initialize_physical_frames(seeds);
        let mut active = [None; 24];
        let mut audio: [Vec<[Sample; 8]>; 2] = core::array::from_fn(|_| Vec::with_capacity(frames));
        let mut state_errors = [0usize; 5];
        let mut audio_errors = [0usize; 2];
        let mut maximum_active = 0;
        for frame in 0..frames {
            if let Some(commands) = gain_commands.get(&frame) {
                for &(slot, delta) in commands {
                    pool.set_stereo_cache_decay(slot, delta);
                }
            }
            for (slot, current) in active.iter_mut().enumerate() {
                if let Some(program) = *current {
                    let segment: &Segment = &segments[program];
                    if frame as u64
                        == segment.plan.reference_start_frame
                            + segment.plan.reference_voice_frames as u64
                    {
                        pool.remove(slot);
                        *current = None;
                    }
                }
            }
            for (program, segment) in segments
                .iter()
                .enumerate()
                .filter(|(_, s)| s.plan.reference_start_frame == frame as u64)
            {
                let mut initial = segment.plan.initial;
                initial.primary.noise = Default::default();
                initial.primary.formant = FormantState {
                    counter: counters.for_slot(segment.slot as u8),
                    filter: Default::default(),
                };
                initial.filter.state = Default::default();
                initial.second_filter.state = Default::default();
                initial.envelope.0 = 0;
                pool.install(
                    segment.slot,
                    0,
                    ActiveVoice {
                        renderer: VoiceRenderer::new(initial, segment.plan.parameters),
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
                        bus: segment.plan.bus,
                    },
                );
                active[segment.slot] = Some(program);
            }
            maximum_active = maximum_active.max(pool.active_count());
            for (slot, program) in active
                .iter()
                .enumerate()
                .filter_map(|(slot, p)| p.map(|p| (slot, p)))
            {
                let segment = &segments[program];
                let index = (frame as u64 - segment.plan.reference_start_frame) as usize;
                let row = &segment.raw[index * 704..index * 704 + 704];
                let input = &segment.noise[index * 20..index * 20 + 20];
                let actor = &pool.active_voice(slot).unwrap().renderer.voice;
                let actual = [
                    actor.primary.phase.0,
                    actor.mixer_noise.state as u32,
                    actor.primary.formant.counter as u16 as u32,
                    actor.primary.noise.first as u32,
                    actor.primary.noise.second as u32,
                ];
                let expected = [
                    w(row, 161),
                    w(input, 3),
                    w(row, 11),
                    w(row, 15) << 16 | w(row, 16),
                    w(row, 17) << 16 | w(row, 18),
                ];
                for field in 0..5 {
                    if (field == 2 && w(row, 2) != 0xc1a8) || (field >= 3 && w(row, 2) != 0xc0a8) {
                        continue;
                    }
                    if actual[field] != expected[field] {
                        if state_errors[field] < 3 {
                            eprintln!(
                                "{name} slot{slot} frame{frame} field{field}:{} vs{}",
                                actual[field], expected[field]
                            );
                        }
                        state_errors[field] += 1;
                    }
                }
            }
            let buses = pool.next_buses(&table, None, |program| &segments[program].plan.events);
            for chip in 0..2 {
                if frame >= lengths[chip] {
                    continue;
                }
                let actual = core::array::from_fn(|channel| {
                    if channel % 2 == 0 {
                        buses[chip][channel / 2].left
                    } else {
                        buses[chip][channel / 2].right
                    }
                });
                let expected = &originals[chip][44 + frame * 32..44 + (frame + 1) * 32];
                for (channel, sample) in actual.iter().enumerate() {
                    if sample.0 != w(expected, channel) as i32 {
                        if audio_errors[chip] < 3 {
                            eprintln!(
                                "{name} DSP{chip} frame{frame} channel{channel}:{} vs{}",
                                sample.0,
                                w(expected, channel) as i32
                            );
                        }
                        audio_errors[chip] += 1;
                    }
                }
                audio[chip].push(actual);
            }
        }
        for (chip, track) in audio.iter().enumerate() {
            wav::write_buses(
                &out.join(format!("{name}-rust-lifecycle-{chip}.wav")),
                track,
            )?;
        }
        let passed = state_errors == [0; 5] && audio_errors == [0; 2];
        let report = serde_json::json!({"name":name,"passed":passed,"physical_slots":grouped.len(),"note_segments":segments.len(),"maximum_active":maximum_active,
            "state_errors":state_errors,"audio_errors":audio_errors,"frames":lengths,"native_boot_states_and_physical_frame_retention_used":true,
            "original_recorded_phases_or_noise_seeds_used_to_render":false,"original_other_compiled_parameters_and_event_times_used":true,
            "native_controller_hpi_timing_qualified":false,"complete_native_engine":false});
        fs::write(
            out.join(format!("{name}-lifecycle-parity.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        if !passed {
            return Err("Native physical lifecycle mismatch".into());
        }
    }
    Ok(())
}
