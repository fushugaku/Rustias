//! Librarian storage adapter. No firmware machine or audio is constructed.
use radias_synth_domain::drum::{DRUM_KIT_BYTES, DrumKit};
use radias_synth_domain::program::{PROGRAM_BYTES, Program};

struct Chunk<'a> {
    kind: &'a [u8],
    payload: &'a [u8],
    remaining: &'a [u8],
}
fn chunk(bytes: &[u8]) -> Result<Chunk<'_>, &'static str> {
    if bytes.len() < 12 {
        return Err("Truncated RDL chunk");
    }
    let header = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let end = header.checked_add(size).ok_or("RDL length overflow")?;
    if header < 12 || end > bytes.len() {
        return Err("Invalid RDL chunk length");
    }
    Ok(Chunk {
        kind: &bytes[..4],
        payload: &bytes[header..end],
        remaining: &bytes[end..],
    })
}

/// Importable librarian records. Keep the whole payload, including editor
/// metadata after the native program/drum body, for lossless source retention.
/// Unlike the device-bank readers below, this also accepts a partial bank or
/// an individual program saved by the librarian.
pub struct ImportLibrary<'a> {
    pub programs: Vec<&'a [u8]>,
    pub drum_kits: Vec<&'a [u8]>,
    pub global: Option<&'a [u8]>,
    pub formants: Vec<&'a [u8]>,
}
pub fn import_library<'a>(bytes: &'a [u8]) -> Result<ImportLibrary<'a>, &'static str> {
    let outer = chunk(bytes)?;
    if !outer.remaining.is_empty() {
        return Err("Expected one complete RDL file");
    }
    let mut library = ImportLibrary {
        programs: Vec::new(),
        drum_kits: Vec::new(),
        global: None,
        formants: Vec::new(),
    };
    let mut program_bank = false;
    let mut drum_bank = false;
    let mut global_bank = false;
    let mut formant_bank = false;
    let mut section = |kind: &[u8], payload: &'a [u8]| -> Result<(), &'static str> {
        match kind {
            b"316p" => {
                if payload.len() < PROGRAM_BYTES {
                    return Err("Truncated native program");
                }
                library.programs.push(payload);
                if library.programs.len() > 256 {
                    return Err("Too many RDL programs");
                }
            }
            b"316d" => {
                if payload.len() < DRUM_KIT_BYTES {
                    return Err("Truncated native drum kit");
                }
                library.drum_kits.push(payload);
                if library.drum_kits.len() > 32 {
                    return Err("Too many RDL drum kits");
                }
            }
            b"316g" => {
                if library.global.is_some() || !matches!(payload.len(), 656 | 736) {
                    return Err("Expected one complete native Global record");
                }
                library.global = Some(payload);
            }
            b"316f" => {
                formant_record(payload)?;
                library.formants.push(payload);
                if library.formants.len() > 16 {
                    return Err("Too many RDL Formant Motion records");
                }
            }
            _ => {}
        }
        Ok(())
    };
    let mut banks = if outer.kind == b"316B" {
        outer.payload
    } else if outer.kind == b"316P" {
        section_records(outer.payload, b"316p", &mut section)?;
        &[]
    } else if outer.kind == b"316p" {
        section(outer.kind, outer.payload)?;
        &[]
    } else if outer.kind == b"316F" {
        section_records(outer.payload, b"316f", &mut section)?;
        &[]
    } else if outer.kind == b"316f" {
        section(outer.kind, outer.payload)?;
        &[]
    } else {
        return Err("Choose a RADIAS .rdl library or program file");
    };
    while !banks.is_empty() {
        let bank = chunk(banks)?;
        banks = bank.remaining;
        let (seen, record) = match bank.kind {
            b"316P" => (&mut program_bank, b"316p"),
            b"316D" => (&mut drum_bank, b"316d"),
            b"316G" => (&mut global_bank, b"316g"),
            b"316F" => (&mut formant_bank, b"316f"),
            _ => continue,
        };
        if *seen {
            return Err("Duplicate RDL bank");
        }
        *seen = true;
        section_records(bank.payload, record, &mut section)?;
    }
    if library.programs.is_empty() && library.formants.is_empty() {
        return Err("The RDL file contains no programs or Formant Motion records");
    }
    Ok(library)
}
fn section_records<'a>(
    mut bytes: &'a [u8],
    expected: &[u8],
    record: &mut impl FnMut(&[u8], &'a [u8]) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    while !bytes.is_empty() {
        let value = chunk(bytes)?;
        bytes = value.remaining;
        if value.kind == expected {
            record(value.kind, value.payload)?;
        }
    }
    Ok(())
}

pub fn global(bytes: &[u8]) -> Result<&[u8], &'static str> {
    let library = chunk(bytes)?;
    if library.kind != b"316B" || !library.remaining.is_empty() {
        return Err("Expected one complete RDL library");
    }
    let mut value = None;
    let mut sections = library.payload;
    while !sections.is_empty() {
        let section = chunk(sections)?;
        sections = section.remaining;
        if section.kind != b"316G" {
            continue;
        }
        let mut records = section.payload;
        while !records.is_empty() {
            let record = chunk(records)?;
            records = record.remaining;
            if record.kind != b"316g" {
                continue;
            }
            if value.is_some() || !matches!(record.payload.len(), 656 | 736) {
                return Err("Expected exactly one native Global record");
            }
            value = Some(record.payload);
        }
    }
    value.ok_or("No RDL Global record")
}

pub fn global_performance(
    bytes: &[u8],
) -> Result<radias_synth_domain::performance::GlobalPerformance, &'static str> {
    let raw = global(bytes)?;
    Ok(radias_synth_domain::performance::GlobalPerformance {
        channel: raw[6] & 15,
        amplitude_receive_mode: raw[15],
    })
}

pub fn programs(bytes: &[u8]) -> Result<Vec<Program>, &'static str> {
    let library = chunk(bytes)?;
    if library.kind != b"316B" || !library.remaining.is_empty() {
        return Err("Expected one complete RDL library");
    }
    let mut bank = None;
    let mut sections = library.payload;
    while !sections.is_empty() {
        let section = chunk(sections)?;
        sections = section.remaining;
        if section.kind != b"316P" {
            continue;
        }
        if bank.is_some() {
            return Err("Duplicate RDL program bank");
        }
        let mut values = Vec::with_capacity(256);
        let mut records = section.payload;
        while !records.is_empty() {
            let record = chunk(records)?;
            records = record.remaining;
            if record.kind == b"316p" {
                let native = record
                    .payload
                    .get(..PROGRAM_BYTES)
                    .ok_or("Truncated native program")?;
                values.push(Program::from_bytes(native).map_err(|_| "Invalid native program")?);
            }
        }
        if values.len() != 256 {
            return Err("Expected 256 RDL programs");
        }
        bank = Some(values);
    }
    bank.ok_or("No RDL program bank")
}

pub fn drum_kits(bytes: &[u8]) -> Result<Vec<DrumKit>, &'static str> {
    let library = chunk(bytes)?;
    if library.kind != b"316B" || !library.remaining.is_empty() {
        return Err("Expected one complete RDL library");
    }
    let mut bank = None;
    let mut sections = library.payload;
    while !sections.is_empty() {
        let section = chunk(sections)?;
        sections = section.remaining;
        if section.kind != b"316D" {
            continue;
        }
        if bank.is_some() {
            return Err("Duplicate RDL drum bank");
        }
        let mut kits = Vec::with_capacity(32);
        let mut records = section.payload;
        while !records.is_empty() {
            let record = chunk(records)?;
            records = record.remaining;
            if record.kind == b"316d" {
                if record.payload.len() != 0x860 {
                    return Err("Expected a complete librarian drum-kit record");
                }
                kits.push(
                    DrumKit::from_bytes(&record.payload[..DRUM_KIT_BYTES])
                        .map_err(|_| "Invalid native drum kit")?,
                );
            }
        }
        if kits.len() != 32 {
            return Err("Expected 32 RDL drum kits");
        }
        bank = Some(kits);
    }
    bank.ok_or("No RDL drum bank")
}

/// Librarian Formant records use a16-byte editor prefix with LE frame count.
/// Return only the typed frame data; `ImportLibrary` retains all source bytes.
pub fn formant_record(
    payload: &[u8],
) -> Result<radias_synth_domain::formant_motion::MotionRecord<'_>, &'static str> {
    if payload.len() < 16 {
        return Err("Truncated RDL Formant Motion header");
    }
    let count = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
    if count > radias_synth_domain::formant_motion::MAX_FRAMES
        || payload.len() != 16 + 16 * count
    {
        return Err("Invalid RDL Formant Motion count or extent");
    }
    radias_synth_domain::formant_motion::MotionRecord::new(&payload[16..])
        .map_err(|_| "Invalid Formant Motion frames")
}
