use radias_application::{BackupScope, Machine, PcmPolicy};
use radias_domain::controller::Bus;
use radias_infrastructure::{
    artifacts::ArtifactPaths,
    capture::{self, Captures, SharedCaptures},
    flash,
    pcm::NativePcmBank,
    state, wav,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Args {
    firmware: PathBuf,
    backup: Option<PathBuf>,
    global_only: bool,
    pcm: Option<PathBuf>,
    flash: Option<PathBuf>,
    dump: Option<PathBuf>,
    dry: Option<PathBuf>,
    mix: Option<PathBuf>,
    vocoder: Option<PathBuf>,
    upload: Option<PathBuf>,
    serial: Option<PathBuf>,
    midi_capture: Option<PathBuf>,
    input_wave: Option<PathBuf>,
    input_loop: bool,
    trace: Option<PathBuf>,
    fxd_trace: Option<PathBuf>,
    fxd_link: Option<PathBuf>,
    interactive: bool,
    strict: bool,
    zero_input: bool,
    native_pcm: bool,
    echo: bool,
    flash_compat: bool,
    steps: u64,
    watch: u32,
}
fn arguments() -> Result<Args, String> {
    let mut a = Args {
        firmware: "firmware/RADIAS_SYS_0200.bin".into(),
        steps: 5_000_000,
        watch: u32::MAX,
        ..Default::default()
    };
    let mut values = std::env::args().skip(1);
    while let Some(v) = values.next() {
        let mut next = || {
            values
                .next()
                .ok_or_else(|| format!("Missing value for {v}"))
        };
        match v.as_str(){"--firmware"=>a.firmware=next()?.into(),"--backup"=>a.backup=Some(next()?.into()),"--backup-global"=>{a.backup=Some(next()?.into());a.global_only=true;},"--pcm-bank"=>{a.pcm=Some(next()?.into());a.native_pcm=true;},"--flash-image"=>a.flash=Some(next()?.into()),"--dump"=>a.dump=Some(next()?.into()),"--dry-audio"=>a.dry=Some(next()?.into()),"--mix-audio"=>a.mix=Some(next()?.into()),"--vocoder-audio"=>a.vocoder=Some(next()?.into()),"--dsp-upload"=>a.upload=Some(next()?.into()),"--serial-trace"=>a.serial=Some(next()?.into()),"--input-wave"=>a.input_wave=Some(next()?.into()),"--input-loop"=>a.input_loop=true,"--trace"=>a.trace=Some(next()?.into()),"--fxd-trace"=>a.fxd_trace=Some(next()?.into()),"--fxd-link-trace"=>a.fxd_link=Some(next()?.into()),"--midi-capture"=>a.midi_capture=Some(next()?.into()),"--interactive"=>a.interactive=true,"--strict"=>a.strict=true,"--zero-input"=>a.zero_input=true,"--native-pcm"=>a.native_pcm=true,"--fxd-return-zero"=>a.echo=false,"--fxd-echo-return"=>a.echo=true,"--flash-native-bypass-reset"=>a.flash_compat=true,"--steps"=>a.steps=next()?.parse().map_err(|_|"Invalid steps")?,"--watch-dsp"=>a.watch=u32::from_str_radix(&next()?,16).map_err(|_|"Invalid DSP address")?,"--help"=>return Err("radias-rust --firmware BIN --backup RDL [--pcm-bank BIN] [--interactive | --steps N] [--dry-audio WAV] [--mix-audio PREFIX] [--vocoder-audio PREFIX] [--dsp-upload BIN] [--dump JSON]".into()),_=>return Err(format!("Unsupported argument: {v}"))}
    }
    Ok(a)
}
fn suffixed(prefix: &Path, label: &str) -> PathBuf {
    PathBuf::from(format!("{}-{label}.wav", prefix.display()))
}
fn parent(path: &Path) -> Result<(), String> {
    if let Some(p) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn save_audio(a: &Args, capture: &SharedCaptures, paths: &ArtifactPaths) -> Result<(), String> {
    let c = capture.lock().unwrap();
    if let Some(p) = &a.dry {
        paths.allow_write(p)?;
        wav::write_pcm32(p, &c.dry)?;
    }
    if let Some(p) = &a.mix {
        for chip in 0..2 {
            paths.allow_write(&suffixed(p, if chip == 0 { "master" } else { "slave" }))?;
            wav::write_pcm32(
                &suffixed(p, if chip == 0 { "master" } else { "slave" }),
                &c.mix[chip],
            )?;
        }
    }
    if let Some(p) = &a.vocoder {
        for chip in 0..2 {
            paths.allow_write(&suffixed(p, if chip == 0 { "master" } else { "slave" }))?;
            wav::write_pcm32(
                &suffixed(p, if chip == 0 { "master" } else { "slave" }),
                &c.vocoder[chip],
            )?;
        }
    }
    Ok(())
}
fn decimal(s: Option<&str>) -> Result<u64, String> {
    s.ok_or("Missing command argument")?
        .parse()
        .map_err(|_| "Invalid decimal argument".into())
}
fn hex(s: Option<&str>) -> Result<u32, String> {
    u32::from_str_radix(s.ok_or("Missing command argument")?, 16)
        .map_err(|_| "Invalid hexadecimal argument".into())
}
fn command(
    line: &str,
    m: &mut Machine,
    a: &Args,
    capture: &SharedCaptures,
    upload: &Option<Arc<Mutex<BufWriter<fs::File>>>>,
    uploaded: &AtomicU64,
    bank: &Option<NativePcmBank>,
    wave: &Arc<Mutex<wav::WaveInput>>,
    paths: &mut ArtifactPaths,
) -> Result<Option<Value>, String> {
    let mut words = line.split_whitespace();
    let cmd = words.next().unwrap_or("");
    match cmd {
        "quit" => return Ok(None),
        "state" => {}
        "run" => m.run_steps(decimal(words.next())?.min(10_000_000), None),
        "runframes" => {
            let n = decimal(words.next())?;
            if n > 480000 {
                return Err("Use runframes 0..480000 (48 kHz frames)".into());
            }
            let first = m.board.audio_frames;
            m.run_frames(n);
            let c = capture.lock().unwrap();
            return Ok(Some(
                json!({"audio_frame_clocks":m.board.audio_frames,"advanced_audio_frames":m.board.audio_frames-first,"steps":m.cpu.steps,"cycles":m.board.ticks,"fault":m.fault,"dsp_faults":[m.board.dsp[0].fault,m.board.dsp[1].fault],"dry_frames":c.dry.len(),"mix_frames":[c.mix[0].len(),c.mix[1].len()]}),
            ));
        }
        "midi" => {
            let bytes = words
                .map(|s| u8::from_str_radix(s, 16).map_err(|_| "Invalid MIDI byte"))
                .collect::<Result<Vec<_>, _>>()?;
            m.midi(&bytes);
        }
        "peek" => {
            let address = hex(words.next())?;
            let n = decimal(words.next())?.min(65536);
            let bytes = (0..n)
                .map(|i| m.board.read8(address.wrapping_add(i as u32)))
                .collect::<Vec<_>>();
            return Ok(Some(json!({"address":address,"bytes":bytes})));
        }
        "dsppeek" => {
            let chip = decimal(words.next())? as usize;
            let address = hex(words.next())?;
            let n = decimal(words.next())? as usize;
            if chip >= 2 || address as usize >= m.board.dsp[chip].memory.len() {
                return Err("Invalid DSP memory range".into());
            }
            let n = n
                .min(65536)
                .min(m.board.dsp[chip].memory.len() - address as usize);
            return Ok(Some(
                json!({"chip":chip,"word_address":address,"words":m.board.dsp[chip].memory[address as usize..address as usize+n]}),
            ));
        }
        "dspio" => {
            let chip = decimal(words.next())? as usize;
            if chip >= 2 {
                return Err("Invalid DSP chip".into());
            }
            return Ok(Some(
                json!({"chip":chip,"registers":m.board.dsp[chip].io.iter().map(|(&address,&value)|json!({"address":address,"value":value})).collect::<Vec<_>>()}),
            ));
        }
        "midiinfo" => {
            return Ok(Some(
                json!({"pending_input_bytes":m.board.midi_in.len(),"receive_fifo_bytes":m.board.scif.rx.len(),"receive_enabled":m.board.scif.control&0x10!=0}),
            ));
        }
        "clockinfo" => return Ok(Some(state::clock_info())),
        "codecinfo" => {
            let c = &m.board.codec;
            return Ok(Some(
                json!({"model":"AK4626VQ ADC power and DAC I2S input","pdn_high":c.pdn_high,"smute_high":c.smute_high,"adc_data_ready":c.adc_ready(),"adc_startup_lrck_cycles":522,"adc_startup_lrck_remaining":c.startup_remaining,"dac_initialization_lrck_cycles":516,"dac_initialization_lrck_remaining":c.dac.startup_remaining,"dac_power_ready":c.dac.ready(),"dac_i2s_input_implemented":true,"dac_received_frames":c.dac.serial.frames,"dac_received_frame_valid":c.dac.serial.valid,"dac_short_slots":c.dac.serial.short_slots,"dac_input_words":c.dac.serial.samples,"dac_source_execution_implemented":false,"adc_filter_group_delay_modeled":false,"dac_output_implemented":false,"register_control_interface_implemented":false,"register_control_interface_used":false}),
            ));
        }
        "audio" => {
            let v = words.next().ok_or("Missing audio mode")?;
            if (a.dry.is_none() && a.mix.is_none() && a.vocoder.is_none())
                || !matches!(v, "start" | "stop")
            {
                return Err("Use --dry-audio WAV, --mix-audio PREFIX or --vocoder-audio PREFIX and audio start/stop".into());
            }
            capture.lock().unwrap().recording = v == "start";
            if v == "stop" {
                save_audio(a, capture, paths)?;
            }
        }
        "input" => {
            match words.next() {
                Some("wave") => {
                    let tail = line
                        .trim_start()
                        .strip_prefix("input")
                        .unwrap()
                        .trim_start();
                    let path = Path::new(tail.strip_prefix("wave").unwrap().trim_start());
                    if path.as_os_str().is_empty() {
                        return Err("Use input wave PATH".into());
                    }
                    let source = wav::WaveInput::load(path)?;
                    paths.protect_input(path)?;
                    *wave.lock().unwrap() = source;
                    let shared = wave.clone();
                    m.board.adc_input = Some(Box::new(move || shared.lock().unwrap().next()));
                }
                Some("loop") => {
                    let mode = words
                        .next()
                        .ok_or("Use input loop on/off with an active WAVE source")?;
                    if m.board.adc_input.is_none() || !matches!(mode, "on" | "off") {
                        return Err("Use input loop on/off with an active WAVE source".into());
                    }
                    wave.lock().unwrap().looping = mode == "on";
                }
                Some("rewind") => {
                    if m.board.adc_input.is_none() {
                        return Err("No active WAVE input to rewind".into());
                    }
                    wave.lock().unwrap().rewind();
                }
                Some("silence") => {
                    m.board.adc_input = None;
                    m.board.adc_idle_noise = false;
                }
                Some("noise") => {
                    m.board.adc_input = None;
                    m.board.adc_idle_noise = true;
                }
                _ => return Err(
                    "Use input silence/noise, input wave PATH, input loop on/off or input rewind"
                        .into(),
                ),
            }
        }
        "inputinfo" => {
            let mut info = json!({"mode":if m.board.adc_input.is_some(){"wave"}else if m.board.adc_idle_noise{"noise"}else{"silence"},"sample_rate":48000,"adc_bits":24});
            if m.board.adc_input.is_some() {
                let w = wave.lock().unwrap();
                for (key,value) in json!({"path":w.path,"source_channels":w.channels,"source_bits":w.bits,"frames":w.frames.len(),"position":w.position,"delivered":w.delivered,"loop":w.looping}).as_object().unwrap() {info[key]=value.clone();}
            }
            return Ok(Some(info));
        }
        "key" => {
            let row = decimal(words.next())? as usize;
            let column = decimal(words.next())?;
            let down = decimal(words.next())?;
            if column >= 8 || down > 1 {
                return Err("Invalid panel key".into());
            }
            m.key(row, column as u8, down != 0)?;
        }
        "pot" => {
            let channel = decimal(words.next())? as usize;
            let mux = decimal(words.next())? as usize;
            let v = decimal(words.next())?;
            if v > 1023 {
                return Err("Invalid panel potentiometer".into());
            }
            m.pot(channel, mux, v as u16)?;
        }
        "encoder" => {
            let v: i32 = words
                .next()
                .ok_or("Missing encoder movement")?
                .parse()
                .map_err(|_| "Invalid encoder movement")?;
            if !(-32..=32).contains(&v) {
                return Err("Invalid encoder movement".into());
            }
            m.board.turn_encoder(v);
        }
        "gpio" => {
            let address = hex(words.next())?;
            let value = hex(words.next())?;
            if value > 255 {
                return Err("Invalid GPIO byte".into());
            }
            m.board.inputs.insert(address, value as u8);
            m.board.refresh_control_pins();
        }
        "break" => {
            let address = hex(words.next())?;
            for _ in 0..10_000_000 {
                if m.cpu.pc == address || !m.fault.is_empty() {
                    break;
                }
                m.run_steps(1, None);
            }
        }
        "reset" => {
            if let Some(f) = upload {
                let mut writer = f.lock().unwrap();
                writer.flush().map_err(|e| e.to_string())?;
                writer.get_mut().set_len(0).map_err(|e| e.to_string())?;
                use std::io::Seek;
                writer.get_mut().rewind().map_err(|e| e.to_string())?;
                uploaded.store(0, Ordering::Relaxed);
            }
            m.reset();
        }
        "uploadinfo" => {
            if let Some(f) = upload {
                f.lock().unwrap().flush().map_err(|e| e.to_string())?;
            }
            return Ok(Some(
                json!({"enabled":upload.is_some(),"mode":"stream","bytes":uploaded.load(Ordering::Relaxed),"capped":false,"reset_scope":"current-machine-run"}),
            ));
        }
        "pcminfo" => {
            return Ok(Some(
                json!({"profile":if a.native_pcm{"native-loader"}else{"explicit-pcm-off"},"source":a.pcm.as_ref().map(|p|p.to_string_lossy().into_owned()).unwrap_or_default(),"mounted_bank_bytes":bank.as_ref().map(|b|b.bytes.len()).unwrap_or(0),"mount_offset":0x1e0000,"mount_capacity":0x220000,"input_was_full_flash":bank.as_ref().is_some_and(|b|b.from_full_flash),"table_counts":bank.as_ref().map(|b|b.table_counts).unwrap_or([0;5]),"container_and_table_bounds_checked":bank.is_some(),"sample_decoding_and_checksums_checked_by_adapter":false,"original_bank_identity_verified":false,"factory_waveform_playback_verified":false,"hardware_conformance_verified":false}),
            ));
        }
        "flashinfo" => {
            return Ok(Some(
                json!({"model":"S29JL032H model 02 x16","bytes":0x400000,"sectors":71,"banks":4,"busy":m.board.nor.busy(),"bypass":m.board.nor.bypass_mode(),"erase_suspended":m.board.nor.erase_suspended(),"program_operations":m.board.nor.programs,"erased_sectors":m.board.nor.erases,"cell_revision":m.board.nor.revision,"image":a.flash.as_ref().map(|p|p.to_string_lossy().into_owned()).unwrap_or_default(),"timing_profile":"deterministic-typical","physical_protection_and_otp_known":false,"native_bypass_f0_compatibility":m.board.nor.native_bypass_f0_compatibility,"native_bypass_f0_hardware_verified":false,"security_sector_implemented":false,"program_suspend_implemented":false}),
            ));
        }
        "flashsave" => {
            if m.board.nor.busy() {
                return Err(
                    "Wait for the Flash program/erase operation to complete before exporting"
                        .into(),
                );
            }
            let path = Path::new(
                line.trim_start()
                    .strip_prefix("flashsave")
                    .unwrap()
                    .trim_start(),
            );
            if path.as_os_str().is_empty() {
                return Err("Use flashsave PATH".into());
            }
            parent(path)?;
            if !a
                .flash
                .as_ref()
                .map(|active| radias_infrastructure::artifacts::same_file(path, active))
                .transpose()?
                .unwrap_or(false)
            {
                paths.allow_extra_output(path)?;
            }
            flash::save(path, &m.board.flash, paths)?;
            return Ok(Some(
                json!({"path":path,"bytes":m.board.flash.len(),"cell_revision":m.board.nor.revision}),
            ));
        }
        "fxdpeek" => {
            let width = decimal(words.next())?;
            let address = hex(words.next())?;
            let n = decimal(words.next())?.min(4096);
            if !matches!(width, 32 | 48) || address > 65535 {
                return Err("Use fxdpeek 48|32 HEX_INDEX COUNT".into());
            }
            let n = n.min(65536 - address as u64);
            let u = &m.board.fxd_upload;
            let values = (0..n)
                .map(|i| {
                    if width == 48 {
                        u.words48.get(&(address as u16 + i as u16)).copied()
                    } else {
                        u.words32
                            .get(&(address as u16 + i as u16))
                            .map(|&v| v as u64)
                    }
                })
                .collect::<Vec<_>>();
            let controls = if width == 48 {
                &u.word_controls48
            } else {
                &u.word_controls32
            };
            return Ok(Some(
                json!({"port_width":width,"index":address,"words":values,"controls":(0..n).map(|i|controls.get(&(address as u16+i as u16)).copied()).collect::<Vec<_>>(),"scope":"last-observed-uploads-by-index","control_fields_interpreted":false,"execution_implemented":false,"capture_error":u.error}),
            ));
        }
        "templatepeek" => {
            let kind = decimal(words.next())? as usize;
            let index = decimal(words.next())? as usize;
            let record = m
                .board
                .backup_librarian
                .templates
                .get(kind)
                .and_then(|bank| bank.get(index))
                .ok_or("Use templatepeek KIND(0..4) INDEX with an imported bank")?;
            return Ok(Some(
                json!({"kind":kind,"index":index,"native_flash_bank":kind<3,"bytes":record}),
            ));
        }
        _ => return Err(format!("Unknown debugger command: {cmd}")),
    }
    if let Some(f) = upload {
        f.lock().unwrap().flush().map_err(|e| e.to_string())?;
    }
    Ok(Some(state::machine_state(m)))
}
fn run() -> Result<i32, String> {
    let a = arguments()?;
    if a.input_loop && a.input_wave.is_none() {
        return Err("--input-loop requires --input-wave".into());
    }
    let initial_wave = a
        .input_wave
        .as_ref()
        .map(|path| wav::WaveInput::load(path))
        .transpose()?;
    let firmware = fs::read(&a.firmware).map_err(|e| e.to_string())?;
    let backup = a
        .backup
        .as_ref()
        .map(fs::read)
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut m = Machine::new_scoped(
        firmware,
        backup.as_deref(),
        if a.native_pcm {
            PcmPolicy::NativeFlash
        } else {
            PcmPolicy::SilentGuards
        },
        if a.global_only {
            BackupScope::GlobalOnly
        } else {
            BackupScope::Full
        },
    )?;
    m.board.strict = a.strict;
    m.board.adc_idle_noise = !a.zero_input;
    m.board.fxd_return_zero = !a.echo;
    m.board.nor.native_bypass_f0_compatibility = a.flash_compat;
    for d in &mut m.board.dsp {
        d.watch_address = a.watch;
    }
    let bank = a
        .pcm
        .as_ref()
        .map(|path| NativePcmBank::parse(fs::read(path).map_err(|e| e.to_string())?))
        .transpose()?;
    if let Some(bank) = &bank {
        bank.mount(&mut m.board.flash)?;
    }
    for p in [
        &a.dry,
        &a.mix,
        &a.vocoder,
        &a.upload,
        &a.dump,
        &a.flash,
        &a.serial,
        &a.midi_capture,
        &a.trace,
        &a.fxd_trace,
        &a.fxd_link,
    ]
    .into_iter()
    .flatten()
    {
        parent(p)?;
    }
    let inputs = std::iter::once(a.firmware.clone())
        .chain(
            [&a.backup, &a.pcm, &a.input_wave]
                .into_iter()
                .flatten()
                .cloned(),
        )
        .collect::<Vec<_>>();
    let mut outputs = [
        &a.dry,
        &a.upload,
        &a.dump,
        &a.flash,
        &a.serial,
        &a.midi_capture,
        &a.trace,
        &a.fxd_trace,
        &a.fxd_link,
    ]
    .into_iter()
    .flatten()
    .cloned()
    .collect::<Vec<_>>();
    for prefix in [&a.mix, &a.vocoder].into_iter().flatten() {
        for chip in ["master", "slave"] {
            outputs.push(suffixed(prefix, chip));
        }
    }
    let mut paths = ArtifactPaths::new(&inputs, &outputs)?;
    if let Some(path) = &a.flash {
        if path.exists() {
            m.board.flash = flash::load(path)?;
        }
        if let Some(bank) = &bank {
            bank.mount(&mut m.board.flash)?;
        }
        flash::save(path, &m.board.flash, &paths)?;
    }
    let mut saved_revision = m.board.nor.revision;
    let wave = Arc::new(Mutex::new(wav::WaveInput::default()));
    if let Some(mut input) = initial_wave {
        input.looping = a.input_loop;
        *wave.lock().unwrap() = input;
        let shared = wave.clone();
        m.board.adc_input = Some(Box::new(move || shared.lock().unwrap().next()));
    }
    if a.trace.is_some() || a.fxd_link.is_some() {
        m.step_observer = Some(Box::new(
            radias_infrastructure::diagnostics::Diagnostics::new(
                a.trace.as_deref(),
                a.fxd_link.as_deref(),
            )?,
        ));
    }

    let c = Arc::new(Mutex::new(Captures {
        recording: !a.interactive,
        dry_enabled: a.dry.is_some(),
        mix_enabled: a.mix.is_some(),
        vocoder_enabled: a.vocoder.is_some(),
        ..Default::default()
    }));
    capture::attach(&mut m.board, c.clone());
    let upload = a
        .upload
        .as_ref()
        .map(|p| fs::File::create(p).map(|f| Arc::new(Mutex::new(BufWriter::new(f)))))
        .transpose()
        .map_err(|e| format!("Cannot write HPI capture: {e}"))?;
    let uploaded = Arc::new(AtomicU64::new(0));
    if let Some(f) = &upload {
        let f = f.clone();
        let n = uploaded.clone();
        m.board.dsp_upload_observer = Some(Box::new(move |_, _, v| {
            f.lock().unwrap().write_all(&[v]).unwrap();
            n.fetch_add(1, Ordering::Relaxed);
        }));
    }
    let serial = a
        .serial
        .as_ref()
        .map(|path| fs::File::create(path).map(|file| Arc::new(Mutex::new(BufWriter::new(file)))))
        .transpose()
        .map_err(|e| e.to_string())?;
    if let Some(file) = &serial {
        file.lock()
            .unwrap()
            .write_all(b"bus_word_clock,chip,port,wire_word\n")
            .map_err(|e| e.to_string())?;
        let shared = file.clone();
        m.board.serial_observer = Some(Box::new(move |chip, port, clock, word| {
            writeln!(shared.lock().unwrap(), "{clock},{chip},{port},{word:08x}")
                .expect("Cannot write serial trace");
        }));
    }
    let midi_capture = a
        .midi_capture
        .as_ref()
        .map(|path| fs::File::create(path).map(|file| Arc::new(Mutex::new(BufWriter::new(file)))))
        .transpose()
        .map_err(|e| e.to_string())?;
    if let Some(file) = &midi_capture {
        let shared = file.clone();
        m.board.midi_observer = Some(Box::new(move |_, byte| {
            shared
                .lock()
                .unwrap()
                .write_all(&[byte])
                .expect("Cannot write MIDI capture");
        }));
    }
    let fxd_trace = a
        .fxd_trace
        .as_ref()
        .map(|path| fs::File::create(path).map(|file| Arc::new(Mutex::new(BufWriter::new(file)))))
        .transpose()
        .map_err(|e| e.to_string())?;
    if let Some(file) = &fxd_trace {
        file.lock()
            .unwrap()
            .write_all(b"cpu_clock\tpc\toperation\tphysical_address\tbyte\n")
            .map_err(|e| e.to_string())?;
        let shared = file.clone();
        m.board.fxd_observer = Some(Box::new(move |clock, pc, address, value, write| {
            writeln!(
                shared.lock().unwrap(),
                "{clock}\t{pc:08x}\t{}\t{address:08x}\t{value:02x}",
                if write { 'W' } else { 'R' }
            )
            .expect("Cannot write FXD03 trace");
        }));
    }
    std::panic::set_hook(Box::new(|_| {}));
    if a.interactive {
        for line in io::stdin().lock().lines() {
            let line = line.map_err(|e| e.to_string())?;
            let answer = match command(
                &line, &mut m, &a, &c, &upload, &uploaded, &bank, &wave, &mut paths,
            ) {
                Ok(Some(v)) => v,
                Ok(None) => break,
                Err(error) => json!({"error":error}),
            };
            if let Some(path) = &a.flash {
                if m.board.nor.revision != saved_revision {
                    flash::save(path, &m.board.flash, &paths)?;
                    saved_revision = m.board.nor.revision;
                }
            }
            for file in [&serial, &midi_capture, &fxd_trace].into_iter().flatten() {
                file.lock().unwrap().flush().map_err(|e| e.to_string())?;
            }
            println!("{answer}");
            io::stdout().flush().map_err(|e| e.to_string())?;
        }
    } else {
        m.run_steps(a.steps, None);
        let result = state::machine_state(&m);
        if let Some(p) = &a.dump {
            paths.allow_write(p)?;
            fs::write(p, format!("{result}\n")).map_err(|e| e.to_string())?;
        }
        println!("{result}");
    }
    for file in [&serial, &midi_capture, &fxd_trace].into_iter().flatten() {
        file.lock().unwrap().flush().map_err(|e| e.to_string())?;
    }
    m.finish_observation()?;
    save_audio(&a, &c, &paths)?;
    if let Some(path) = &a.flash {
        flash::save(path, &m.board.flash, &paths)?;
    }
    if let Some(f) = upload {
        f.lock().unwrap().flush().map_err(|e| e.to_string())?;
    }
    Ok(if m.fault.is_empty() { 0 } else { 1 })
}
fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
