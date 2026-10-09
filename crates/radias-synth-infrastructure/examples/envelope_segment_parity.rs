use radias_synth_domain::amp_envelope::{AmpEnvelope, AmpEnvelopeParameters};
use radias_synth_domain::amplifier_control::AmplifierControl;
use radias_synth_domain::envelope_segment::{EnvelopeSegment, EnvelopeTiming};
use radias_synth_infrastructure::firmware::{
    amplifier_tables, envelope_curves, envelope_timing_tables,
};
use std::{fs, path::PathBuf};
fn word(raw: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or("..".into()));
    let curves = envelope_curves(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let mut errors = 0;
    let raw = fs::read(root.join("runs/native-clone/envelope-curves.bin"))?;
    if raw.len() != 524_288 * 12 {
        return Err("Incomplete curve corpus".into());
    }
    for (i, r) in raw.chunks_exact(12).enumerate() {
        let actual = curves.evaluate(word(r, 0) as u8, word(r, 1));
        if actual != word(r, 2) {
            if errors < 3 {
                eprintln!("Curve {i}: {actual} != {}", word(r, 2));
            }
            errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/envelope-segments.bin"))?;
    if raw.len() != 98_304 * 44 {
        return Err("Incomplete segment corpus".into());
    }
    for (i, r) in raw.chunks_exact(44).enumerate() {
        let mut segment = EnvelopeSegment {
            phase: word(r, 2),
            increment: word(r, 3),
            start: word(r, 4) as u16,
            difference: word(r, 5) as i16,
            level: 0,
            regular_increment: word(r, 6) != 0,
        };
        let complete = segment.advance(&curves, word(r, 1) as u8);
        let actual = [
            segment.phase,
            segment.level as u32,
            segment.regular_increment as u32,
            complete as u32,
        ];
        let expected = core::array::from_fn::<_, 4, _>(|n| word(r, 7 + n));
        if actual != expected {
            if errors < 3 {
                eprintln!("Segment {i}: {actual:?} != {expected:?}");
            }
            errors += 1;
        }
    }
    let tables = envelope_timing_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let raw = fs::read(root.join("runs/native-clone/envelope-timings.bin"))?;
    if raw.len() != 98_304 * 32 {
        return Err("Incomplete timing corpus".into());
    }
    for (i, r) in raw.chunks_exact(32).enumerate() {
        let actual = tables.increment(EnvelopeTiming {
            curve: word(r, 1) as u8,
            time: word(r, 2) as u8,
            velocity: word(r, 3) as u8,
            velocity_sensitivity: word(r, 4) as u8,
            note: word(r, 5) as u8,
            key_tracking: word(r, 6) as u8,
        });
        if actual != word(r, 7) {
            if errors < 3 {
                eprintln!("Timing {i}: {actual} != {} for {:?}", word(r, 7), r);
            }
            errors += 1;
        }
    }
    let raw = fs::read(root.join("runs/native-clone/envelope-lifecycle.bin"))?;
    if raw.len() != 40_960 * 100 {
        return Err("Incomplete lifecycle corpus".into());
    }
    let mut envelope = AmpEnvelope::default();
    let mut previous = None;
    let mut lifecycle_errors = 0;
    for (i, r) in raw.chunks_exact(100).enumerate() {
        let parameters = AmpEnvelopeParameters {
            attack: word(r, 4) as u8,
            decay: word(r, 5) as u8,
            sustain: word(r, 6) as u8,
            release: word(r, 7) as u8,
            curve: word(r, 8) as u8,
            velocity: word(r, 9) as u8,
            velocity_sensitivity: word(r, 10) as u8,
            note: word(r, 11) as u8,
            key_tracking: word(r, 12) as u8,
        };
        match word(r, 2) {
            0 => {
                envelope = AmpEnvelope::default();
                envelope.note_on(parameters, &curves, &tables, 0);
            }
            1 => {
                envelope.publish();
                envelope.tick(parameters, &curves, &tables, word(r, 3) != 0);
            }
            2 => envelope.release(parameters, &tables),
            3 => envelope.edit(
                previous.ok_or("Previous ADSR parameters absent")?,
                parameters,
                &tables,
            ),
            _ => return Err("Unknown lifecycle action".into()),
        }
        previous = Some(parameters);
        let s = envelope.segment;
        let actual = [
            s.phase,
            s.increment,
            s.start as u32,
            s.difference as u16 as u32,
            s.level as u32,
            envelope.published_level as u32,
            envelope.target as u32,
            envelope.increment_flags as u32,
            envelope.divider as u32,
            envelope.release_hold as u32,
            envelope.stage as u8 as u32,
            envelope.dirty as u32,
        ];
        let expected = core::array::from_fn::<_, 12, _>(|n| word(r, 13 + n));
        if actual != expected {
            if lifecycle_errors < 4 {
                eprintln!(
                    "Lifecycle {i}, scenario {}, tick {}, action {}: {actual:?} != {expected:?}",
                    word(r, 0),
                    word(r, 1),
                    word(r, 2)
                );
            }
            lifecycle_errors += 1;
        }
    }
    errors += lifecycle_errors;
    let tables = amplifier_tables(&fs::read(root.join("firmware/RADIAS_SYS_0200.bin"))?)?;
    let raw = fs::read(root.join("runs/native-clone/envelope-amplifier.bin"))?;
    if raw.len() != 32768 * 48 {
        return Err("Incomplete amplifier corpus".into());
    }
    let mut amplifier_errors = 0;
    for (i, r) in raw.chunks_exact(48).enumerate() {
        let control = AmplifierControl {
            level: word(r, 0) as u8,
            level_offset: word(r, 1) as i8,
            source_gain: word(r, 2) as u16,
            envelope_level: word(r, 3) as u16,
            velocity: word(r, 4) as u8,
            velocity_sensitivity: word(r, 5) as u8,
            modulation: [word(r, 6) as i16, word(r, 7) as i16],
            midi_volume: if word(r, 8) == 0 {
                None
            } else {
                Some(word(r, 9) as u8)
            },
            program_volume: word(r, 10) as u8,
        };
        let actual = tables.target(control) as u16 as u32;
        if actual != word(r, 11) {
            if amplifier_errors < 4 {
                eprintln!("Amplifier {i}: {actual} != {} for {control:?}", word(r, 11));
            }
            amplifier_errors += 1;
        }
    }
    errors += amplifier_errors;
    let report = serde_json::json!({"curve_cases":524288,"segment_cases":98304,"timing_cases":98304,
        "lifecycle_frames":40960,"lifecycle_errors":lifecycle_errors,"amplifier_cases":32768,"amplifier_errors":amplifier_errors,"errors":errors,
        "original_bytes_executed":true,"complete_adsr":false});
    fs::write(
        root.join("runs/native-clone/envelope-segment-parity.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    println!("{report}");
    if errors != 0 {
        return Err("Controller envelope segment parity failed".into());
    }
    Ok(())
}
