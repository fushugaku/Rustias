//! Browser librarian adapter. Parsing and packed controls use the same Rust
//! readers as the native application; no firmware, device or audio is created.
use radias_synth_application::program::TimbreControls;
use radias_synth_domain::{
    drum::{DRUM_KIT_BYTES, DrumKit},
    mono_notes::NotePriority,
    program::{PROGRAM_BYTES, Program, Timbre},
};
use radias_synth_infrastructure::{
    rdl::import_library,
    standalone::{Parameter, Values, default_values, parameters},
};
use serde::Serialize;
use std::cell::RefCell;

pub const MAX_RDL_BYTES: usize = 8 * 1024 * 1024;
#[derive(Default)]
struct Buffers {
    input: Vec<u8>,
    result: Vec<u8>,
}
thread_local! {
    static BUFFERS: RefCell<Buffers> = RefCell::new(Buffers::default());
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_rdl_buffer(length: u32) -> *mut u8 {
    if length == 0 || length as usize > MAX_RDL_BYTES {
        return std::ptr::null_mut();
    }
    BUFFERS.with(|state| {
        let mut state = state.borrow_mut();
        state.input.resize(length as usize, 0);
        state.input.as_mut_ptr()
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_rdl_import(length: u32) -> u32 {
    BUFFERS.with(|state| {
        let mut state = state.borrow_mut();
        let result = if length == 0 || length as usize != state.input.len() {
            serde_json::json!({"ok":false,"error":"Invalid RDL input length"})
        } else {
            match convert(&state.input) {
                Ok(library) => serde_json::json!({"ok":true,"library":library}),
                Err(error) => serde_json::json!({"ok":false,"error":error}),
            }
        };
        state.result = serde_json::to_vec(&result).expect("Serializable librarian result");
        state.result.len() as u32
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_rdl_result_buffer() -> *const u8 {
    BUFFERS.with(|state| state.borrow().result.as_ptr())
}

#[derive(Serialize)]
pub struct ImportedLibrary {
    programs: Vec<ImportedProgram>,
    drum_kits: usize,
}
#[derive(Serialize)]
struct ImportedProgram {
    name: String,
    slot: usize,
    engine: EngineState,
    rdl: Source,
}
#[derive(Serialize)]
struct EngineState {
    version: u8,
    timbres: Vec<Vec<i32>>,
    drums: Vec<Vec<i32>>,
    effects: crate::effects::State,
}
#[derive(Default, Serialize)]
struct Source {
    version: u8,
    program: String,
    drum_kit: Option<String>,
    drum_kit_name: Option<String>,
    global: Option<String>,
    notices: Vec<String>,
    unavailable: Vec<Unavailable>,
}
#[derive(Serialize)]
struct Unavailable {
    timbre: usize,
    drum: Option<usize>,
    selection: u8,
    label: String,
}
pub fn convert(bytes: &[u8]) -> Result<ImportedLibrary, &'static str> {
    if bytes.len() > MAX_RDL_BYTES {
        return Err("This RDL file is too large (maximum 8 MiB)");
    }
    let library = import_library(bytes)?;
    let spec = parameters();
    let defaults = default_values();
    let mut imported = Vec::with_capacity(library.programs.len());
    for (slot, record) in library.programs.iter().enumerate() {
        let program =
            Program::from_bytes(&record[..PROGRAM_BYTES]).map_err(|_| "Invalid native program")?;
        let mut source = Source {
            version: 1,
            program: base64(record),
            global: library.global.map(base64),
            ..Default::default()
        };
        let drum_program = program.drum_program();
        let owner = drum_program.timbre.map(usize::from);
        let mut timbres: [Values; 4] = core::array::from_fn(|i| {
            values(
                program.timbre(i).unwrap(),
                None,
                i,
                None,
                &defaults,
                &spec,
                &mut source,
            )
        });
        let mut drums: [Values; 16] = core::array::from_fn(|i| {
            let mut v = defaults;
            v[3] = 0;
            v[4] = 32;
            v[5] = 0;
            v[6] = 20;
            v[146] = 60 + i as i32;
            v
        });
        if let Some(owner) = owner {
            if let Some(record) = library.drum_kits.get(drum_program.kit as usize) {
                let kit = DrumKit::from_bytes(&record[..DRUM_KIT_BYTES])
                    .map_err(|_| "Invalid native drum kit")?;
                source.drum_kit = Some(base64(record));
                source.drum_kit_name = Some(name(&kit.bytes()[..12]));
                for (i, v) in drums.iter_mut().enumerate() {
                    *v = values(
                        program.timbre(owner).unwrap(),
                        Some(kit.instrument(i).unwrap()),
                        owner,
                        Some(i),
                        &defaults,
                        &spec,
                        &mut source,
                    );
                    v[146] = i32::from(kit.bytes()[36 + i]);
                    v[147] = i32::from(kit.exclusive_group(i).unwrap());
                    bound(v, &spec, &format!("Drum {}", i + 1), &mut source);
                }
            } else {
                source.notices.push(format!(
                    "Drum kit {} is absent from this file. Its drums are muted until you choose a synth source or assign a sample.",
                    drum_program.kit + 1
                ));
                for i in 0..16 {
                    source.unavailable.push(Unavailable {
                        timbre: owner,
                        drum: Some(i),
                        selection: 0,
                        label: "Missing drum kit".into(),
                    });
                }
            }
        }
        let global_channel = library.global.map_or(0, |g| i32::from(g[6] & 15));
        let receive_mode = library.global.map_or(0, |g| i32::from(g[15] == 1));
        let global_values = [
            (89, i32::from(program.tempo_tenths())),
            (140, i32::from(owner.is_some())),
            (141, owner.unwrap_or(0) as i32),
            (143, i32::from(drum_program.level)),
            (144, i32::from(drum_program.pan)),
            (145, i32::from(drum_program.transpose)),
            (148, global_channel),
            (149, receive_mode),
        ];
        for (id, raw) in global_values {
            let value = raw.clamp(spec[id].min, spec[id].max);
            if value != raw {
                source.notices.push(format!(
                    "Program parameter {id} was limited from {raw} to {value}."
                ));
            }
            for v in timbres.iter_mut().chain(drums.iter_mut()) {
                v[id] = value;
            }
        }
        if program.vocoder_flags() & 128 != 0 {
            source.notices.push("Vocoder is enabled in the original program; it is not available in the web engine.".into());
        }
        if (0..4).any(|i| (program.timbre(i).unwrap().bytes()[0] >> 2) & 3 == 1) {
            source.notices.push("The hardware arpeggiator is retained as source data. Note step sequences and modulation sequences are imported into browser tracks.".into());
        }
        let mut program_name = name(program.name());
        if program_name.is_empty() {
            program_name = format!("RDL Program {:03}", slot + 1);
        }
        imported.push(ImportedProgram {
            name: program_name,
            slot,
            engine: EngineState {
                version: 1,
                timbres: timbres.iter().map(|v| v.to_vec()).collect(),
                drums: drums.iter().map(|v| v.to_vec()).collect(),
                effects: crate::effects::State::from_stored(&program, &mut source.notices),
            },
            rdl: source,
        });
    }
    Ok(ImportedLibrary {
        programs: imported,
        drum_kits: library.drum_kits.len(),
    })
}

fn values(
    owner: Timbre<'_>,
    instrument: Option<&[u8; 104]>,
    timbre: usize,
    drum: Option<usize>,
    defaults: &Values,
    spec: &[Parameter],
    source: &mut Source,
) -> Values {
    let mut raw = *owner.bytes();
    if let Some(body) = instrument {
        raw[16..120].copy_from_slice(body);
    }
    let location = drum.map_or_else(
        || format!("Timbre {}", timbre + 1),
        |d| format!("Drum {}", d + 1),
    );
    // Unavailable controller sources/destinations must not become a different
    // active route. Disable only that route and retain its original bytes.
    for i in 0..6 {
        let b = 16 + 0x56 + i * 3;
        if raw[b] > 8
            || !spec[91 + i * 4]
                .values
                .as_ref()
                .unwrap()
                .contains(&i32::from(raw[b + 1]))
        {
            if raw[b + 2] != 64 {
                source.notices.push(format!("{location}, patch {}: source {} / destination {} is unavailable; the route is disabled.", i + 1, raw[b], raw[b + 1]));
            }
            raw[b..b + 3].copy_from_slice(&[0, 0, 64]);
        }
    }
    let c = TimbreControls::from_timbre(Timbre::from_bytes(&raw))
        .expect("Destinations were validated above");
    let mut v = *defaults;
    let wave = c.oscillator_selection & 15;
    let mode = (c.oscillator_selection >> 4) & 3;
    if wave >= 6 || wave >= 4 && mode != 0 {
        let label = match wave {
            6 => "Audio In".into(),
            7 => "PCM".into(),
            _ => format!("Original OSC 1 {}", c.oscillator_selection & 63),
        };
        source.unavailable.push(Unavailable {
            timbre,
            drum,
            selection: c.oscillator_selection,
            label,
        });
        // A valid placeholder is required by the parameter schema. The web
        // adapter blocks this instrument's notes until its source is replaced.
        v[0] = 0;
        v[10] = 0;
    } else {
        v[0] = i32::from(wave);
        v[10] = i32::from(mode);
    }
    for (id, value) in [
        (11, c.oscillator_controls[0]),
        (12, c.oscillator_controls[1]),
        (13, c.secondary.selection & 3),
        (14, (c.secondary.selection >> 4) & 3),
        (15, c.secondary.pitch.semitone),
        (16, c.secondary.pitch.fine_tune),
        (17, c.mixer.levels[0]),
        (18, c.mixer.levels[1]),
        (19, c.mixer.levels[2]),
        (7, c.amplifier_level),
        (8, c.pan),
        (9, c.filter_type),
        (1, c.cutoff[0]),
        (2, c.resonance[0]),
        (25, c.eg1_intensity),
        (26, c.filter_key_tracking),
        (20, c.filter_route & 3),
        (21, (c.filter_route >> 4) & 3),
        (22, c.cutoff[1]),
        (23, c.resonance[1]),
        (27, c.filter2_eg_intensity),
        (28, c.filter2_key_tracking),
        (29, c.shaper.allocation_mode()),
        (31, c.shaper.control.depth),
        (154, raw[16 + 0x2f] & 15),
        (52, c.amplifier_key_tracking),
        (53, c.pitch.transpose),
        (54, c.pitch.fine_tune),
        (55, c.pitch.vibrato_intensity),
        (56, c.pitch.bend_range),
        (59, c.portamento.time),
        (60, c.portamento.curve),
        (68, ((c.voice_group.raw & 15) + 2).min(8)),
        (69, c.voice_group.detune),
        (70, c.voice_group.spread),
        (72, if raw[4] < 16 { raw[4] } else { 16 }),
        (119, raw[6]),
        (120, raw[7]),
    ] {
        v[id] = i32::from(value);
    }
    for (id, value) in [
        (24, c.filter_route & 128 != 0),
        (
            30,
            matches!(
                c.shaper.position,
                radias_synth_domain::waveshaper::ShaperPosition::PreAmp
            ),
        ),
        (57, c.pitch.bend_enabled),
        (58, c.pitch.wheel_enabled),
        (61, c.portamento.switch_required),
        (62, c.voice_mode.polyphonic),
        (63, c.voice_mode.multi_trigger),
        (65, c.sustain.enabled),
        (67, c.voice_group.raw & 128 != 0),
        (71, owner.enabled()),
        (151, raw[5] & 64 != 0),
        (153, raw[5] & 32 != 0),
    ] {
        v[id] = i32::from(value);
    }
    v[64] = match c.voice_mode.priority {
        NotePriority::Last => 0,
        NotePriority::Lowest => 1,
        NotePriority::Highest => 2,
    };
    for (i, e) in c.envelope.iter().enumerate() {
        let adsr = [32, 3, 36][i];
        let extra = [40, 44, 48][i];
        for j in 0..4 {
            v[adsr + j] = i32::from(e.adsr[j]);
        }
        v[extra] = i32::from(e.curve & 7);
        v[extra + 1] = i32::from(e.velocity_level_sensitivity);
        v[extra + 2] = i32::from(e.velocity_time_sensitivity);
        v[extra + 3] = i32::from(e.key_tracking);
    }
    for (i, lfo) in c.modulation.lfo.iter().enumerate() {
        let b = 73 + i * 8;
        v[b] = i32::from(lfo.waveform & 3);
        v[b + 1] = i32::from(lfo.shape & 127);
        v[b + 2] = i32::from(lfo.frequency & 127);
        v[b + 3] = i32::from((lfo.phase_sync >> 5) & 3);
        v[b + 4] = i32::from(lfo.phase_sync & 31);
        v[b + 5] = i32::from(lfo.phase_sync & 128 != 0);
        v[b + 6] = i32::from(c.modulation.tempo_divisions[i] & 31);
    }
    for i in 0..6 {
        let b = 16 + 0x56 + i * 3;
        v[90 + i * 4] = i32::from(raw[b]);
        v[91 + i * 4] = i32::from(raw[b + 1]);
        v[92 + i * 4] = i32::from(raw[b + 2]);
    }
    if raw[16 + 0x2e] & 3 == 3 {
        source
            .notices
            .push(format!("{location}: an unknown Drive/WS mode is disabled."));
    }
    bound(&mut v, spec, &location, source);
    if v[119] > v[120] {
        // An inverted range accepts no notes on the device. Do not turn it into
        // a playable broad range in the browser.
        v[71] = 0;
        v[119] = defaults[119];
        v[120] = defaults[120];
        source
            .notices
            .push(format!("{location}: an inverted key range is disabled."));
    }
    v
}
fn bound(v: &mut Values, spec: &[Parameter], location: &str, source: &mut Source) {
    for p in spec {
        let old = v[p.id];
        let value = if p
            .values
            .as_ref()
            .is_some_and(|choices| !choices.contains(&old))
        {
            p.default
        } else {
            old.clamp(p.min, p.max)
        };
        if value != old {
            source.notices.push(format!(
                "{location}, parameter {}: {old} is outside the web range; using {value}.",
                p.id
            ));
        }
        v[p.id] = value;
    }
}
fn name(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}
fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for part in bytes.chunks(3) {
        let b = u32::from(part[0]) << 16
            | u32::from(*part.get(1).unwrap_or(&0)) << 8
            | u32::from(*part.get(2).unwrap_or(&0));
        result.push(DIGITS[((b >> 18) & 63) as usize] as char);
        result.push(DIGITS[((b >> 12) & 63) as usize] as char);
        result.push(if part.len() > 1 {
            DIGITS[((b >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if part.len() > 2 {
            DIGITS[(b & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}
