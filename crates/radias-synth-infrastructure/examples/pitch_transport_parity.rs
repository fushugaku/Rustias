use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, SynthesisParameterTransport,
};
use radias_synth_domain::amplifier_delivery::AmplifierPacket;
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u16 {
    let v = u16::from_le_bytes(raw[*cursor..*cursor + 2].try_into().unwrap());
    *cursor += 2;
    v
}
fn long(words: &[u16], i: usize) -> u32 {
    (u32::from(words[i]) << 16) | u32::from(words[i + 1])
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Published {
    Amp(usize, i16),
    Pitch(usize, Option<(u16, u32)>, u32, i16, i16, Option<i16>),
    Detune(usize, i16),
}
fn published(slot: usize, opcode: u16, words: &[u16]) -> Published {
    if opcode == 27 {
        return Published::Detune(slot, words[4] as i16);
    }
    Published::Pitch(
        slot,
        (opcode != 17).then_some((words[0], long(words, 2))),
        long(words, 36),
        words[42] as i16,
        words[43] as i16,
        match opcode {
            29 => Some(words[8] as i16),
            30 => Some(words[9] as i16),
            _ => None,
        },
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let dispatch = firmware::primary_pitch_sender_table(&fs::read(
        root.join("firmware/RADIAS_SYS_0200.bin"),
    )?)?;
    let rom = ["master", "slave"].map(|chip| {
        let raw = fs::read(root.join(format!("firmware/dsp-{chip}-host-stream.bin"))).unwrap();
        MasterTables::from_host_stream(&raw)
            .unwrap()
            .pitch_receiver_rom()
            .unwrap()
    });
    let (mut calls, mut entries, mut errors) = (0, 0, 0);
    let mut first_error = None;
    let mut snapshot = None;
    for family in ["pitch", "oscillator", "oscillator-extended"] {
        let raw = fs::read(out.join(format!("dsp-original-{family}-receiver.bin")))?;
        let mut cursor = 0;
        while cursor < raw.len() {
            let chip = usize::from(take(&raw, &mut cursor));
            let opcode = take(&raw, &mut cursor);
            let _norm = [take(&raw, &mut cursor), take(&raw, &mut cursor)];
            let before = (0..896)
                .map(|_| take(&raw, &mut cursor))
                .collect::<Vec<_>>();
            let after = (0..896)
                .map(|_| take(&raw, &mut cursor))
                .collect::<Vec<_>>();
            let _ready = [take(&raw, &mut cursor), take(&raw, &mut cursor)];
            let count = take(&raw, &mut cursor);
            let writes = (0..count)
                .map(|_| (take(&raw, &mut cursor), take(&raw, &mut cursor)))
                .collect::<Vec<_>>();
            if opcode == 23 {
                continue;
            }
            let count = usize::from(before[2]) + 1;
            let mut state = before.clone();
            let mut boundaries = Vec::new();
            let terminal = match opcode {
                16 | 17 | 24 | 25 => 45,
                26 => 34,
                27 => 19,
                29 => 10,
                30 => 11,
                _ => return Err("Unexpected source pitch opcode".into()),
            };
            for (address, value) in writes {
                if (0x2000..0x2300).contains(&address) {
                    state[128 + usize::from(address - 0x2000)] = value;
                    if (address - 0x2000) % 160 == terminal {
                        let local = usize::from(address - 0x2000) / 160;
                        let start = 128 + 160 * local + 2;
                        boundaries.push(published(
                            local + 12 * chip,
                            opcode,
                            &state[start..start + 44],
                        ));
                    }
                }
            }
            if boundaries.len() != count {
                return Err("Source pitch publication boundaries differ".into());
            }
            if opcode == 16 && count == 1 && snapshot.is_none() {
                snapshot = Some((chip, before.clone(), boundaries[0].clone()));
            }
            for chunk in [1, 31, 3000] {
                let mut queue = SynthesisParameterTransport::default();
                queue.configure_pitch_receivers(rom.clone(), dispatch);
                for local in 0..5 {
                    let start = 128 + 160 * local + 2;
                    queue.restore_pitch(local + 12 * chip, before[start..start + 44].try_into()?);
                }
                let mut expected = Vec::new();
                for (i, p) in boundaries.iter().enumerate() {
                    let address = before[3 + 2 * i];
                    let slot = usize::from(address - 0x2000) / 160 + 12 * chip;
                    let value = before[4 + 2 * i];
                    let amp = (calls as i16).wrapping_add(i as i16);
                    queue
                        .enqueue(0, slot, AmplifierPacket::Target(amp))
                        .map_err(|e| format!("{e:?}"))?;
                    let request = match opcode {
                        17 => queue.secondary_pitch(0, slot, value as i16),
                        27 => queue.unison_detune(0, slot, value as i16),
                        _ => queue.primary_pitch(
                            0,
                            slot,
                            match opcode {
                                16 => 0,
                                24 => 2,
                                25 => 3,
                                26 => 32,
                                29 => 4,
                                30 => 5,
                                _ => unreachable!(),
                            },
                            value,
                        ),
                    };
                    request.map_err(|e| format!("{e:?}"))?;
                    expected.push(Published::Amp(slot, amp));
                    expected.push(p.clone());
                }
                let mut actual = Vec::new();
                let end = count as u64 * 212;
                let mut clock = 0;
                while clock < end {
                    clock = (clock + chunk).min(end);
                    queue.advance_until(clock, |_, slot, packet| match packet {
                        DeliveredSynthesisParameter::Amplifier(AmplifierPacket::Target(v)) => {
                            actual.push(Published::Amp(slot, v))
                        }
                        DeliveredSynthesisParameter::Pitch {
                            primary,
                            secondary,
                            noise_pitch,
                        } => actual.push(Published::Pitch(
                            slot,
                            primary.map(|(code, increment, _)| (code, increment.0)),
                            secondary.increment.0,
                            secondary.edge,
                            secondary.bandwidth,
                            noise_pitch,
                        )),
                        DeliveredSynthesisParameter::UnisonDetune(v) => {
                            actual.push(Published::Detune(slot, v))
                        }
                        _ => {}
                    });
                }
                let mut equal = actual == expected && queue.pending() == 0;
                for local in 0..5 {
                    let start = 128 + 160 * local + 2;
                    equal &= queue.pitch_state(local + 12 * chip).as_slice()
                        == &after[start..start + 44];
                }
                if !equal {
                    errors += 1;
                    first_error.get_or_insert(serde_json::json!({"case":calls,"opcode":opcode,"chunk":chunk,"native":format!("{actual:?}"),"source":format!("{expected:?}")}));
                }
            }
            calls += 1;
            entries += count;
        }
    }
    let (chip, before, expected) = snapshot.ok_or("Pitch reset fixture missing")?;
    let local = usize::from(before[3] - 0x2000) / 160;
    let slot = local + 12 * chip;
    let start = 128 + 160 * local + 2;
    let mut queue = SynthesisParameterTransport::default();
    queue.configure_pitch_receivers(rom, dispatch);
    queue.restore_pitch(slot, before[start..start + 44].try_into()?);
    queue
        .primary_pitch(0, slot, 0, before[4])
        .map_err(|e| format!("{e:?}"))?;
    queue.reset_pitch(slot, 8192, -768, true, 12000);
    let mut published = None;
    queue.advance_until(106, |_, slot, p| {
        if let DeliveredSynthesisParameter::Pitch {
            primary,
            secondary,
            noise_pitch,
        } = p
        {
            published = Some(Published::Pitch(
                slot,
                primary.map(|(code, increment, _)| (code, increment.0)),
                secondary.increment.0,
                secondary.edge,
                secondary.bandwidth,
                noise_pitch,
            ));
        }
    });
    let prior_context = published == Some(expected);
    if !prior_context {
        errors += 1;
    }
    let report = serde_json::json!({"passed":errors==0,"whole_original_receiver_calls":calls,"original_parameter_publications":entries,"errors":errors,"first_error":first_error,
  "both_processors":true,"CPU_clock_partitions":[1,31,3000],"production_SynthesisParameterTransport_used":true,"AMP_and_pitch_share_FIFO":true,
  "full44_word_pitch_banks_match_source":errors==0,"earlier_queued_pitch_bank_retained_across_later_reset":prior_context,
  "source_prior_bank_values_and_ROM_are_declared_initial_inputs":true,"source_coefficient_outputs_used_only_for_comparison":true,
  "PCM_opcode23_excluded":true,"receiver_job_DMA_timing_and_whole_instrument_audio_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("pitch-transport-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Production pitch delivery differs".into());
    }
    Ok(())
}
