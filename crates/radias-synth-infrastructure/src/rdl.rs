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
