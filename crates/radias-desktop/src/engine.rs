use radias_application::{Machine, PcmPolicy, Program};
use radias_domain::{backup, controller::Bus};
use radias_infrastructure::{
    artifacts::ArtifactPaths,
    audio::LiveInput,
    capture::{self, Captures},
    pcm::NativePcmBank,
    wav,
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Instant,
};

// Conservative original-firmware servicing budgets used by the verified
// auditions. An SH edit-buffer match alone does not mean the DSP has received
// the configuration yet. These are desktop readiness guards, not ASIC timing.
const BOOT_STEPS: u64 = 60_000_000;
const PROGRAM_SERVICE_STEPS: u64 = 10_000_000;

pub enum Command {
    Midi(Vec<u8>),
    Note(u8, bool),
    Press(usize, u8),
    Pot(usize, usize, u16),
    Encoder(i32),
    Preset(usize),
    Program(Program),
    Preview,
    Stop,
    InputWave(PathBuf),
    Pcm(PathBuf),
    Audio(Option<LiveInput>),
    Reset,
    Shutdown,
    RunReference(bool),
}
pub struct View {
    pub status: String,
    pub fault: String,
    pub command_error: Option<String>,
    pub pcm_description: String,
    pub ready: bool,
    pub lcd: Vec<u8>,
    pub pots: [[u16; 8]; 5],
    pub current: Option<Program>,
    pub channel: u8,
    pub steps: u64,
    pub native_frames: u64,
    pub speed: f64,
    pub monitor_frames: u64,
    pub monitor_nonzero_frames: u64,
    pub preview_progress: Option<f32>,
    pub preview_generation: u64,
    pub preview: Vec<[i32; 2]>,
    pub wav: Option<PathBuf>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            status: "Запуск прошивки…".into(),
            fault: String::new(),
            command_error: None,
            pcm_description: "PCM отключены".into(),
            ready: false,
            lcd: vec![0; 8192],
            pots: [[512; 8]; 5],
            current: None,
            channel: 0,
            steps: 0,
            native_frames: 0,
            speed: 0.0,
            monitor_frames: 0,
            monitor_nonzero_frames: 0,
            preview_progress: None,
            preview_generation: 0,
            preview: Vec::new(),
            wav: None,
        }
    }
}
pub struct Engine {
    pub commands: Sender<Command>,
    pub view: Arc<Mutex<View>>,
    pub names: Vec<String>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Engine {
    pub fn new(root: PathBuf, audio: Option<LiveInput>) -> Self {
        let (tx, rx) = mpsc::channel();
        let view = Arc::new(Mutex::new(View::default()));
        let bank = std::fs::read(root.join("firmware/Radias-backup.rdl")).ok();
        let programs = bank
            .as_ref()
            .and_then(|b| backup::records(b, b"316P", b"316p").ok())
            .unwrap_or_default()
            .iter()
            .filter_map(|bytes| Program::from_bytes(&bytes[..bytes.len().min(1790)]).ok())
            .collect::<Vec<_>>();
        let names = programs
            .iter()
            .enumerate()
            .map(|(i, p)| {
                format!(
                    "{}{:02}  {}",
                    char::from(b'A' + (i / 16) as u8),
                    i % 16 + 1,
                    p.name()
                )
            })
            .collect();
        let worker = view.clone();
        let handle = thread::spawn(move || {
            if let Err(error) = run(root, bank, programs, rx, &worker, audio) {
                let mut v = worker.lock().unwrap();
                v.status = "Ошибка движка".into();
                v.fault = error;
                v.ready = false;
            }
        });
        Self {
            commands: tx,
            view,
            names,
            thread: Some(handle),
        }
    }
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}
fn run(
    root: PathBuf,
    bank: Option<Vec<u8>>,
    programs: Vec<Program>,
    rx: Receiver<Command>,
    view: &Arc<Mutex<View>>,
    mut audio: Option<LiveInput>,
) -> Result<(), String> {
    let firmware =
        std::fs::read(root.join("firmware/RADIAS_SYS_0200.bin")).map_err(|e| e.to_string())?;
    let default_pcm_path = root.join("runs/alternative-pcm/alternative-pcm.bin");
    let working_path = root.join("runs/rust-desktop/working-flash.bin");
    let working_exists = working_path.exists();
    let default_pcm = if default_pcm_path.exists() && !working_exists {
        Some(NativePcmBank::parse(
            std::fs::read(&default_pcm_path).map_err(|e| e.to_string())?,
        )?)
    } else {
        None
    };
    let mut machine = Machine::new(
        firmware,
        bank.as_deref(),
        if default_pcm.is_some() {
            PcmPolicy::NativeFlash
        } else {
            PcmPolicy::SilentGuards
        },
    )?;
    if working_exists {
        machine.board.flash = radias_infrastructure::flash::load(&working_path)?;
        if NativePcmBank::parse(machine.board.flash.clone()).is_ok() {
            machine.pcm_policy = PcmPolicy::NativeFlash;
            view.lock().unwrap().pcm_description = "PCM: рабочий образ".into();
        }
    }
    if let Some(pcm) = default_pcm {
        pcm.mount(&mut machine.board.flash)?;
        view.lock().unwrap().pcm_description = "PCM: альтернативы".into();
    }
    let output_path = root.join("runs/rust-desktop/last-preview.wav");
    let master_path = output_path.with_extension("mix-master.wav");
    let slave_path = output_path.with_extension("mix-slave.wav");
    let dry_path = output_path.with_extension("native-dry.wav");
    std::fs::create_dir_all(output_path.parent().unwrap()).map_err(|e| e.to_string())?;
    let mut source_paths = vec![root.join("firmware/RADIAS_SYS_0200.bin")];
    if bank.is_some() {
        source_paths.push(root.join("firmware/Radias-backup.rdl"));
    }
    if default_pcm_path.exists() {
        source_paths.push(default_pcm_path);
    }
    let mut artifact_paths = ArtifactPaths::new(
        &source_paths,
        &[
            output_path.clone(),
            master_path.clone(),
            slave_path.clone(),
            dry_path.clone(),
            working_path.clone(),
        ],
    )?;
    radias_infrastructure::flash::save(&working_path, &machine.board.flash, &artifact_paths)?;
    let mut saved_revision = machine.board.nor.revision;
    let captures = Arc::new(Mutex::new(Captures {
        recording: audio.is_some(),
        dry_enabled: false,
        mix_enabled: true,
        vocoder_enabled: false,
        ..Default::default()
    }));
    capture::attach(&mut machine.board, captures.clone());
    let start = Instant::now();
    let mut releases = Vec::<(u64, usize, u8)>::new();
    let mut preview = None::<(u64, u64, bool)>;
    let mut preview_requested = false;
    let mut held = Vec::<(u8, Vec<(u8, u8)>)>::new();
    let mut preview_notes = Vec::<(u8, u8)>::new();
    let mut selected = None::<Program>;
    // The selector initially shows bank entry zero. Load that exact payload
    // instead of showing A01 while playing a different boot-time edit buffer.
    let mut pending_program = programs.first().cloned();
    let mut program_service_until = 0u64;
    let mut reference_running = true;
    loop {
        if machine.board.nor.revision != saved_revision {
            radias_infrastructure::flash::save(
                &working_path,
                &machine.board.flash,
                &artifact_paths,
            )?;
            saved_revision = machine.board.nor.revision;
        }
        let channel = machine
            .board
            .backup_global_data
            .get(6)
            .copied()
            .unwrap_or(0)
            & 15;
        for cmd in rx.try_iter() {
            match cmd {
                Command::RunReference(running) => reference_running = running,
                Command::Audio(output) => {
                    if let Some(audio) = &audio {
                        audio.clear();
                    }
                    audio = output;
                    if preview.is_none() {
                        clear_captures(&mut captures.lock().unwrap());
                    }
                }
                Command::Shutdown => {
                    radias_infrastructure::flash::save(
                        &working_path,
                        &machine.board.flash,
                        &artifact_paths,
                    )?;
                    return Ok(());
                }
                Command::Reset => {
                    if let Some(audio) = &audio {
                        audio.clear();
                    }
                    clear_captures(&mut captures.lock().unwrap());
                    machine.reset();
                    releases.clear();
                    preview = None;
                    preview_requested = false;
                    held.clear();
                    selected = None;
                    pending_program = None;
                    program_service_until = 0;
                    let mut v = view.lock().unwrap();
                    v.ready = false;
                    v.current = None;
                    v.preview_progress = None;
                    v.command_error = None;
                    v.fault.clear();
                }
                Command::Midi(bytes) => {
                    if bytes.first().is_some_and(|status| {
                        status & 0xf0 == 0x90 || *status == 0xfa || *status == 0xfb
                    }) {
                        if let Some(audio) = &audio {
                            audio.resume();
                        }
                    }
                    machine.midi(&bytes);
                }
                Command::Note(note, down) => {
                    if down && !held.iter().any(|(key, _)| *key == note) {
                        if let Some(program) = current_program(&mut machine) {
                            let routes = program.keyboard_routes(note, channel);
                            if !routes.is_empty() {
                                if let Some(audio) = &audio {
                                    audio.resume();
                                }
                            }
                            send_notes(&mut machine, &routes, true);
                            held.push((note, routes));
                        }
                    } else if !down {
                        if let Some(i) = held.iter().position(|(key, _)| *key == note) {
                            let (_, routes) = held.remove(i);
                            send_notes(&mut machine, &routes, false);
                        }
                    }
                }
                Command::Press(row, column) => {
                    machine.key(row, column, true)?;
                    releases.push((machine.cpu.steps + 2_000_000, row, column));
                }
                Command::Pot(ch, mux, v) => {
                    machine.pot(ch, mux, v)?;
                }
                Command::Encoder(v) => machine.board.turn_encoder(v),
                Command::Preset(i) => {
                    if let Some(p) = programs.get(i) {
                        pending_program = Some(p.clone());
                        view.lock().unwrap().status = "Загрузка патча…".into();
                    }
                }
                Command::Program(p) => {
                    pending_program = Some(p);
                    view.lock().unwrap().status = "Загрузка параметров…".into();
                }
                Command::Preview => {
                    if preview.is_none() {
                        preview_requested = true;
                        let mut v = view.lock().unwrap();
                        v.preview_progress = Some(0.0);
                        v.status = "Подготовка звука…".into();
                    }
                }
                Command::Stop => {
                    if let Some(audio) = &audio {
                        audio.pause();
                    }
                    all_notes_off(&mut machine);
                    preview = None;
                    preview_requested = false;
                    held.clear();
                    clear_captures(&mut captures.lock().unwrap());
                    let mut v = view.lock().unwrap();
                    v.preview_progress = None;
                    v.status = "Готово".into();
                }
                Command::InputWave(path) => {
                    match wav::WaveInput::load(&path).and_then(|wave| {
                        artifact_paths.protect_input(&path)?;
                        Ok(wave)
                    }) {
                        Ok(mut wave) => {
                            wave.looping = true;
                            machine.board.adc_input = Some(Box::new(move || wave.next()));
                            view.lock().unwrap().command_error = None;
                        }
                        Err(error) => {
                            view.lock().unwrap().command_error = Some(error);
                        }
                    }
                }
                Command::Pcm(path) => {
                    let parsed = std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(NativePcmBank::parse)
                        .and_then(|pcm| {
                            artifact_paths.protect_input(&path)?;
                            Ok(pcm)
                        });
                    match parsed {
                        Ok(pcm) => {
                            if let Some(audio) = &audio {
                                audio.clear();
                            }
                            clear_captures(&mut captures.lock().unwrap());
                            pcm.mount(&mut machine.board.flash)?;
                            machine.pcm_policy = PcmPolicy::NativeFlash;
                            machine.reset();
                            selected = None;
                            pending_program = None;
                            program_service_until = 0;
                            preview = None;
                            preview_requested = false;
                            held.clear();
                            releases.clear();
                            let mut v = view.lock().unwrap();
                            v.ready = false;
                            v.current = None;
                            v.preview_progress = None;
                            v.command_error = None;
                            v.pcm_description = "PCM: внешний банк".into();
                        }
                        Err(error) => {
                            view.lock().unwrap().command_error = Some(error);
                        }
                    }
                }
            }
        }
        if !reference_running {
            thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }
        if machine.cpu.steps >= BOOT_STEPS
            && machine.board.midi_in.is_empty()
            && machine.board.scif.rx.is_empty()
        {
            if let Some(p) = pending_program.take() {
                machine.midi(&p.sysex(channel)?);
                selected = Some(p);
                program_service_until = machine.cpu.steps + PROGRAM_SERVICE_STEPS;
            }
        }
        if !machine.fault.is_empty() {
            if let Some(audio) = &audio {
                audio.clear();
            }
            let mut v = view.lock().unwrap();
            v.fault = machine.fault.clone();
            v.status = "Движок остановлен".into();
            v.ready = false;
            drop(v);
            match rx.recv() {
                Ok(Command::Reset) => {
                    machine.reset();
                    selected = None;
                    pending_program = None;
                    program_service_until = 0;
                    preview = None;
                    preview_requested = false;
                    held.clear();
                    releases.clear();
                    let mut v = view.lock().unwrap();
                    v.fault.clear();
                    v.current = None;
                    v.preview_progress = None;
                    v.command_error = None;
                }
                Ok(Command::Shutdown) | Err(_) => return Ok(()),
                _ => {}
            }
            continue;
        }
        // Accept the request once and service it after pending MIDI/program
        // work. A Stop followed quickly by Preview used to discard Preview
        // because All Notes Off was still in the controller's input queue.
        if preview_requested
            && view.lock().unwrap().ready
            && pending_program.is_none()
            && machine.board.midi_in.is_empty()
            && machine.board.scif.rx.is_empty()
            && preview.is_none()
        {
            if let Some(program) = current_program(&mut machine) {
                preview_requested = false;
                match program.audition_routes(channel) {
                    Ok(routes) => {
                        all_notes_off(&mut machine);
                        if let Some(audio) = &audio {
                            audio.clear();
                        }
                        let mut c = captures.lock().unwrap();
                        clear_captures(&mut c);
                        c.dry_enabled = true;
                        c.vocoder_enabled = true;
                        c.recording = true;
                        let first = machine.board.audio_frames;
                        preview = Some((first, first + 48_000, false));
                        send_notes(&mut machine, &routes, true);
                        preview_notes = routes;
                        view.lock().unwrap().status = "Рендер…".into();
                    }
                    Err(error) => {
                        let mut v = view.lock().unwrap();
                        v.command_error = Some(error);
                        v.preview_progress = None;
                    }
                }
            }
        }
        let end = preview.map(|(_, last, released)| if released { last } else { last - 12000 });
        machine.run_steps(200_000, end);
        releases.retain(|&(step, row, column)| {
            if machine.cpu.steps >= step {
                let _ = machine.key(row, column, false);
                false
            } else {
                true
            }
        });
        if let Some((first, last, released)) = preview {
            if !released && machine.board.audio_frames >= last - 12000 {
                send_notes(&mut machine, &preview_notes, false);
                preview = Some((first, last, true));
            }
            if machine.board.audio_frames >= last {
                let mut c = captures.lock().unwrap();
                c.recording = false;
                // Listening projection includes all four native Master feeds.
                // Slave is already on the Master bus and must not be summed
                // again. Fixed /4 avoids overflow without per-patch rescaling.
                let frames = c.mix[0].iter().map(monitor_frame).collect::<Vec<_>>();
                for path in [&master_path, &slave_path, &dry_path] {
                    artifact_paths.allow_write(path)?;
                }
                wav::write_pcm32(&master_path, &c.mix[0])?;
                wav::write_pcm32(&slave_path, &c.mix[1])?;
                wav::write_pcm32(&dry_path, &c.dry)?;
                artifact_paths.allow_write(&output_path)?;
                wav::write_pcm32(&output_path, &frames)?;
                clear_captures(&mut c);
                let mut v = view.lock().unwrap();
                v.preview = frames;
                v.wav = Some(output_path.clone());
                v.preview_generation += 1;
                v.preview_progress = None;
                v.status = "Готово".into();
                preview = None;
            } else {
                view.lock().unwrap().preview_progress =
                    Some((machine.board.audio_frames - first) as f32 / (last - first) as f32);
            }
        }
        let mut current = None;
        let ready = machine.cpu.steps >= BOOT_STEPS
            && machine.cpu.steps >= program_service_until
            && machine.board.dsp.iter().all(|dsp| dsp.started)
            && machine.board.scif.control & 0x10 != 0;
        if ready && machine.board.midi_in.is_empty() && machine.board.scif.rx.is_empty() {
            let ptr = machine.board.read32(0x0c0cea3c);
            let p = BoardRange::read_program(&mut machine, ptr);
            if let Some(p) = p {
                let matched = selected
                    .as_ref()
                    .is_none_or(|wanted| wanted.bytes() == p.bytes());
                if matched && preview.is_none() && !preview_requested {
                    view.lock().unwrap().status = "Готово".into();
                }
                if matched {
                    selected = None;
                }
                current = Some(p);
            }
        }
        let mut v = view.lock().unwrap();
        v.ready = ready
            && pending_program.is_none()
            && machine.board.midi_in.is_empty()
            && machine.board.scif.rx.is_empty()
            && current.as_ref().is_some_and(|actual| {
                selected
                    .as_ref()
                    .is_none_or(|wanted| wanted.bytes() == actual.bytes())
            });
        v.channel = channel;
        v.lcd = machine.board.lcd.pixels();
        v.pots = machine.board.panel_adc;
        v.steps = machine.cpu.steps;
        v.native_frames = machine.board.audio_frames;
        v.speed = machine.board.audio_frames as f64 / 48000.0 / start.elapsed().as_secs_f64();
        if current.is_some() {
            v.current = current;
        }
        // Outside an explicit WAV render, drain original Master samples to
        // the audio-device port. Previously these samples were discarded,
        // so keyboard and virtual MIDI input could never be heard.
        if preview.is_none() {
            let mut c = captures.lock().unwrap();
            // MIDI/program queues temporarily make the controls unready, but
            // the DSP still emits valid samples. Never drop those frames.
            if machine.cpu.steps >= BOOT_STEPS {
                if let Some(audio) = &audio {
                    v.monitor_frames += c.mix[0].len() as u64;
                    v.monitor_nonzero_frames += c.mix[0]
                        .iter()
                        .map(monitor_frame)
                        .filter(|frame| frame.iter().any(|&x| x != 0))
                        .count() as u64;
                    audio.push(c.mix[0].iter().map(monitor_frame));
                }
            }
            clear_captures(&mut c);
            c.recording = audio.is_some();
            c.dry_enabled = false;
            c.vocoder_enabled = false;
        }
    }
}
fn monitor_frame(native: &[i32; 8]) -> [i32; 2] {
    std::array::from_fn(|channel| {
        let sum = (0..4)
            .map(|bus| native[bus * 2 + channel] as i64)
            .sum::<i64>();
        (sum / 4) as i32
    })
}
fn clear_captures(captures: &mut Captures) {
    captures.dry.clear();
    for frames in &mut captures.mix {
        frames.clear();
    }
    for frames in &mut captures.vocoder {
        frames.clear();
    }
}
struct BoardRange;
fn current_program(machine: &mut Machine) -> Option<Program> {
    let ptr = machine.board.read32(0x0c0cea3c);
    BoardRange::read_program(machine, ptr)
}
fn send_notes(machine: &mut Machine, routes: &[(u8, u8)], down: bool) {
    let mut bytes = Vec::with_capacity(routes.len() * 3);
    for &(channel, note) in routes {
        bytes.extend_from_slice(&[
            if down { 0x90 | channel } else { 0x80 | channel },
            note,
            if down { 100 } else { 0 },
        ]);
    }
    machine.midi(&bytes);
}
fn all_notes_off(machine: &mut Machine) {
    let mut bytes = Vec::with_capacity(48);
    for channel in 0..16 {
        bytes.extend_from_slice(&[0xb0 | channel, 123, 0]);
    }
    machine.midi(&bytes);
}
impl BoardRange {
    fn read_program(machine: &mut Machine, a: u32) -> Option<Program> {
        if !(0x0c000000..0x10000000).contains(&a) || a & 0xffffff > 0xffffff - 1790 {
            return None;
        }
        let mut data = [0; 1790];
        for (i, v) in data.iter_mut().enumerate() {
            *v = machine.board.read8(a + i as u32);
        }
        Program::from_bytes(&data).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radias_infrastructure::audio::Player;
    use std::time::Duration;

    #[test]
    #[ignore = "Uses the real audio output and original firmware assets"]
    fn original_keyboard_and_preview_reach_audio_device() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = source.join(format!(
            "runs/rust-desktop/audio-smoke-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("firmware")).unwrap();
        std::fs::create_dir_all(root.join("runs/alternative-pcm")).unwrap();
        // All generated artifacts go to a separate workspace. Never replace
        // the user's active edit/Flash state during an automated audio check.
        for file in [
            "firmware/RADIAS_SYS_0200.bin",
            "firmware/Radias-backup.rdl",
            "runs/alternative-pcm/alternative-pcm.bin",
        ] {
            std::fs::copy(source.join(file), root.join(file)).unwrap();
        }
        let player = Player::new().expect("Open system audio output");
        let engine = Engine::new(root.clone(), Some(player.live_input()));
        let wait = |predicate: &dyn Fn(&View) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(90);
            loop {
                {
                    let v = engine.view.lock().unwrap();
                    assert!(v.fault.is_empty(), "Engine fault: {}", v.fault);
                    if predicate(&v) {
                        break;
                    }
                }
                assert!(Instant::now() < deadline, "Original engine timeout");
                thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&|v| v.ready && v.current.is_some());
        let backup = std::fs::read(source.join("firmware/Radias-backup.rdl")).unwrap();
        let records = backup::records(&backup, b"316P", b"316p").unwrap();
        let expected = Program::from_bytes(&records[0][..1790]).unwrap();
        wait(&|v| {
            v.ready
                && v.current
                    .as_ref()
                    .is_some_and(|p| p.bytes() == expected.bytes())
        });
        println!(
            "Keyboard routes: {:?}",
            expected.keyboard_routes(60, engine.view.lock().unwrap().channel)
        );
        player.stop();
        let first = engine.view.lock().unwrap().monitor_frames;
        let before = player.status().audible_frames;
        let start = Instant::now();
        engine.send(Command::Note(60, true));
        wait(&|v| v.monitor_frames >= first + 24_000);
        engine.send(Command::Note(60, false));
        wait(&|v| v.monitor_frames >= first + 36_000);
        let elapsed = start.elapsed().as_secs_f64();
        let keyboard = player.status();
        {
            let view = engine.view.lock().unwrap();
            println!(
                "Keyboard status: {keyboard:?}, native frames {}, captured {}, nonzero {}, speed {}",
                view.native_frames,
                view.monitor_frames,
                view.monitor_nonzero_frames,
                0.75 / elapsed
            );
        }
        assert!(keyboard.error.is_none());
        engine.send(Command::Stop);
        let generation = engine.view.lock().unwrap().preview_generation;
        engine.send(Command::Preview);
        wait(&|v| v.preview_generation > generation);
        let frames = engine.view.lock().unwrap().preview.clone();
        assert!(
            frames.iter().any(|frame| frame.iter().any(|&x| x != 0)),
            "Original preview is silent"
        );
        let count = frames.len();
        let nonzero_preview_frames = frames
            .iter()
            .filter(|frame| frame.iter().any(|&x| x != 0))
            .count();
        let before_preview = player.status().audible_frames;
        player.play(frames);
        let deadline = Instant::now() + Duration::from_secs(5);
        while player.status().preview_playing {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        let preview = player.status();
        println!("Preview status: {preview:?}, native frames {count}");
        assert!(
            keyboard.audible_frames > before,
            "Keyboard samples never reached the output callback"
        );
        assert!(preview.audible_frames >= before_preview + nonzero_preview_frames as u64 - 16);
        assert!(preview.error.is_none());
        engine.send(Command::Stop);
        let report = serde_json::json!({
            "keyboard_output_verified": true, "preview_output_verified": true,
            "device": preview.device, "sample_rate": preview.sample_rate,
            "callbacks": preview.callbacks, "keyboard_nonzero_frames": keyboard.audible_frames - before,
            "preview_nonzero_frames": preview.audible_frames - before_preview,
            "preview_native_frames": count, "keyboard_emulated_seconds": 0.75,
            "keyboard_wall_seconds": elapsed, "keyboard_speed": 0.75 / elapsed,
            "workspace": root, "audio_error": preview.error,
            "acoustic_or_hardware_capture_verified": false,
            "realtime_instrument_complete": false,
        });
        std::fs::write(
            source.join("runs/rust-desktop/audio-smoke-verification.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("{report}");
    }
}
