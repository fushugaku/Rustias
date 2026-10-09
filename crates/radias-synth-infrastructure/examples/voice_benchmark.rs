use radias_synth_application::VoiceRenderer;
use radias_synth_domain::pan::StereoFrame;
use radias_synth_infrastructure::{firmware::MasterTables, prepared::PreparedVoice};
use std::{fs, path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or("..".into()));
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    let plans = ["saw", "pulse", "triangle", "sine", "cross", "unison", "vpm"]
        .iter()
        .map(|name| {
            let raw = fs::read(root.join(format!("assets/native-va/{name}.json")))
                .map_err(|e| e.to_string())?;
            PreparedVoice::from_program_json(&raw)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut voices: [VoiceRenderer; 24] = core::array::from_fn(|i| {
        VoiceRenderer::new(
            plans[i % plans.len()].initial,
            plans[i % plans.len()].parameters,
        )
    });
    let mut output = [[StereoFrame::default(); 128]; 24];
    let mut timings = Vec::with_capacity(512);
    let mut nonzero = 0u64;
    let start = Instant::now();
    for _ in 0..512 {
        let block = Instant::now();
        for (i, voice) in voices.iter_mut().enumerate() {
            voice.render(&table, &plans[i % plans.len()].events, &mut output[i]);
        }
        timings.push(block.elapsed().as_nanos() as u64);
        for frames in &output {
            nonzero += frames
                .iter()
                .filter(|s| s.left.0 != 0 || s.right.0 != 0)
                .count() as u64;
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    timings.sort_unstable();
    let deadline = 128.0 / 48000.0;
    let misses = timings
        .iter()
        .filter(|&&n| n as f64 / 1e9 > deadline)
        .count();
    let report = serde_json::json!({"scope":"24 simultaneous recovered voice graphs with seven qualified primary modes; note allocation, full modulation and FXD03 excluded","voices":24,"blocks":512,"block_frames":128,"sample_rate":48000,"deadline_ms":deadline*1000.0,"p99_ms":timings[506] as f64/1e6,"worst_ms":timings[511] as f64/1e6,"deadline_misses":misses,"audible_voice_frames":nonzero,"realtime_ratio":512.0*deadline/elapsed,"cpu_emulation_in_renderer":false,"complete_engine_benchmark":false});
    println!("{report}");
    fs::write(
        root.join("runs/native-clone/voice-benchmark.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if nonzero == 0 || misses != 0 {
        return Err("Recovered voice graph realtime budget failed".into());
    }
    Ok(())
}
