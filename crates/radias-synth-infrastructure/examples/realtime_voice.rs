#[cfg(feature = "desktop-io")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use radias_synth_application::amplifier::ControllerTables;
    use radias_synth_infrastructure::{
        audio::NativePlayer,
        firmware::{MasterTables, amplifier_tables, envelope_curves, envelope_timing_tables},
        prepared::PreparedVoice,
    };
    use std::{
        fs,
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or("..".into()));
    let raw = fs::read(root.join("runs/native-clone/native-voice-voice-inputs.bin"))?;
    let plan = PreparedVoice::from_reference_parameters(&raw)?;
    let source = fs::read(root.join("firmware/dsp-master-host-stream.bin"))?;
    let table = MasterTables::from_host_stream(&source)?.waveform()?;
    let mut plans = vec![plan];
    for name in ["live-va-pulse", "live-va-triangle", "live-va-sine"] {
        let raw = fs::read(root.join(format!("runs/native-clone/{name}-voice-va-inputs.bin")))?;
        plans.push(PreparedVoice::from_reference_va_parameters(&raw)?);
    }
    let system = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let player = NativePlayer::with_controller(
        plans,
        table,
        None,
        Some(ControllerTables {
            curves: envelope_curves(&system)?,
            timing: envelope_timing_tables(&system)?,
            amplifier: amplifier_tables(&system)?,
        }),
    )?;
    player.gain(0.2);
    player.adsr([5, 10, 80, 10])?;
    let mut modes = Vec::new();
    for (index, label) in ["Saw", "Pulse", "Triangle", "Sine"].iter().enumerate() {
        let before = player.status().audible_frames;
        player.waveform(index)?;
        player.note(60, true)?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(100));
        }
        let status = player.status();
        modes.push(serde_json::json!({"mode":label,"audible_frames":status.audible_frames-before,"deadline_misses":status.deadline_misses}));
        if status.audible_frames == before {
            return Err("Native waveform silent on device".into());
        }
    }
    player.note(60, false)?;
    thread::sleep(Duration::from_millis(300));
    let released = player.status().audible_frames;
    thread::sleep(Duration::from_millis(150));
    let release_silent = player.status().audible_frames == released;
    player.stop()?;
    thread::sleep(Duration::from_millis(100));
    let status = player.status();
    let report = serde_json::json!({"device":status.device,"sample_rate":status.sample_rate,"callbacks":status.callbacks,
        "native_frames":status.native_frames,"audible_frames":status.audible_frames,"worst_render_ms":status.worst_render_ns as f64/1e6,
        "deadline_misses":status.deadline_misses,"device_failed":status.failed,"cpu_emulation_in_callback":false,"native_voice_graph":true,"native_adsr":true,"release_completed":release_silent,"modes":modes});
    println!("{report}");
    fs::write(
        root.join("runs/native-clone/realtime-device.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if status.audible_frames == 0 || status.failed || status.deadline_misses != 0 || !release_silent
    {
        return Err("Native device realtime check failed".into());
    }
    Ok(())
}
#[cfg(not(feature = "desktop-io"))]
fn main() {
    eprintln!("Enable --features desktop-io for the native device adapter");
}
