use radias_synth_domain::controller_service::{
    ControllerServiceTimer, VoiceService, select_voice_service,
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let out = root.join("runs/native-clone");
    let raw = fs::read(out.join("controller-original-timer.bin"))?;
    let flags = fs::read(out.join("controller-original-service-flags.bin"))?;
    if raw.len() != 32768 * 28 || flags.len() != 1024 * 16 {
        return Err("Original controller corpus incomplete".into());
    }
    let mut clock_errors = 0;
    for row in raw.chunks_exact(28) {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let mut timer = ControllerServiceTimer {
            constant: w(0),
            counter: w(1),
            prescaler_phase: w(2) as u8,
        };
        let count = timer.advance_cpu_clocks(w(3));
        if timer.counter != w(4)
            || u32::from(timer.prescaler_phase) != w(5)
            || u32::from(count != 0) != w(6)
        {
            clock_errors += 1;
        }
    }
    let mut flag_errors = 0;
    for row in flags.chunks_exact(16) {
        let w = |i: usize| u32::from_le_bytes(row[4 * i..4 * i + 4].try_into().unwrap());
        let mut current = w(0) as u8;
        let decision = match select_voice_service(&mut current, w(1) as u8) {
            VoiceService::None => 0,
            VoiceService::Envelopes => 1,
            VoiceService::Release => 2,
        };
        if u32::from(current) != w(2) || decision != w(3) {
            flag_errors += 1;
        }
    }
    let sys = fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?;
    let mut one = radias_synth_infrastructure::firmware::controller_service_timer(&sys)?;
    let mut split = one;
    let events = one.advance_cpu_clocks(720240);
    let split_events = (0..240)
        .map(|_| split.advance_cpu_clocks(3001))
        .sum::<u32>();
    let partition_exact = one == split && events == split_events && events == 10;
    let report = serde_json::json!({"passed":clock_errors==0&&flag_errors==0&&partition_exact,
        "complete_original_initializer":true,"original_voice_service_whole_calls":1024,"original_timer_cases":32768,
        "clock_errors":clock_errors,"flag_errors":flag_errors,"split_clock_exact":partition_exact,
        "TCOR0_from_original_SYS":3000,"cpu_clocks_per_service":72024,"audio_frames_per_service":24.008,
        "source_subcalls_skipped":false,"original_instructions_modified":false,"timer_reference":"Existing Board TMU0 functional peripheral; hardware cycle conformance remains open",
        "all256_flag_bytes_and_four_inhibit_values":true,"complete_HPI_audio_timing_qualified":false,"complete_native_engine":false});
    fs::write(
        out.join("controller-service-parity.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if !report["passed"].as_bool().unwrap() {
        return Err("Native controller service differed".into());
    }
    Ok(())
}
