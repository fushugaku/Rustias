//! Original-firmware throughput/profile probe, separate from the audio device.
use radias_application::{Machine, PcmPolicy, Program, StepObserver};
use radias_domain::{backup, board::Board, controller::Sh3};
use radias_infrastructure::{
    capture::{self, Captures},
    pcm::NativePcmBank,
    wav,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

struct Profile(Arc<Mutex<Vec<u64>>>);
impl StepObserver for Profile {
    fn before_step(&mut self, cpu: &Sh3, _board: &mut Board) -> Result<(), String> {
        let offset = cpu.pc.wrapping_sub(0x0c000000);
        if offset < 0xe0000 {
            self.0.lock().unwrap()[offset as usize / 2] += 1;
        }
        Ok(())
    }
}
fn main() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = root.join("runs/realtime");
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let profiled = args.iter().any(|a| a == "--profile");
    let backup =
        std::fs::read(root.join("firmware/Radias-backup.rdl")).map_err(|e| e.to_string())?;
    let firmware =
        std::fs::read(root.join("firmware/RADIAS_SYS_0200.bin")).map_err(|e| e.to_string())?;
    let records = backup::records(&backup, b"316P", b"316p")?;
    let program = Program::from_bytes(&records[0][..1790])?;
    let channel = backup::global(&backup)?[6] & 15;
    let mut machine = Machine::new(firmware, Some(&backup), PcmPolicy::NativeFlash)?;
    let pcm = NativePcmBank::parse(
        std::fs::read(root.join("runs/alternative-pcm/alternative-pcm.bin"))
            .map_err(|e| e.to_string())?,
    )?;
    pcm.mount(&mut machine.board.flash)?;
    machine.run_steps(60_000_000, None);
    machine.midi(&program.sysex(channel)?);
    machine.run_steps(10_000_000, None);
    if !machine.fault.is_empty() {
        return Err(machine.fault);
    }
    let capture = Arc::new(Mutex::new(Captures {
        recording: true,
        mix_enabled: true,
        ..Default::default()
    }));
    capture::attach(&mut machine.board, capture.clone());
    let counts = Arc::new(Mutex::new(vec![0u64; 0x70000]));
    if profiled {
        machine.step_observer = Some(Box::new(Profile(counts.clone())));
    }
    for (ch, note) in program.keyboard_routes(60, channel) {
        machine.midi(&[0x90 | ch, note, 100]);
    }
    let start = Instant::now();
    let first_steps = machine.cpu.steps;
    let first_frames = machine.board.audio_frames;
    while capture.lock().unwrap().mix[0].len() < 12_000 && machine.fault.is_empty() {
        machine.run_steps(200_000, None);
    }
    let seconds = start.elapsed().as_secs_f64();
    if !machine.fault.is_empty() {
        return Err(machine.fault);
    }
    let captured = capture.lock().unwrap();
    let frames = captured.mix[0].len();
    let stem = if profiled { "profile" } else { "baseline" };
    wav::write_pcm32(&output.join(format!("{stem}-master.wav")), &captured.mix[0])?;
    wav::write_pcm32(&output.join(format!("{stem}-slave.wav")), &captured.mix[1])?;
    let mut hot = counts
        .lock()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, c)| **c != 0)
        .map(|(pc, &count)| (count, pc))
        .collect::<Vec<_>>();
    hot.sort_unstable_by(|a, b| b.cmp(a));
    let report = serde_json::json!({"profiled":profiled,"native_sample_rate":48000,"native_frames":frames,
        "wall_seconds":seconds,"speed":frames as f64/48000.0/seconds,
        "controller_steps":machine.cpu.steps-first_steps,"board_frames":machine.board.audio_frames-first_frames,
        "fault":machine.fault,"top_pc":hot.iter().take(40).map(|&(count,pc)|serde_json::json!({"pc":format!("{:08x}",0x0c000000+pc*2),"count":count})).collect::<Vec<_>>(),
        "full_engine_complete":false,"realtime_complete":false});
    std::fs::write(
        output.join(format!("{stem}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    println!("{report}");
    Ok(())
}
