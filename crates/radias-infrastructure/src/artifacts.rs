//! Preserve firmware, banks and external inputs when writing diagnostics.
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
struct Identity {
    path: PathBuf,
    #[cfg(unix)]
    inode: Option<(u64, u64)>,
}
impl Identity {
    fn of(path: &Path) -> Result<Self, String> {
        let canonical = if path.exists() {
            path.canonicalize().map_err(|e| e.to_string())?
        } else {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            parent
                .canonicalize()
                .map_err(|e| e.to_string())?
                .join(path.file_name().ok_or("Output needs a filename")?)
        };
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
        };
        Ok(Self {
            path: canonical,
            #[cfg(unix)]
            inode,
        })
    }
    fn same(&self, other: &Self) -> bool {
        if self.path == other.path {
            return true;
        }
        #[cfg(unix)]
        if self.inode.is_some() && self.inode == other.inode {
            return true;
        }
        false
    }
}

pub struct ArtifactPaths {
    inputs: Vec<Identity>,
    outputs: Vec<Identity>,
}
impl ArtifactPaths {
    pub fn new(inputs: &[PathBuf], outputs: &[PathBuf]) -> Result<Self, String> {
        let mut paths = Self {
            inputs: inputs
                .iter()
                .map(|p| Identity::of(p))
                .collect::<Result<_, _>>()?,
            outputs: Vec::new(),
        };
        for path in outputs {
            let identity = paths.check_write(path)?;
            if paths.outputs.iter().any(|p| p.same(&identity)) {
                return Err(format!(
                    "Output target aliases a source or capture file: {}",
                    path.display()
                ));
            }
            paths.outputs.push(identity);
        }
        Ok(paths)
    }
    fn check_write(&self, path: &Path) -> Result<Identity, String> {
        let identity = Identity::of(path)?;
        if self.inputs.iter().any(|p| p.same(&identity)) {
            return Err(format!(
                "Output target aliases a source file: {}",
                path.display()
            ));
        }
        Ok(identity)
    }
    pub fn allow_write(&self, path: &Path) -> Result<(), String> {
        self.check_write(path).map(|_| ())
    }
    pub fn allow_extra_output(&self, path: &Path) -> Result<(), String> {
        let identity = self.check_write(path)?;
        if self.outputs.iter().any(|p| p.same(&identity)) {
            return Err(format!(
                "Output target aliases a source or capture file: {}",
                path.display()
            ));
        }
        Ok(())
    }
    pub fn protect_input(&mut self, path: &Path) -> Result<(), String> {
        let identity = Identity::of(path)?;
        if self.outputs.iter().any(|p| p.same(&identity)) {
            return Err(format!(
                "Input aliases an active output: {}",
                path.display()
            ));
        }
        self.inputs.push(identity);
        Ok(())
    }
}
pub fn same_file(first: &Path, second: &Path) -> Result<bool, String> {
    Ok(Identity::of(first)?.same(&Identity::of(second)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "radias-artifacts-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
        fn file(&self, name: &str) -> PathBuf {
            let p = self.0.join(name);
            fs::write(&p, b"original").unwrap();
            p
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn source_and_output_aliases_are_rejected_without_writes() {
        let t = Temp::new();
        let original = t.file("sys.bin");
        assert!(ArtifactPaths::new(&[original.clone()], &[original.clone()]).is_err());
        assert_eq!(fs::read(&original).unwrap(), b"original");
        let output = t.0.join("capture.wav");
        assert!(ArtifactPaths::new(&[], &[output.clone(), output.clone()]).is_err());
        assert!(!output.exists());
    }
    #[test]
    fn newly_loaded_wave_cannot_be_an_active_output() {
        let t = Temp::new();
        let wave = t.file("input.wav");
        let out = t.0.join("dry.wav");
        let mut guard = ArtifactPaths::new(&[], &[out.clone()]).unwrap();
        guard.protect_input(&wave).unwrap();
        assert!(guard.allow_write(&wave).is_err());
        fs::write(&out, b"capture").unwrap();
        assert!(guard.protect_input(&out).is_err());
        guard.allow_write(&out).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlink_and_hardlink_preserve_original() {
        let t = Temp::new();
        let original = t.file("sys.bin");
        let symbolic = t.0.join("symbol.bin");
        std::os::unix::fs::symlink(&original, &symbolic).unwrap();
        let hard = t.0.join("hard.bin");
        fs::hard_link(&original, &hard).unwrap();
        for alias in [symbolic, hard] {
            assert!(ArtifactPaths::new(&[original.clone()], &[alias]).is_err());
        }
        assert_eq!(fs::read(original).unwrap(), b"original");
    }
}
