//! Native program value object, lossless and independent of devices/UI.
pub const PROGRAM_BYTES: usize = 1790;
#[derive(Clone)]
pub struct Program {
    raw: [u8; PROGRAM_BYTES],
}
impl Program {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let raw = bytes
            .try_into()
            .map_err(|_| "Expected 1790 native program bytes")?;
        Ok(Self { raw })
    }
    pub fn bytes(&self) -> &[u8; PROGRAM_BYTES] {
        &self.raw
    }
    /// Local keyboard performance reaches each switched-on timbre's original
    /// receive channel and key window. External MIDI remains unmodified.
    pub fn keyboard_routes(&self, note: u8, global_channel: u8) -> Vec<(u8, u8)> {
        let mut routes = Vec::new();
        for index in 0..4 {
            let base = 48 + 228 * index;
            if self.raw[base] & 128 == 0 {
                continue;
            }
            let channel = if self.raw[base + 4] < 16 {
                self.raw[base + 4]
            } else {
                global_channel & 15
            };
            if self.raw[base + 6] <= note && note <= self.raw[base + 7] && note < 128 {
                routes.push((channel, note));
            }
        }
        routes.sort_unstable();
        routes.dedup();
        routes
    }
    /// A documented audition gesture, not a modification of notes/sequence
    /// data in the program. Include drum assignments and PCM timbres as stored.
    pub fn audition_routes(&self, global_channel: u8) -> Result<Vec<(u8, u8)>, String> {
        let mut routes = Vec::new();
        let sequencer = self.raw[1062] & 128 != 0;
        for index in 0..4 {
            let base = 48 + 228 * index;
            if self.raw[base] & 128 == 0 {
                continue;
            }
            let channel = if self.raw[base + 4] < 16 {
                self.raw[base + 4]
            } else {
                global_channel & 15
            };
            let (lo, hi) = (self.raw[base + 6], self.raw[base + 7]);
            if lo > hi || hi > 127 {
                return Err(format!("Invalid key window in timbre {}", index + 1));
            }
            let candidates: &[u8] = if sequencer && (self.raw[base] >> 2) & 3 != 0 {
                &[60]
            } else {
                &[48, 55, 60]
            };
            for &note in candidates {
                routes.push((channel, note.clamp(lo, hi)));
            }
        }
        routes.sort_unstable();
        routes.dedup();
        Ok(routes)
    }
    pub fn name(&self) -> String {
        String::from_utf8_lossy(&self.raw[..12]).trim().to_string()
    }
    pub fn set_byte(&mut self, offset: usize, value: u8) -> Result<(), String> {
        let target = self
            .raw
            .get_mut(offset)
            .ok_or("Invalid program parameter offset")?;
        *target = value;
        Ok(())
    }
    pub fn set_bits(
        &mut self,
        offset: usize,
        mask: u8,
        shift: u8,
        value: u8,
    ) -> Result<(), String> {
        if shift >= 8 || (value as u16) << shift & !(mask as u16) != 0 {
            return Err("Parameter value exceeds encoded field".into());
        }
        let target = self
            .raw
            .get_mut(offset)
            .ok_or("Invalid program parameter offset")?;
        *target = (*target & !mask) | (value << shift);
        Ok(())
    }
    pub fn sysex(&self, channel: u8) -> Result<Vec<u8>, String> {
        if channel > 15 {
            return Err("Invalid MIDI channel".into());
        }
        let mut out = vec![0xf0, 0x42, 0x30 | channel, 0x72, 0x40];
        for block in self.raw.chunks(7) {
            let mask = block
                .iter()
                .enumerate()
                .fold(0, |mask, (i, v)| mask | ((v >> 7) << i));
            out.push(mask);
            out.extend(block.iter().map(|v| v & 127));
        }
        out.push(0xf7);
        Ok(out)
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    fn fixture() -> Program {
        let mut raw = [0; PROGRAM_BYTES];
        for i in 0..4 {
            let base = 48 + i * 228;
            raw[base] = 128;
            raw[base + 4] = [2, 2, 16, 7][i];
            raw[base + 6] = [0, 40, 64, 72][i];
            raw[base + 7] = [63, 63, 71, 127][i];
        }
        Program::from_bytes(&raw).unwrap()
    }
    #[test]
    fn keyboard_preserves_channels_splits_and_deduplicates_layers() {
        let p = fixture();
        assert_eq!(p.keyboard_routes(60, 9), [(2, 60)]);
        assert_eq!(p.keyboard_routes(67, 9), [(9, 67)]);
        assert_eq!(p.keyboard_routes(80, 9), [(7, 80)]);
        assert!(p.keyboard_routes(128, 9).is_empty());
    }
    #[test]
    fn audition_reaches_all_enabled_timbres_without_changing_payload() {
        let p = fixture();
        let before = *p.bytes();
        assert_eq!(
            p.audition_routes(9).unwrap(),
            [(2, 48), (2, 55), (2, 60), (7, 72), (9, 64)]
        );
        assert_eq!(p.bytes(), &before);
    }
    #[test]
    fn sequenced_timbre_receives_one_trigger_and_disabled_timbre_none() {
        let mut p = fixture();
        p.set_byte(1062, 128).unwrap();
        p.set_byte(48, 132).unwrap();
        p.set_byte(48 + 228, 0).unwrap();
        assert_eq!(p.audition_routes(9).unwrap(), [(2, 60), (7, 72), (9, 64)]);
    }
    #[test]
    fn invalid_window_cannot_be_presented_as_successful_audition() {
        let mut p = fixture();
        p.set_byte(48 + 6, 100).unwrap();
        assert!(p.audition_routes(9).is_err());
    }
}
