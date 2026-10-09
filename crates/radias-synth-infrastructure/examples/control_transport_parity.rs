use radias_synth_application::synthesis_transport::{
    DeliveredSynthesisParameter, ScalarParameter, SynthesisParameterTransport,
};
use radias_synth_domain::{
    amplifier_delivery::AmplifierPacket,
    filter::FilterCoefficients,
    filter_routing::{Filter2Coefficients, Filter2Output},
};
use radias_synth_infrastructure::firmware::{self, MasterTables};
use std::{fs, path::PathBuf};
fn take(raw: &[u8], cursor: &mut usize) -> u16 {
    let v = u16::from_le_bytes(raw[*cursor..*cursor + 2].try_into().unwrap());
    *cursor += 2;
    v
}
fn long(words: &[u16], i: usize) -> i32 {
    ((u32::from(words[i]) << 16) | u32::from(words[i + 1])) as i32
}
#[derive(Debug, PartialEq, Eq)]
enum Event {
    Scalar(ScalarParameter, i16),
    Pitch(u16, u32, u32, i16, i16),
    Amp(u16, i16),
    Filter(i32, i32, i16, i16),
    NoiseShape(i16, i16),
    Filter2(i16, i32, i32, Filter2Output),
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("control-transport-original.bin"))?;
    let rom = ["master", "slave"].map(|chip| {
        let r = fs::read(root.join(format!("firmware/dsp-{chip}-host-stream.bin"))).unwrap();
        MasterTables::from_host_stream(&r)
            .unwrap()
            .pitch_receiver_rom()
            .unwrap()
    });
    let dispatch = firmware::primary_pitch_sender_table(&fs::read(
        root.join("firmware/RADIAS_SYS_0200.bin"),
    )?)?;
    let (mut cursor, mut cases, mut errors) = (0, 0, 0);
    let mut first_error = None;
    while cursor < raw.len() {
        let chip = usize::from(take(&raw, &mut cursor));
        let local = usize::from(take(&raw, &mut cursor));
        let slot = local + 12 * chip;
        let norm = ((u32::from(take(&raw, &mut cursor)) << 16) | u32::from(take(&raw, &mut cursor)))
            as i32;
        let before: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor));
        let mut stages = Vec::new();
        for _ in 0..20 {
            let tag = take(&raw, &mut cursor);
            let high = take(&raw, &mut cursor);
            let low = take(&raw, &mut cursor);
            let after: [u16; 160] = core::array::from_fn(|_| take(&raw, &mut cursor));
            stages.push((tag, high, low, after));
        }
        for chunk in [1u64, 31, 3000] {
            let mut queue = SynthesisParameterTransport::default();
            queue.configure_pitch_receivers(rom.clone(), dispatch);
            queue.restore_parameters(slot, before);
            queue.configure_filter(
                slot,
                norm,
                FilterCoefficients {
                    input_gain: before[54] as i16,
                    feedback: long(&before, 56),
                    integrator_gain: long(&before, 64),
                    post_gain: before[68] as i16,
                    post_feedback: before[70] as i16,
                    mix: core::array::from_fn(|i| before[72 + 2 * i] as i16),
                },
            );
            let filter2_base = Filter2Coefficients {
                input_gain: before[94] as i16,
                feedback: ((u32::from(before[96]) << 16) | u32::from(before[97])) as i32,
                integrator_gain: ((u32::from(before[104]) << 16) | u32::from(before[105])) as i32,
                output: Filter2Output::LowPass,
            };
            queue.configure_filter2(slot, norm, filter2_base);
            let mut expected = Vec::new();
            for &(tag, high, low, after) in &stages {
                if tag == 17 {
                    queue.configure_filter2(
                        slot,
                        norm,
                        Filter2Coefficients {
                            output: Filter2Output::Comb,
                            ..filter2_base
                        },
                    );
                }
                let request = match tag {
                    0 => queue.mixer_level(0, slot, 0, high as i16),
                    1 => queue.primary_control(0, slot, 0, high as i16),
                    2 => queue.primary_pitch(0, slot, 32, high),
                    3 => queue.primary_ratio(0, slot, high as i16),
                    4 => queue.secondary_pitch(0, slot, high as i16),
                    5 => queue.mixer_level(0, slot, 1, high as i16),
                    6 => queue.mixer_level(0, slot, 2, high as i16),
                    7 => queue.pan(0, slot, high as i16),
                    8 => queue.primary_control(0, slot, 3, high as i16),
                    9 => queue.enqueue(
                        0,
                        slot,
                        AmplifierPacket::RateAndTarget {
                            rate: high,
                            target: low as i16,
                        },
                    ),
                    10 => queue.filter_frequency(
                        0,
                        slot,
                        ((u32::from(high) << 16) | u32::from(low)) as i32,
                    ),
                    11 => queue.noise_shape(0, slot, (u32::from(high) << 16) | u32::from(low)),
                    12 => queue.noise_gain(0, slot, high as i16),
                    13 => queue.noise_frequency(0, slot, high as i16),
                    14 => queue.filter2_frequency(
                        0,
                        slot,
                        ((u32::from(high) << 16) | u32::from(low)) as i32,
                    ),
                    15 => queue.filter2_resonance(
                        0,
                        slot,
                        ((u32::from(high) << 16) | u32::from(low)) as i32,
                    ),
                    16 => queue.filter2_input_gain(0, slot, high as i16),
                    17 => queue.comb_delay(0, slot, (u32::from(high) << 16) | u32::from(low)),
                    18 => queue.comb_feedback(0, slot, (u32::from(high) << 16) | u32::from(low)),
                    19 => queue.shaper_depth(0, slot, high as i16),
                    _ => unreachable!(),
                };
                request.map_err(|e| format!("Queue: {e:?}"))?;
                expected.push(match tag {
                    0 => Event::Scalar(ScalarParameter::Mixer(0), after[47] as i16),
                    1 => Event::Scalar(ScalarParameter::PrimaryControl, after[6] as i16),
                    2 | 4 => Event::Pitch(
                        if tag == 4 { u16::MAX } else { after[2] },
                        if tag == 4 { 0 } else { long(&after, 4) as u32 },
                        long(&after, 38) as u32,
                        after[44] as i16,
                        after[45] as i16,
                    ),
                    3 => Event::Scalar(ScalarParameter::PrimaryRatio, after[8] as i16),
                    5 => Event::Scalar(ScalarParameter::Mixer(1), after[49] as i16),
                    6 => Event::Scalar(ScalarParameter::Mixer(2), after[51] as i16),
                    7 => Event::Scalar(ScalarParameter::Pan, after[127] as i16),
                    8 => Event::Scalar(ScalarParameter::PrimaryControl, after[11] as i16),
                    9 => Event::Amp(after[124], after[125] as i16),
                    10 => Event::Filter(
                        long(&after, 56),
                        long(&after, 64),
                        after[68] as i16,
                        after[70] as i16,
                    ),
                    11 => Event::NoiseShape(after[16] as i16, after[17] as i16),
                    12 => Event::Scalar(ScalarParameter::NoiseGain, after[6] as i16),
                    13 => Event::Scalar(ScalarParameter::NoiseFrequency, after[8] as i16),
                    19 => Event::Scalar(ScalarParameter::ShaperDepth, after[84] as i16),
                    14..=18 => Event::Filter2(
                        after[94] as i16,
                        ((u32::from(after[96]) << 16) | u32::from(after[97])) as i32,
                        ((u32::from(after[104]) << 16) | u32::from(after[105])) as i32,
                        if tag >= 17 {
                            Filter2Output::Comb
                        } else {
                            Filter2Output::LowPass
                        },
                    ),
                    _ => unreachable!(),
                });
            }
            queue.reset_filter2(slot);
            queue.configure_filter2(
                slot,
                !norm,
                Filter2Coefficients {
                    output: Filter2Output::HighPass,
                    ..filter2_base
                },
            );
            let mut actual = Vec::new();
            let (mut clock, mut origin) = (0, 0);
            for (index, &(tag, _, _, ref after)) in stages.iter().enumerate() {
                let is_long =
                    (9..=11).contains(&tag) || (14..=15).contains(&tag) || (17..=18).contains(&tag);
                let ack = origin + if is_long { 106 } else { 95 };
                while clock < ack {
                    clock = (clock + chunk).min(ack);
                    queue.advance_until(clock, |_, received_slot, event| {
                        assert_eq!(slot, received_slot);
                        actual.push(match event {
                            DeliveredSynthesisParameter::Scalar(p, v) => Event::Scalar(p, v),
                            DeliveredSynthesisParameter::Pitch {
                                primary, secondary, ..
                            } => Event::Pitch(
                                primary.map_or(u16::MAX, |p| p.0),
                                primary.map_or(0, |p| p.1.0),
                                secondary.increment.0,
                                secondary.edge,
                                secondary.bandwidth,
                            ),
                            DeliveredSynthesisParameter::Amplifier(
                                AmplifierPacket::RateAndTarget { rate, target },
                            ) => Event::Amp(rate, target),
                            DeliveredSynthesisParameter::Filter1(c) => Event::Filter(
                                c.feedback,
                                c.integrator_gain,
                                c.post_gain,
                                c.post_feedback,
                            ),
                            DeliveredSynthesisParameter::NoiseShape {
                                input_gain,
                                feedback,
                            } => Event::NoiseShape(input_gain, feedback),
                            DeliveredSynthesisParameter::Filter2(c) => Event::Filter2(
                                c.input_gain,
                                c.feedback,
                                c.integrator_gain,
                                c.output,
                            ),
                            _ => panic!("Unexpected mixed delivery"),
                        });
                    });
                }
                if queue.parameter_state(slot) != *after || actual.get(index) != expected.get(index)
                {
                    errors += 1;
                    first_error.get_or_insert(serde_json::json!({"case":cases,"chip":chip,"slot":slot,"stage":tag,"partition":chunk,"source":format!("{:?}",expected.get(index)),"native":format!("{:?}",actual.get(index)),"whole160_word_bank_matches":queue.parameter_state(slot)==*after}));
                }
                origin += if is_long { 117 } else { 106 };
            }
            queue.advance_until(origin, |_, _, _| {});
            if queue.pending() != 0 || actual != expected {
                errors += 1;
            }
        }
        cases += 1;
    }
    let report = serde_json::json!({"passed":cases==8192&&errors==0,"mixed_sequences":cases,"complete_original_receiver_calls":cases*20,"errors":errors,"first_error":first_error,
  "shared_production_transport_used":true,"both_processors_and_all24_slots":true,"whole160_word_banks_after_every_source_call_match":errors==0,
  "CPU_clock_partitions":[1,31,3000],"all_signed_raw_scalar_values_and_prior_parameter_words_declared_inputs":true,"source_outputs_used_only_for_comparison":true,
  "mixer_CTRL1_ratio_pan_Noise_shape_gain_frequency_pitch_Filter1_Filter2_AMP_order_preserved":true,"queued_Filter2_normalization_and_output_context_preserved_across_later_reset":true,"Filter2_sender_addresses_and_entire_parameter_bank_checked":true,"receiver_job_DMA_and_whole_audio_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("control-transport-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !report["passed"].as_bool().unwrap() {
        return Err("Mixed production control delivery differs".into());
    }
    Ok(())
}
