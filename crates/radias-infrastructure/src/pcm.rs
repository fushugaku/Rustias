use radias_domain::board::nor::SIZE;
pub const OFFSET: usize = 0x1e0000;
pub const CAPACITY: usize = SIZE - OFFSET;
pub struct NativePcmBank {
    pub bytes: Vec<u8>,
    pub table_counts: [u32; 5],
    pub from_full_flash: bool,
}
impl NativePcmBank {
    fn be32(&self, a: usize) -> Result<u32, String> {
        let source = self
            .bytes
            .get(a..a.saturating_add(4))
            .ok_or("Truncated PCM integer")?;
        Ok(u32::from_be_bytes(source.try_into().unwrap()))
    }
    fn section(&self, pointer: usize, magic: Option<&[u8; 4]>) -> Result<usize, String> {
        let a = self.be32(pointer)? as usize;
        if a < 0x30 || a > self.bytes.len() || self.bytes.len() - a < 0x20 {
            return Err("PCM section header is outside the supplied bank".into());
        }
        if magic.is_some_and(|m| self.bytes.get(a..a + 4) != Some(m)) {
            return Err("PCM DRUM/SMPL section signature does not match the native loader".into());
        }
        Ok(a)
    }
    pub fn parse(input: Vec<u8>) -> Result<Self, String> {
        let full = input.len() == SIZE;
        let bytes = if full {
            input[OFFSET..].to_vec()
        } else {
            input
        };
        if bytes.len() < 0x30 || bytes.len() > CAPACITY {
            return Err("PCM bank must fit the original 001e0000..003fffff region".into());
        }
        let mut out = Self {
            bytes,
            table_counts: [0; 5],
            from_full_flash: full,
        };
        if &out.bytes[..12] != b"KORGX3160PCM" {
            return Err("Expected a KORG/X3160PCM container, not WAVE/ESX data".into());
        }
        for i in 0..5 {
            let pointer = [0x10, 0x18, 0x1c, 0x20, 0x28][i];
            if i == 4 && out.be32(pointer)? == u32::MAX {
                continue;
            }
            let a = out.section(pointer, None)?;
            let count = out.be32(a + 8)?;
            if count > [256, 256, 1024, 1024, 256][i]
                || count as usize * [20, 16, 8, 12, 16][i] > out.bytes.len() - a - 0x20
            {
                return Err(
                    "PCM descriptor count exceeds native limits or supplied table bytes".into(),
                );
            }
            out.table_counts[i] = count;
        }
        out.section(0x14, Some(b"DRUM"))?;
        out.section(0x24, Some(b"SMPL"))?;
        Ok(out)
    }
    pub fn mount(&self, flash: &mut [u8]) -> Result<(), String> {
        if self.bytes.is_empty() || flash.len() != SIZE {
            return Err("Cannot mount PCM bank in an incomplete physical Flash array".into());
        }
        flash[OFFSET..].fill(255);
        flash[OFFSET..OFFSET + self.bytes.len()].copy_from_slice(&self.bytes);
        Ok(())
    }
}
