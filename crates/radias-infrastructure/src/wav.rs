use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};
#[derive(Default)]
pub struct WaveInput {
    pub frames: Vec<[u32; 2]>,
    pub path: String,
    pub channels: u32,
    pub bits: u32,
    pub position: usize,
    pub delivered: u64,
    pub looping: bool,
}
impl WaveInput {
    pub fn next(&mut self) -> [u32; 2] {
        self.delivered += 1;
        if self.position == self.frames.len() && self.looping && !self.frames.is_empty() {
            self.position = 0;
        }
        if self.position < self.frames.len() {
            let frame = self.frames[self.position];
            self.position += 1;
            frame
        } else {
            [0; 2]
        }
    }
    pub fn rewind(&mut self) {
        self.position = 0;
        self.delivered = 0;
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        Self::parse(&data, path.to_string_lossy().into_owned())
    }
    pub fn parse(data: &[u8], path: String) -> Result<Self, String> {
        fn little(data: &[u8], offset: usize, size: usize) -> Result<u32, String> {
            let source = data
                .get(offset..offset.checked_add(size).ok_or("WAVE integer overflow")?)
                .ok_or("Input WAVE data is truncated")?;
            let mut v = 0;
            for (i, &b) in source.iter().enumerate() {
                v |= (b as u32) << (i * 8);
            }
            Ok(v)
        }
        if data.len() < 12 {
            return Err("Input WAVE header is truncated".into());
        }
        if &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
            return Err("Expected a RIFF WAVE input".into());
        }
        let end = little(data, 4, 4)? as usize + 8;
        if end < 12 || end > data.len() {
            return Err("Input WAVE RIFF size is invalid".into());
        }
        let mut format = None;
        let mut audio = None;
        let mut at = 12;
        while at < end {
            if end - at < 8 {
                return Err("Input WAVE chunk header is truncated".into());
            }
            let size = little(data, at + 4, 4)? as usize;
            let payload = at + 8;
            let next = payload
                .checked_add(size)
                .and_then(|v| v.checked_add(size & 1))
                .ok_or("WAVE chunk overflow")?;
            if next > end {
                return Err("Input WAVE chunk exceeds the RIFF boundary".into());
            }
            if &data[at..at + 4] == b"fmt " {
                if format.is_some() || size < 16 {
                    return Err("Input WAVE format chunk is invalid".into());
                }
                format = Some(&data[payload..payload + size]);
            } else if &data[at..at + 4] == b"data" {
                if audio.is_some() {
                    return Err("Multiple input WAVE data chunks are unsupported".into());
                }
                audio = Some(&data[payload..payload + size]);
            }
            at = next;
        }
        let fmt = format.ok_or("Input WAVE requires format and data chunks")?;
        let audio = audio.ok_or("Input WAVE requires format and data chunks")?;
        let mut tag = little(fmt, 0, 2)?;
        let channels = little(fmt, 2, 2)?;
        let rate = little(fmt, 4, 4)?;
        let align = little(fmt, 12, 2)?;
        let bits = little(fmt, 14, 2)?;
        let mut valid = bits;
        if tag == 0xfffe {
            if fmt.len() < 40
                || little(fmt, 16, 2)? < 22
                || fmt[28..40] != [0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71]
            {
                return Err("Unsupported extensible input WAVE format".into());
            }
            tag = little(fmt, 24, 4)?;
            valid = little(fmt, 18, 2)?;
        }
        if rate != 48000 || !matches!(channels, 1 | 2) {
            return Err("Input WAVE must have one or two channels at 48000 Hz".into());
        }
        if !matches!(tag, 1 | 3)
            || !matches!(bits, 16 | 24 | 32)
            || (tag == 3 && bits != 32)
            || valid == 0
            || valid > bits
            || (tag == 3 && valid != 32)
        {
            return Err("Input WAVE must use integer 16/24/32-bit or IEEE float32 samples".into());
        }
        let width = bits / 8;
        if align != channels * width
            || little(fmt, 8, 4)? != rate * align
            || audio.len() % align as usize != 0
        {
            return Err("Input WAVE frame layout is invalid".into());
        }
        let mut out = Self {
            path,
            channels,
            bits,
            ..Default::default()
        };
        for block in audio.chunks_exact(align as usize) {
            let mut frame = [0; 2];
            for channel in 0..channels {
                let mut raw = little(block, (channel * width) as usize, width as usize)?;
                frame[channel as usize] = if tag == 3 {
                    let value = f32::from_bits(raw);
                    if !value.is_finite() {
                        return Err("Input WAVE contains a non-finite sample".into());
                    }
                    let sample = if value >= 1.0 {
                        8388607
                    } else if value <= -1.0 {
                        -8388608
                    } else {
                        ((value as f64) * 8388608.0).floor() as i64
                    };
                    (sample as u32).wrapping_mul(256)
                } else {
                    if valid < bits {
                        raw &= !((1u32 << (bits - valid)) - 1);
                    }
                    raw << (32 - bits) & 0xffffff00
                };
            }
            out.frames.push(frame);
        }
        Ok(out)
    }
}
pub fn write_pcm32<const N: usize>(path: &Path, frames: &[[i32; N]]) -> Result<(), String> {
    if N == 0 || N > 65535 / 4 {
        return Err("Invalid PCM32 block alignment".into());
    }
    let length = frames
        .len()
        .checked_mul(4 * N)
        .filter(|&n| n <= u32::MAX as usize - 36)
        .ok_or("Dry capture exceeds RIFF size")? as u32;
    let mut out =
        BufWriter::new(File::create(path).map_err(|e| format!("Cannot write audio capture: {e}"))?);
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(length + 36).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&(N as u16).to_le_bytes());
    header.extend_from_slice(&48000u32.to_le_bytes());
    header.extend_from_slice(&(48000 * 4 * N as u32).to_le_bytes());
    header.extend_from_slice(&(4 * N as u16).to_le_bytes());
    header.extend_from_slice(&32u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&length.to_le_bytes());
    out.write_all(&header).map_err(|e| e.to_string())?;
    for frame in frames {
        for &v in frame {
            out.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
        }
    }
    out.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod input_tests {
    use super::*;
    fn wave(tag: u16, bits: u16, channels: u16, samples: &[u8], valid: Option<u16>) -> Vec<u8> {
        let align = channels * bits / 8;
        let mut fmt = Vec::new();
        for v in [if valid.is_some() { 0xfffe } else { tag }, channels] {
            fmt.extend_from_slice(&v.to_le_bytes());
        }
        fmt.extend_from_slice(&48000u32.to_le_bytes());
        fmt.extend_from_slice(&(48000u32 * align as u32).to_le_bytes());
        fmt.extend_from_slice(&align.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        if let Some(valid) = valid {
            fmt.extend_from_slice(&22u16.to_le_bytes());
            fmt.extend_from_slice(&valid.to_le_bytes());
            fmt.extend_from_slice(&0u32.to_le_bytes());
            fmt.extend_from_slice(&(tag as u32).to_le_bytes());
            fmt.extend_from_slice(&[0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71]);
        }
        let mut body = b"WAVEfmt ".to_vec();
        body.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        body.extend(fmt);
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        body.extend_from_slice(samples);
        if samples.len() & 1 != 0 {
            body.push(0);
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }
    #[test]
    fn integer_widths_preserve_native_adc_sign_and_padding() {
        for (bits, data, expected) in [
            (
                16,
                vec![0, 0x80, 0xff, 0x7f],
                vec![[0x80000000, 0], [0x7fff0000, 0]],
            ),
            (
                24,
                vec![0xff, 0xff, 0xff, 1, 0, 0],
                vec![[0xffffff00, 0], [0x100, 0]],
            ),
            (
                32,
                vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f],
                vec![[0xffffff00, 0], [0x7fffff00, 0]],
            ),
        ] {
            assert_eq!(
                WaveInput::parse(&wave(1, bits, 1, &data, None), String::new())
                    .unwrap()
                    .frames,
                expected
            );
        }
    }
    #[test]
    fn float_clamp_and_negative_floor_match_adc_contract() {
        let samples = [1.5f32, -1.5, 0.5, -0.1];
        let data = samples
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<_>>();
        let w = WaveInput::parse(&wave(3, 32, 2, &data, None), String::new()).unwrap();
        assert_eq!(
            w.frames,
            [
                [0x7fffff00, 0x80000000],
                [0x40000000, (-838861i32 as u32) << 8]
            ]
        );
    }
    #[test]
    fn extensible_valid_bits_ignore_declared_padding() {
        let w = WaveInput::parse(
            &wave(
                1,
                32,
                2,
                &[0xff, 0x34, 0x12, 0x7f, 0xab, 0xff, 0xff, 0xff],
                Some(24),
            ),
            String::new(),
        )
        .unwrap();
        assert_eq!(w.frames, [[0x7f123400, 0xffffff00]]);
    }
    #[test]
    fn loop_eof_and_rewind_are_exact_and_count_deliveries() {
        let mut w = WaveInput::parse(&wave(1, 16, 1, &[1, 0, 2, 0], None), String::new()).unwrap();
        assert_eq!(w.next(), [0x10000, 0]);
        assert_eq!(w.next(), [0x20000, 0]);
        assert_eq!(w.next(), [0, 0]);
        assert_eq!(w.delivered, 3);
        w.looping = true;
        assert_eq!(w.next(), [0x10000, 0]);
        w.rewind();
        assert_eq!((w.position, w.delivered), (0, 0));
        assert_eq!(w.next(), [0x10000, 0]);
    }
    #[test]
    fn wrong_rate_malformed_frame_nonfinite_and_truncation_fail() {
        let good = wave(1, 16, 2, &[0, 0, 0, 0], None);
        let mut bad = good.clone();
        bad[24..28].copy_from_slice(&44100u32.to_le_bytes());
        assert!(WaveInput::parse(&bad, String::new()).is_err());
        assert!(WaveInput::parse(&wave(1, 24, 2, &[0, 0, 0], None), String::new()).is_err());
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(
                WaveInput::parse(&wave(3, 32, 1, &v.to_le_bytes(), None), String::new()).is_err()
            );
        }
        for length in [0, 11, good.len() - 1] {
            assert!(WaveInput::parse(&good[..length], String::new()).is_err());
        }
    }
}
