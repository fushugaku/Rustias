//! Whole native sample-path throughput; this is a performance gate, not a
//! claim that analytic standalone controller tables match the original ROM.
use radias_synth_application::vocoder::VocoderRenderer;
use radias_synth_domain::{
    Sample, pan::StereoFrame, program::Program, vocoder_control::VocoderControlInputs,
};
use radias_synth_infrastructure::{
    standalone::StandaloneSynth, synthesizer::Command, vocoder_tables,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    fs,
    hint::black_box,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
struct MeasuredAllocator;
fn allocated(bytes: usize) {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    let current = LIVE_BYTES.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK_BYTES.fetch_max(current, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
            allocated(size);
        }
        result
    }
}
#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Repository required")?);
    let raw = fs::read(root.join("runs/vocoder-native-synth-program.bin"))?;
    let program = Program::from_bytes(&raw).map_err(|_| "Native program extent")?;
    let memory_baseline = LIVE_BYTES.load(Ordering::Relaxed);
    PEAK_BYTES.store(memory_baseline, Ordering::Relaxed);
    let mut instrument = StandaloneSynth::new();
    for timbre in 0..4 {
        if !instrument.control(timbre, 71, 1) {
            return Err("Timbre activation failed".into());
        }
        for note in 0..6 {
            instrument
                .engine
                .apply(Command::Note(timbre, 48 + timbre * 3 + note, 96));
        }
    }
    let renderer = VocoderRenderer::from_program(
        program.vocoder(),
        VocoderControlInputs::default(),
        &vocoder_tables::original(),
        vocoder_tables::interpolation(),
    )
    .map_err(|e| format!("Vocoder {e:?}"))?;
    instrument.engine.set_vocoder(Some(Box::new(renderer)));
    if instrument.engine.active_count() != 24 {
        return Err("Benchmark did not activate24 native voices".into());
    }
    let count = 480_000usize;
    let mut phase = 0u32;
    let mut check = 0i32;
    let mut nonzero = 0usize;
    let live_engine_heap_bytes = LIVE_BYTES.load(Ordering::Relaxed) - memory_baseline;
    let peak_engine_setup_heap_bytes = PEAK_BYTES.load(Ordering::Relaxed) - memory_baseline;
    let allocation_baseline = ALLOCATIONS.load(Ordering::Relaxed);
    let start = Instant::now();
    for frame in 0..count {
        phase = phase.wrapping_add(0x0258_bf25);
        let input = StereoFrame {
            left: Sample(0),
            right: Sample((phase as i32) >> 3),
        };
        let output = instrument
            .engine
            .sample_with_input(input, true)
            .map_err(|e| format!("Sample {frame}: {e:?}"))?;
        nonzero += usize::from(output != StereoFrame::default());
        check ^= output.left.0 ^ output.right.0;
    }
    let elapsed = start.elapsed().as_secs_f64();
    let render_allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocation_baseline;
    black_box(check);
    if nonzero == 0 || instrument.engine.active_count() != 24 {
        return Err("Benchmark became silent or lost held voices".into());
    }
    let speed = 10.0 / elapsed;
    let report = serde_json::json!({"passed":speed>=1.0 && render_allocations==0,"sample_frames":count,"audio_seconds":10,"wall_seconds":elapsed,"times_realtime":speed,"active_native_voices":24,"timbres":4,"vocoder_bands":16,"nonzero_frames":nonzero,"render_allocations":render_allocations,"live_engine_host_heap_payload_bytes":live_engine_heap_bytes,"peak_engine_setup_host_heap_payload_bytes":peak_engine_setup_heap_bytes,"vocoder_state_bytes":core::mem::size_of::<radias_synth_domain::vocoder::Vocoder>(),"vocoder_renderer_bytes":core::mem::size_of::<VocoderRenderer>(),"scope":"Native24-voice/four-timbre sample path plus full vocoder, parameter interpolation and stereo projection","standalone_controller_tables_are_analytic":true,"host_layout_is_not_target_memory_or_abi_proof":true,"device_MIDI_input_output_FXD03_and_original_whole_sound_parity_qualified":false});
    fs::write(
        root.join("runs/native-clone/vocoder-instrument-benchmark.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if speed < 1.0 {
        return Err("Whole sample path is slower than realtime".into());
    }
    if render_allocations != 0 {
        return Err("Sample path allocated during render".into());
    }
    Ok(())
}
