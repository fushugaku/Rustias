//! Configuration adapter; audio-rate motion itself belongs to each Rust voice.
use radias_synth_application::{
    motion::{Lane as VoiceLane, Program},
    polyphony::TIMBRE_COUNT,
};
use serde::Deserialize;
use std::{collections::HashSet, sync::Arc};

#[derive(Deserialize)]
struct Config {
    version: u32,
    tracks: Vec<Vec<Lane>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lane {
    id: String,
    enabled: bool,
    target: Option<Target>,
    amount: f64,
    length: usize,
    resolution: String,
    motion: String,
    direction: String,
    run_mode: String,
    values: Vec<f64>,
    #[serde(default)]
    playback: Option<String>,
    #[serde(default)]
    loop_start: Option<usize>,
    #[serde(default)]
    loop_end: Option<usize>,
    #[serde(default)]
    durations: Option<Vec<f64>>,
    #[serde(default)]
    probabilities: Option<Vec<f64>>,
    #[serde(default)]
    curves: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct Target {
    kind: String,
    timbre: Option<usize>,
    parameter: Option<usize>,
}

pub fn compile(bytes: &[u8]) -> Option<Vec<Arc<Program>>> {
    let config: Config = serde_json::from_slice(bytes).ok()?;
    if config.version != 1 || !(4..=TIMBRE_COUNT).contains(&config.tracks.len()) {
        return None;
    }
    let mut programs = Vec::with_capacity(TIMBRE_COUNT);
    for (t, lanes) in config.tracks.into_iter().enumerate() {
        if lanes.len() > 6 {
            return None;
        }
        let mut program = Program::default();
        let mut ids = HashSet::new();
        for (index, lane) in lanes.into_iter().enumerate() {
            if !ids.insert(lane.id.clone()) || lane.id.is_empty() || lane.id.len() > 64 {
                return None;
            }
            if !matches!(
                lane.playback.as_deref(),
                None | Some("Timbre") | Some("Poly")
            ) {
                return None;
            }
            if lane.playback.as_deref() != Some("Poly") || !lane.enabled || lane.target.is_none() {
                continue;
            }
            let target = lane.target?;
            let parameter = target.parameter?;
            if target.kind != "synth"
                || target.timbre != Some(t)
                || !radias_synth_infrastructure::circuit::motion::supported(parameter)
            {
                return None;
            }
            if !(1..=128).contains(&lane.length)
                || lane.values.len() != 128
                || lane
                    .values
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 127.0)
                || !lane.amount.is_finite()
                || lane.amount.abs() > 100.0
            {
                return None;
            }
            let (n, d) = lane.resolution.split_once('/')?;
            let beats = 4.0 * n.parse::<f64>().ok()? / d.parse::<f64>().ok()?;
            if !beats.is_finite() || !(1.0 / 12.0..=4.0).contains(&beats) {
                return None;
            }
            let start = lane.loop_start.unwrap_or(0);
            let end = lane.loop_end.unwrap_or(lane.length - 1);
            if start > end || end >= lane.length {
                return None;
            }
            let durations = lane.durations.unwrap_or_else(|| vec![1.0; 128]);
            let probabilities = lane.probabilities.unwrap_or_else(|| vec![100.0; 128]);
            if durations.len() != 128
                || durations
                    .iter()
                    .any(|v| !v.is_finite() || !(0.125..=16.0).contains(v))
                || probabilities.len() != 128
                || probabilities
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=100.0).contains(v))
            {
                return None;
            }
            let curves = lane
                .curves
                .unwrap_or_else(|| vec!["Auto".into(); 128])
                .into_iter()
                .map(|s| {
                    Some(match s.as_str() {
                        "Auto" => 0,
                        "Linear" => 1,
                        "Exp" => 2,
                        "Log" => 3,
                        "Hold" => 4,
                        _ => return None,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            if curves.len() != 128 {
                return None;
            }
            program.lanes.push(VoiceLane {
                index,
                id: lane.id,
                parameter,
                amount: lane.amount,
                beats,
                start,
                end,
                direction: match lane.direction.as_str() {
                    "Forward" => 0,
                    "Reverse" => 1,
                    "Alt1" => 2,
                    "Alt2" => 3,
                    "Random" => 4,
                    _ => return None,
                },
                one_shot: match lane.run_mode.as_str() {
                    "Loop" => false,
                    "OneShot" => true,
                    _ => return None,
                },
                slide: match lane.motion.as_str() {
                    "Step" => false,
                    "Slide" => true,
                    _ => return None,
                },
                values: lane.values,
                durations,
                probabilities,
                curves,
            });
        }
        programs.push(Arc::new(program));
    }
    while programs.len() < TIMBRE_COUNT {
        programs.push(Arc::default());
    }
    Some(programs)
}
