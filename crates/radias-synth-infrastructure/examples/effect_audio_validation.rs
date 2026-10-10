//! Audible reconstruction validation; NEVER an original FXD03 parity report.
use radias_synth_domain::{
    Sample,
    effect_audio::{DELAY_WORDS, EffectAudioContext, EffectAudioProcessor},
    pan::StereoFrame,
};
use radias_synth_infrastructure::effect_audio::{
    compile, default_program, definition, prepare_rack,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

struct CountingAllocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn input(frame: usize) -> StereoFrame {
    let seconds = frame as f64 / 48_000.0;
    let on = seconds < 2.0;
    let envelope = if on {
        (-((frame % 24_000) as f64) / 9000.0).exp()
    } else {
        0.0
    };
    let left = envelope
        * (0.16 * (std::f64::consts::TAU * 220.0 * seconds).sin()
            + 0.06 * (std::f64::consts::TAU * 659.25 * seconds).sin());
    let right = envelope
        * (0.13 * (std::f64::consts::TAU * 330.0 * seconds).sin()
            + 0.05 * (std::f64::consts::TAU * 987.77 * seconds).sin());
    StereoFrame {
        left: Sample((left * 2_147_483_648.0) as i32),
        right: Sample((right * 2_147_483_648.0) as i32),
    }
}
fn wav(path: &std::path::Path, frames: &[StereoFrame]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let size = (frames.len() * 8) as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + size).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&48_000u32.to_le_bytes())?;
    f.write_all(&384_000u32.to_le_bytes())?;
    f.write_all(&8u16.to_le_bytes())?;
    f.write_all(&32u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&size.to_le_bytes())?;
    for frame in frames {
        f.write_all(&frame.left.0.to_le_bytes())?;
        f.write_all(&frame.right.0.to_le_bytes())?;
    }
    f.flush()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("runs/native-clone/effect-audio".into()),
    );
    std::fs::create_dir_all(&output)?;
    let context = EffectAudioContext::default();
    let mut rows = Vec::new();
    let mut total_allocations = 0;
    let mut minimum_realtime = f64::MAX;
    let mut finite_memory = true;
    let mut audible_types = 0;
    let mut mutated_cases = 0;
    for master in [false, true] {
        for kind in 0..31 {
            let mut p = default_program(kind, master)?;
            if kind == 6 {
                p.parameters[6] = 76;
            }
            let settings = compile(p, 1200)?;
            let mut processor = EffectAudioProcessor::new(settings);
            let mut memory = vec![0.0; DELAY_WORDS];
            let frames = 192_000usize;
            let inputs: Vec<_> = (0..frames).map(input).collect();
            let mut samples = vec![StereoFrame::default(); frames];
            let before = ALLOCATIONS.load(Ordering::Relaxed);
            let start = Instant::now();
            for (i, sample) in samples.iter_mut().enumerate() {
                *sample = processor.process(inputs[i], &mut memory, &context)?;
            }
            let elapsed = start.elapsed().as_secs_f64();
            let allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
            total_allocations += allocations;
            let speed = 4.0 / elapsed;
            minimum_realtime = minimum_realtime.min(speed);
            finite_memory &= memory.iter().all(|x| x.is_finite());
            let different = samples.iter().zip(&inputs).filter(|(a, b)| a != b).count();
            let nonzero = samples
                .iter()
                .filter(|f| f.left.0 != 0 || f.right.0 != 0)
                .count();
            let tail = samples[96_000..]
                .iter()
                .filter(|f| f.left.0.abs_diff(0) > 256 || f.right.0.abs_diff(0) > 256)
                .count();
            if kind != 0 && different > 0 && nonzero > 0 {
                audible_types += 1;
            }
            let name = definition(kind, master).unwrap().name;
            let filename = format!(
                "{}-{kind:02}-{}.wav",
                if master { "master" } else { "insert" },
                name.replace([' ', '/'], "_")
            );
            wav(&output.join(&filename), &samples)?;
            rows.push(serde_json::json!({"kind":kind,"master":master,"name":name,"wav":filename,"different_frames_from_dry":different,"nonzero_frames":nonzero,"tail_frames":tail,"realtime_factor":speed,"render_allocations":allocations}));
            // Every stored property at both valid boundaries, without resetting
            // other fields to invalid invented values. Feedback and filters are
            // subsequently flushed with real zero input.
            let def = definition(kind, master).unwrap();
            for property in 0..usize::from(def.count) {
                for value in [
                    def.properties[property].minimum,
                    def.properties[property].maximum,
                ] {
                    let mut boundary = default_program(kind, master)?;
                    boundary.parameters[property] =
                        (value + i16::from(def.properties[property].zero)) as u8;
                    processor.configure(compile(boundary, 1200)?);
                    memory.fill(0.0);
                    for i in 0..4096 {
                        let _ = processor.process(input(i), &mut memory, &context)?;
                    }
                    assert!(memory.iter().all(|v| v.is_finite()));
                    mutated_cases += 1;
                }
            }
        }
    }
    assert_eq!(
        audible_types, 60,
        "every insert/master effect must produce audible processing"
    );
    assert_eq!(total_allocations, 0, "render must not allocate");
    assert!(finite_memory);
    // A nonzero tail must survive a wet/dry edit; type change must clear it.
    let mut p = default_program(14, false)?;
    p.parameters[2] = 0;
    p.parameters[4] = 10;
    p.parameters[5] = 10;
    p.parameters[0] = 100;
    let mut engine = EffectAudioProcessor::new(compile(p, 1200)?);
    let mut memory = vec![0.0; DELAY_WORDS];
    let impulse = StereoFrame {
        left: Sample(0x2000_0000),
        right: Sample(0x1000_0000),
    };
    engine.process(impulse, &mut memory, &context)?;
    for _ in 0..2048 {
        engine.process(Default::default(), &mut memory, &context)?;
    }
    let retained = memory.clone();
    p.parameters[0] = 70;
    assert!(!engine.configure(compile(p, 1200)?));
    assert_eq!(memory, retained);
    // Allocation-free nine-slot production rack benchmark, including changes
    // of tempo and controller input during playback.
    let programs = core::array::from_fn(|i| {
        default_program([11, 14, 20, 23, 26, 22, 28, 24, 11][i], i == 8).unwrap()
    });
    let mut rack = prepare_rack(programs, 1200)?;
    let mut checksum = 0i64;
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let start = Instant::now();
    for i in 0..96_000 {
        if i == 48_000 {
            rack.set_tempo(960);
            rack.set_controllers([0.25; 13]);
        }
        let frame = rack.process([input(i); 4]);
        checksum ^= i64::from(frame.left.0);
    }
    let rack_elapsed = start.elapsed().as_secs_f64();
    let rack_allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
    assert_eq!(rack_allocations, 0);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let backup = std::fs::read(root.join("firmware/Radias-backup.rdl"))?;
    let stored = radias_synth_infrastructure::rdl::programs(&backup)?;
    let mut stored_count = 0;
    for program in &stored {
        for effect in radias_synth_infrastructure::effect_audio::programs_from_stored(program) {
            compile(effect, program.tempo_tenths())?;
            stored_count += 1;
        }
    }
    let mut synth = radias_synth_infrastructure::standalone::StandaloneSynth::new();
    synth
        .engine
        .apply(radias_synth_infrastructure::synthesizer::Command::Effects(
            prepare_rack(programs, 1200)?,
        ));
    for timbre in 0..4 {
        assert!(synth.control(timbre, 71, 1));
        assert!(synth.control(timbre, 0, i32::from(timbre)));
        for note in 0..6 {
            synth
                .engine
                .apply(radias_synth_infrastructure::synthesizer::Command::Note(
                    timbre,
                    48 + 3 * timbre + note,
                    96,
                ));
        }
    }
    assert_eq!(synth.engine.active_count(), 24);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let start = Instant::now();
    let mut native_nonzero = 0;
    for _ in 0..96_000 {
        let frame = synth.engine.sample();
        native_nonzero += usize::from(frame.left.0 != 0 || frame.right.0 != 0);
        std::hint::black_box(frame);
    }
    let integrated_elapsed = start.elapsed().as_secs_f64();
    let integrated_allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
    assert_eq!(integrated_allocations, 0);
    assert!(native_nonzero > 0);
    assert!(
        integrated_elapsed < 2.0,
        "24 voices + nine effects must render in realtime"
    );
    let report = serde_json::json!({
        "passed":true,"scope":"audible native reconstructions; not original firmware sound",
        "original_FXD03_audio_parity_qualified":false,"effect_types":30,"banks":2,
        "audible_type_bank_cases":audible_types,"property_boundary_cases":mutated_cases,
        "render_allocations":total_allocations,"all_delay_memory_finite":finite_memory,
        "minimum_single_effect_realtime_factor":minimum_realtime,
        "nine_slot_rack_realtime_factor":2.0/rack_elapsed,"nine_slot_render_allocations":rack_allocations,
        "rack_delay_storage_bytes":rack.delay_storage_bytes(),"checksum":checksum,"cases":rows,
        "stored_programs_effects_compiled":stored.len(),"stored_effect_slots_compiled":stored_count,
        "native_24_voice_nine_effect_realtime_factor":2.0/integrated_elapsed,
        "native_24_voice_nine_effect_render_allocations":integrated_allocations,
        "native_integrated_nonzero_frames":native_nonzero,
    });
    std::fs::write(
        output.join("validation.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "60 audible Insert/Master cases; {mutated_cases} property boundaries; {stored_count} stored effect slots; no render allocations; 24 voices + 9 FX {:.2}x realtime; ORIGINAL FXD03 PARITY UNQUALIFIED",
        2.0 / integrated_elapsed
    );
    Ok(())
}
