//! Prepared program storage adapter. Contains state and controls, never audio.
use crate::prepared::{PreparedVoice, parameters};
use radias_synth_application::VoiceControlEvent;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Initial {
    controls: Vec<u16>,
    primary_phase: u32,
    secondary_phase: u32,
    modulated_phase: u32,
    pan_weights: [i16; 2],
    #[serde(default)]
    previous_secondary: i32,
    #[serde(default)]
    previous_primary_phase: u32,
}
#[derive(Serialize, Deserialize)]
struct Event {
    frame: u64,
    controls: Vec<u16>,
    pan_weights: [i16; 2],
}
#[derive(Serialize, Deserialize)]
struct Program {
    format: String,
    version: u32,
    sample_rate: u32,
    source_layout: String,
    reference_start_frame: u64,
    reference_voice_frames: usize,
    #[serde(default)]
    bus: u8,
    initial: Initial,
    events: Vec<Event>,
}

fn word(raw: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap())
}
fn controls(raw: &[u8]) -> Vec<u16> {
    (1..161).map(|i| word(raw, i) as u16).collect()
}
fn pan(raw: &[u8]) -> [i16; 2] {
    [word(raw, 163) as i16, word(raw, 164) as i16]
}
fn record(controls: &[u16], pan: [i16; 2]) -> Result<[u8; 704], String> {
    if controls.len() != 160 {
        return Err("Native program control count differs".into());
    }
    let mut raw = [0u8; 704];
    for (i, &value) in controls.iter().enumerate() {
        raw[(i + 1) * 4..(i + 2) * 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    for (index, value) in [
        (163, pan[0] as u16 as u32),
        (164, pan[1] as u16 as u32),
        (173, 2),
    ] {
        raw[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(raw)
}

impl PreparedVoice {
    /// Compile a qualified reference fixture into minimal product data. Only
    /// initial state and changed controls survive; output fields are excluded.
    pub fn export_program(raw: &[u8], extended: bool) -> Result<Vec<u8>, String> {
        let size = if extended { 704 } else { 696 };
        let plan = if extended {
            Self::from_reference_va_parameters(raw)
        } else {
            Self::from_reference_parameters(raw)
        }
        .map_err(str::to_owned)?;
        let first = &raw[..size];
        let mut events = Vec::new();
        for event in &plan.events {
            let r = &raw[event.frame as usize * size..(event.frame as usize + 1) * size];
            events.push(Event {
                frame: event.frame,
                controls: {
                    let mut controls = controls(r);
                    controls[136..].fill(0);
                    controls[126] = 0;
                    controls
                },
                pan_weights: pan(r),
            });
        }
        let program = Program {
            format: "RADIAS native prepared voice".into(),
            version: 1,
            sample_rate: 48000,
            source_layout:
                "SYS 2.00 Master compiled voice controls; qualified single/dual filter routes"
                    .into(),
            reference_start_frame: plan.reference_start_frame,
            reference_voice_frames: plan.reference_voice_frames,
            bus: plan.bus.index() as u8,
            initial: Initial {
                controls: controls(first),
                primary_phase: plan.initial.primary.phase.0,
                secondary_phase: plan.initial.secondary.phase().0,
                modulated_phase: plan.initial.primary.modulated_phase.0,
                pan_weights: pan(first),
                previous_secondary: plan.initial.previous_secondary.0,
                previous_primary_phase: plan.initial.previous_primary.0,
            },
            events,
        };
        serde_json::to_vec_pretty(&program).map_err(|e| e.to_string())
    }
    pub fn from_program_json(raw: &[u8]) -> Result<Self, String> {
        let program: Program = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
        if program.format != "RADIAS native prepared voice"
            || program.version != 1
            || program.sample_rate != 48000
        {
            return Err("Unsupported native voice program".into());
        }
        let mut r = record(&program.initial.controls, program.initial.pan_weights)?;
        let increment =
            (program.initial.controls[38] as u32) << 16 | program.initial.controls[39] as u32;
        for (i, value) in [
            (175, program.initial.previous_secondary as u32),
            (171, program.initial.previous_primary_phase),
            (161, program.initial.primary_phase),
            (162, program.initial.secondary_phase.wrapping_add(increment)),
            (174, program.initial.modulated_phase),
        ] {
            r[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut plan = Self::from_reference_va_parameters(&r).map_err(str::to_owned)?;
        let mut previous = None;
        for event in program.events {
            if previous.is_some_and(|frame| event.frame <= frame) {
                return Err("Native control events out of order".into());
            }
            let r = record(&event.controls, event.pan_weights)?;
            plan.events.push(VoiceControlEvent {
                frame: event.frame,
                parameters: parameters(&r).map_err(str::to_owned)?,
            });
            previous = Some(event.frame);
        }
        plan.reference_start_frame = program.reference_start_frame;
        plan.reference_voice_frames = program.reference_voice_frames;
        plan.bus =
            radias_synth_domain::pan::VoiceBus::new(program.bus).ok_or("Invalid native bus")?;
        Ok(plan)
    }
}
