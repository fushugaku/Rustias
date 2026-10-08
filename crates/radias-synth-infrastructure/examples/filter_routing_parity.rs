use radias_synth_domain::{
    Sample,
    filter::{FilterCoefficients, FilterState, ResonantFilter},
    filter_routing::{DualFilterGraph, Filter2, Filter2Coefficients, Filter2State, FilterRouting},
    mixer::OscillatorMix,
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/native-clone/filter-routing.bin"))?;
    if raw.len() != 98304 * 876 {
        return Err("Incomplete original filter routing corpus".into());
    }
    let mut errors = [[0usize; 7]; 3];
    for (i, r) in raw.chunks_exact(876).enumerate() {
        let word = |n: usize| u32::from_le_bytes(r[n * 4..n * 4 + 4].try_into().unwrap());
        let p = |n: usize| word(n + 1) as i16;
        let q = |n: usize| ((word(n + 1) << 16) | word(n + 2)) as i32;
        let f = |n: usize| word(n + 161) as u16;
        let frame_pair = |n: usize| ((u32::from(f(n)) << 16) | u32::from(f(n + 1))) as i32;
        let mode = word(0) as usize;
        if mode != i / 32768 {
            return Err("Route corpus order differs".into());
        }
        let mut graph = DualFilterGraph {
            first: ResonantFilter {
                state: FilterState {
                    first: q(136),
                    second: q(138),
                    post: [q(140), q(142)],
                },
            },
            second: Filter2 {
                state: Filter2State {
                    first: q(144),
                    second: q(146),
                },
            },
        };
        let c1 = FilterCoefficients {
            input_gain: p(55),
            feedback: q(58),
            integrator_gain: q(66),
            post_gain: p(69),
            post_feedback: p(71),
            mix: [p(73), p(75), p(77), p(79), p(81)],
        };
        let c2 = Filter2Coefficients {
            input_gain: p(95),
            feedback: q(98),
            integrator_gain: q(106),
            output: match p(112) as u16 {
                0x4022 => radias_synth_domain::filter_routing::Filter2Output::LowPass,
                0x401e => radias_synth_domain::filter_routing::Filter2Output::HighPass,
                0x4020 => radias_synth_domain::filter_routing::Filter2Output::BandPass,
                _ => return Err("Unsupported original Filter2 selector".into()),
            },
        };
        let mix = OscillatorMix {
            primary_gain: p(48),
            secondary_gain: p(50),
            noise_gain: p(52),
        };
        let output = graph.sample(
            [
                FilterRouting::Serial,
                FilterRouting::Parallel,
                FilterRouting::Individual,
            ][mode],
            mix,
            c1,
            c2,
            (
                Sample(frame_pair(24)),
                Sample(frame_pair(26)),
                f(28) as i16,
                f(30) as i16,
            ),
        );
        let actual = [
            output.0,
            graph.first.state.first,
            graph.first.state.second,
            graph.first.state.post[0],
            graph.first.state.post[1],
            graph.second.state.first,
            graph.second.state.second,
        ];
        let expected = [
            word(210) as i32,
            word(211) as i32,
            word(212) as i32,
            word(213) as i32,
            word(214) as i32,
            word(215) as i32,
            word(216) as i32,
        ];
        for n in 0..7 {
            if actual[n] != expected[n] {
                if errors[mode][n] < 2 {
                    eprintln!(
                        "Route{mode} case{i} stage{n}:{} vs {}",
                        actual[n], expected[n]
                    );
                }
                errors[mode][n] += 1;
            }
        }
    }
    let passed = errors.iter().flatten().all(|&n| n == 0);
    let report = serde_json::json!({"passed":passed,"original_transition_cases":98304,"route_count":3,"filter2_output_count":3,"errors":errors,"source_range":"Master B5A0..BE77, disabled waveshaper; Filter2 LPF/HPF/BPF","source_globals_and_typed_context_used":true,"actual_graph_entry_coefficient_and_sample_addresses_verified":true,"complete_native_engine":false});
    fs::write(
        root.join("runs/native-clone/filter-routing-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !passed {
        return Err("Native filter routing mismatch".into());
    }
    Ok(())
}
