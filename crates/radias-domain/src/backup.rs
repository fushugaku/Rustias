//! Checked librarian framing and unchanged native flash payloads.
#[derive(Clone, Default)]
pub struct BackupExtras {
    pub templates: [Vec<Vec<u8>>; 5],
    pub formants: Vec<Vec<u8>>,
}
fn le32(data: &[u8], a: usize) -> Result<u32, String> {
    let source = data
        .get(a..a.checked_add(4).ok_or("backup offset overflow")?)
        .ok_or("Truncated RADIAS backup field")?;
    Ok(u32::from_le_bytes(source.try_into().unwrap()))
}
fn magic(data: &[u8], a: usize, s: &[u8; 4]) -> bool {
    data.get(a..a.saturating_add(4)) == Some(s)
}
fn chunk(data: &[u8], a: usize, limit: usize) -> Result<(usize, usize), String> {
    if a > limit || limit - a < 12 {
        return Err("Truncated RADIAS backup chunk".into());
    }
    let header = le32(data, a + 4)? as usize;
    let size = le32(data, a + 8)? as usize;
    let end = a
        .checked_add(header)
        .and_then(|n| n.checked_add(size))
        .ok_or("backup chunk overflow")?;
    if header < 12 || end > limit {
        return Err("Invalid RADIAS backup chunk bounds".into());
    }
    Ok((a + header, end))
}
pub fn records(data: &[u8], section: &[u8; 4], record: &[u8; 4]) -> Result<Vec<Vec<u8>>, String> {
    if !magic(data, 0, b"316B") {
        return Err("Not a RADIAS 316B librarian backup".into());
    }
    let (payload, end) = chunk(data, 0, data.len())?;
    if payload < 20 || end != data.len() {
        return Err("Invalid RADIAS backup root".into());
    }
    let sections = le32(data, 16)?;
    if sections > 128 {
        return Err("Too many RADIAS backup sections".into());
    }
    let mut pos = payload;
    let mut result = Vec::new();
    for _ in 0..sections {
        let (payload, section_end) = chunk(data, pos, end)?;
        let selected = magic(data, pos, section);
        let mut cursor = payload;
        while cursor < section_end {
            let (begin, last) = chunk(data, cursor, section_end)?;
            if selected && magic(data, cursor, record) {
                result.push(data[begin..last].to_vec());
            }
            cursor = last;
        }
        pos = section_end;
    }
    if pos != end {
        return Err("Unparsed bytes in RADIAS backup".into());
    }
    Ok(result)
}
pub fn global(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut found = records(data, b"316G", b"316g")?;
    if found.len() != 1 {
        return Err("RADIAS backup must contain exactly one Global record".into());
    }
    let v = found.remove(0);
    if v.len() != 656 && v.len() != 736 {
        return Err("Unsupported RADIAS Global record size".into());
    }
    Ok(v)
}
pub fn formant_frames(record: &[u8]) -> Result<usize, String> {
    if record.len() < 16 {
        return Err("Truncated RADIAS Formant Motion record".into());
    }
    let n = le32(record, 0)? as usize;
    if n > 750 || record.len() != 16 + n * 16 {
        return Err("Invalid RADIAS Formant Motion frame count or payload length".into());
    }
    Ok(n)
}
pub fn extras(data: &[u8]) -> Result<BackupExtras, String> {
    let mut out = BackupExtras::default();
    let groups = [b"31Tt", b"31Ti", b"31Tf", b"31Tm", b"31Ts"];
    let kinds = [b"Timb", b"IFx ", b"MFx ", b"MSeq", b"SSeq"];
    let sizes = [240, 36, 34, 30, 340];
    let counts = [128, 128, 128, 64, 64];
    for kind in 0..5 {
        let found = records(data, b"316T", groups[kind])?;
        if found.is_empty() {
            continue;
        }
        if found.len() != 1 {
            return Err("Duplicate RADIAS template group".into());
        }
        let group = &found[0];
        let mut cursor = 0;
        while cursor < group.len() {
            if group.len() - cursor < 32
                || !magic(group, cursor, b"316t")
                || !magic(group, cursor + 4, kinds[kind])
            {
                return Err("Invalid nested RADIAS template kind".into());
            }
            let header = le32(group, cursor + 8)? as usize;
            let size = le32(group, cursor + 12)? as usize;
            let offset = le32(group, cursor + 16)? as usize;
            let length = le32(group, cursor + 20)? as usize;
            let end = cursor
                .checked_add(header)
                .and_then(|n| n.checked_add(size))
                .ok_or("template bounds overflow")?;
            if header != 32
                || offset != 96
                || length != sizes[kind]
                || offset + length != size
                || end > group.len()
            {
                return Err("Unsupported RADIAS template framing or payload size".into());
            }
            out.templates[kind].push(group[cursor + header + offset..end].to_vec());
            cursor = end;
        }
        if out.templates[kind].len() != counts[kind] {
            return Err("Incomplete RADIAS template bank".into());
        }
    }
    out.formants = records(data, b"316F", b"316f")?;
    if !out.formants.is_empty() && out.formants.len() != 16 {
        return Err("Incomplete RADIAS Formant Motion bank".into());
    }
    for r in &out.formants {
        formant_frames(r)?;
    }
    Ok(out)
}
pub fn native_usr_flash(data: &[u8], extras: &BackupExtras) -> Result<Vec<u8>, String> {
    let programs = records(data, b"316P", b"316p")?;
    let drums = records(data, b"316D", b"316d")?;
    if programs.len() != 256 || drums.len() != 32 {
        return Err("Library import requires 256 programs and 32 drums".into());
    }
    let mut flash = vec![255; 0x100000];
    for (i, r) in programs.iter().enumerate() {
        if r.len() != 0x900 {
            return Err("Unsupported RADIAS program record size".into());
        }
        let a = (i / 36) * 0x10000 + 0x10 + (i % 36) * 0x71c;
        flash[a..a + 0x71c].copy_from_slice(&r[..0x71c]);
    }
    for (i, r) in drums.iter().enumerate() {
        if r.len() != 0x860 {
            return Err("Unsupported RADIAS drum record size".into());
        }
        let a = 0x72000 + i * 0x700;
        flash[a..a + 0x700].copy_from_slice(&r[..0x700]);
    }
    for kind in 0..3 {
        for (i, r) in extras.templates[kind].iter().enumerate() {
            let start = [0x80000, 0x88000, 0x8c000][kind];
            let stride = [0x100, 0x40, 0x30][kind];
            if i >= 128 || r.len() > stride {
                return Err("Invalid native RADIAS template bank".into());
            }
            let a = start + i * stride;
            flash[a..a + r.len()].copy_from_slice(r);
        }
    }
    if !extras.formants.is_empty() && extras.formants.len() != 16 {
        return Err("Incomplete RADIAS Formant Motion bank".into());
    }
    for (i, r) in extras.formants.iter().enumerate() {
        let frames = formant_frames(r)?;
        let a = 0xd0000 + i * 0x3000;
        flash[a..a + 4].copy_from_slice(&(frames as u32).to_be_bytes());
        flash[a + 4..a + 4 + r.len() - 16].copy_from_slice(&r[16..]);
    }
    Ok(flash)
}
