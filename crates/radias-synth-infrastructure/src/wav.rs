use radias_synth_domain::{SAMPLE_RATE, Sample};
use std::{
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
};

/// Preserve signed sample words at the original 48 kHz boundary.
pub fn write_mono(path: &Path, samples: &[Sample]) -> io::Result<()> {
    let length = samples
        .len()
        .checked_mul(4)
        .filter(|n| *n <= (u32::MAX - 36) as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WAV exceeds RIFF length"))?
        as u32;
    let mut output = BufWriter::new(File::create(path)?);
    output.write_all(b"RIFF")?;
    output.write_all(&(length + 36).to_le_bytes())?;
    output.write_all(b"WAVEfmt ")?;
    output.write_all(&16u32.to_le_bytes())?;
    output.write_all(&1u16.to_le_bytes())?;
    output.write_all(&1u16.to_le_bytes())?;
    output.write_all(&SAMPLE_RATE.to_le_bytes())?;
    output.write_all(&(SAMPLE_RATE * 4).to_le_bytes())?;
    output.write_all(&4u16.to_le_bytes())?;
    output.write_all(&32u16.to_le_bytes())?;
    output.write_all(b"data")?;
    output.write_all(&length.to_le_bytes())?;
    for sample in samples {
        output.write_all(&sample.0.to_le_bytes())?;
    }
    output.flush()
}

pub fn write_buses(path: &Path, frames: &[[Sample; 8]]) -> io::Result<()> {
    let length = frames
        .len()
        .checked_mul(32)
        .filter(|n| *n <= (u32::MAX - 36) as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WAV exceeds RIFF length"))?
        as u32;
    let mut output = BufWriter::new(File::create(path)?);
    output.write_all(b"RIFF")?;
    output.write_all(&(length + 36).to_le_bytes())?;
    output.write_all(b"WAVEfmt ")?;
    output.write_all(&16u32.to_le_bytes())?;
    output.write_all(&1u16.to_le_bytes())?;
    output.write_all(&8u16.to_le_bytes())?;
    output.write_all(&SAMPLE_RATE.to_le_bytes())?;
    output.write_all(&(SAMPLE_RATE * 32).to_le_bytes())?;
    output.write_all(&32u16.to_le_bytes())?;
    output.write_all(&32u16.to_le_bytes())?;
    output.write_all(b"data")?;
    output.write_all(&length.to_le_bytes())?;
    for frame in frames {
        for sample in frame {
            output.write_all(&sample.0.to_le_bytes())?;
        }
    }
    output.flush()
}

/// Production dry stereo words, without monitoring gain or float conversion.
pub fn write_stereo(path: &Path, frames: &[[Sample; 2]]) -> io::Result<()> {
    let length = frames
        .len()
        .checked_mul(8)
        .filter(|n| *n <= (u32::MAX - 36) as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "WAV exceeds RIFF length"))?
        as u32;
    let mut output = BufWriter::new(File::create(path)?);
    output.write_all(b"RIFF")?;
    output.write_all(&(length + 36).to_le_bytes())?;
    output.write_all(b"WAVEfmt ")?;
    output.write_all(&16u32.to_le_bytes())?;
    output.write_all(&1u16.to_le_bytes())?;
    output.write_all(&2u16.to_le_bytes())?;
    output.write_all(&SAMPLE_RATE.to_le_bytes())?;
    output.write_all(&(SAMPLE_RATE * 8).to_le_bytes())?;
    output.write_all(&8u16.to_le_bytes())?;
    output.write_all(&32u16.to_le_bytes())?;
    output.write_all(b"data")?;
    output.write_all(&length.to_le_bytes())?;
    for frame in frames {
        for sample in frame {
            output.write_all(&sample.0.to_le_bytes())?;
        }
    }
    output.flush()
}
