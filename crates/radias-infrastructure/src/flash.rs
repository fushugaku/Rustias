//! Persist NOR cells atomically; this is not a running CPU/board snapshot.
use crate::artifacts::ArtifactPaths;
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub const FLASH_BYTES: usize = 0x400000;
pub fn load(path: &Path) -> Result<Vec<u8>, String> {
    let cells = fs::read(path).map_err(|e| format!("Cannot read Flash image: {e}"))?;
    if cells.len() != FLASH_BYTES {
        return Err("Flash image must contain exactly 4194304 bytes".into());
    }
    Ok(cells)
}
pub fn save(path: &Path, cells: &[u8], paths: &ArtifactPaths) -> Result<(), String> {
    if cells.len() != FLASH_BYTES {
        return Err("Invalid Flash array capacity".into());
    }
    paths.allow_write(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut name: OsString = path
        .file_name()
        .ok_or("Flash image needs a filename")?
        .to_os_string();
    name.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let temporary = parent.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| format!("Cannot stage Flash image: {e}"))?;
    let result = (|| {
        file.write_all(cells)
            .map_err(|e| format!("Cannot write Flash image: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Cannot flush Flash image: {e}"))?;
        drop(file);
        paths.allow_write(path)?;
        fs::rename(&temporary, path).map_err(|e| format!("Cannot replace Flash image: {e}"))
    })(); // Keep old cells intact on all failed writes/rechecks/renames.
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
