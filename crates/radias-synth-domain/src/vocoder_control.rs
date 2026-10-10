//! Stored vocoder controls compiled by SYS 03b040 and its called algorithms.
//! Immutable tables and current modulation inputs arrive through data ports.
use crate::vocoder::PARAMETER_WORDS;
pub const STORED_BYTES: usize = 78;
#[derive(Clone, Copy)]
pub struct VocoderProgram<'a> {
    pub bytes: &'a [u8; STORED_BYTES],
}
pub struct VocoderControlTables<'a> {
    pub initial: &'a [u16; PARAMETER_WORDS],
    pub release: &'a [u32; 128],
    pub gate: &'a [u32; 128],
    pub damping: &'a [u32; 128],
    pub envelope_attack: &'a [u32; 129],
    pub envelope_release: &'a [u32; 129],
    pub frequencies: &'a [u16; 1536],
    pub resonance_gain: &'a [u16; 128],
    pub pans: &'a [u16; 128],
    pub linear: &'a [u16; 128],
    pub bipolar: &'a [u16; 128],
    pub depth: &'a [u16; 128],
    pub pitch_depth: &'a [u16; 128],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VocoderControlInputs {
    /// Common byte zero of the currently selected carrier timbre.
    pub carrier_flags: u8,
    /// Original selected frequency-modulation source's signed high word.
    pub frequency_source: i16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VocoderControlError {
    FormantShift,
}
fn put(words: &mut [u16], at: usize, value: u32) {
    words[at] = (value >> 16) as u16;
    words[at + 1] = value as u16;
}
fn level(value: u8) -> u16 {
    let value = u16::from(value & 127);
    2 * value * value
}
fn signed_clamp(value: i32) -> i16 {
    value.clamp(-32767, 32767) as i16
}
impl VocoderProgram<'_> {
    pub fn enabled(self) -> bool {
        self.bytes[0] & 128 != 0
    }
    pub fn carrier_timbre(self) -> usize {
        const SELECT: [usize; 16] = [0, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0];
        SELECT[usize::from(self.bytes[45] & 15)]
    }
    /// SYS03ba64/03baa8: convert the stored modulation intensity according
    /// to its source family, including signed saturation before publication.
    pub fn frequency_depth(self, tables: &VocoderControlTables<'_>) -> i16 {
        let at = usize::from(self.bytes[43] & 127);
        let value = match self.bytes[40] & 15 {
            0..=2 | 9 | 15 => i32::from(tables.linear[at] as i16),
            3 | 4 => i32::from(tables.bipolar[at] as i16),
            8 => i32::from(tables.pitch_depth[at] as i16),
            _ => i32::from(tables.depth[at] as i16) << 3,
        };
        signed_clamp(value)
    }
    /// Whole 03bb10 arithmetic after the selected source callback returns.
    pub fn frequency_offset(self, source: i16, tables: &VocoderControlTables<'_>) -> i16 {
        let depth = self.frequency_depth(tables);
        let product = i32::from(source) * i32::from(depth);
        let base = (i32::from(self.bytes[41] & 127) - 64) << 17;
        let modulated = product.wrapping_add(base) >> 16;
        let index = ((modulated.clamp(-127, 127) >> 1) + 64) as usize;
        signed_clamp(i32::from(tables.linear[index] as i16))
    }
    /// Compile target publications from the actual stored fields. Smoothing
    /// currents and filter histories are initialized separately by opcode35.
    pub fn compile_targets(
        self,
        inputs: VocoderControlInputs,
        tables: &VocoderControlTables<'_>,
    ) -> Result<[u16; PARAMETER_WORDS], VocoderControlError> {
        let b = self.bytes;
        let mut p = *tables.initial;
        p[0] = u16::from(self.enabled());
        let modulator = usize::from(b[0] & 3);
        let auxiliary = usize::from((b[0] >> 2) & 3);
        let input_muted = inputs.carrier_flags & 0x30 == 0x10;
        const MODULATOR: [[u16; 2]; 4] = [[0, 0], [2, 4], [2, 2], [4, 4]];
        const AUXILIARY: [[u16; 2]; 4] = [[0, 0], [4, 6], [4, 4], [6, 6]];
        p[1..3].copy_from_slice(&MODULATOR[modulator]);
        p[3] = if modulator != 0 && input_muted {
            0
        } else {
            32767
        };
        put(&mut p, 6, tables.release[usize::from(b[1] & 127)]);
        put(&mut p, 8, tables.gate[usize::from(b[2] & 127)]);
        let formant = b[0] & 64 != 0;
        p[15] = if formant {
            2
        } else {
            u16::from(b[44] & 127 == 127)
        };
        let sensitivity = if formant {
            128
        } else {
            usize::from(b[44] & 127)
        };
        put(&mut p, 0xb0, tables.envelope_attack[sensitivity]);
        put(&mut p, 0xb2, tables.envelope_release[sensitivity]);
        let timbre = self.carrier_timbre();
        p[0xb4] = 8 + 4 * timbre as u16;
        p[0xb5] = p[0xb4] + 2;
        p[0xb6..0xb8].copy_from_slice(&AUXILIARY[auxiliary]);
        p[0xb8] = level(b[5]);
        p[0xba] = level(b[6]);
        p[0xbc] = if auxiliary != 0 && input_muted {
            0
        } else {
            32767
        };
        const SHIFT: [i16; 5] = [0, 2, 4, -2, -4];
        p[0xc1] = *SHIFT
            .get(usize::from((b[40] >> 4) & 7))
            .ok_or(VocoderControlError::FormantShift)? as u16;
        p[0xbf] = self.frequency_offset(inputs.frequency_source, tables) as u16;
        let resonance = usize::from(b[42] & 127);
        p[0xf2] = tables.resonance_gain[resonance];
        put(&mut p, 0xf4, tables.damping[resonance]);
        let bank = resonance / 4 * 48;
        p[0xc2..0xf2].copy_from_slice(&tables.frequencies[bank..bank + 48]);
        for band in 0..16 {
            p[0x118 + 4 * band] = level(b[9 + 2 * band]);
            p[0x11a + 4 * band] = tables.pans[usize::from(b[8 + 2 * band] & 127)];
        }
        // Initial carrier-note mask is empty (03b198). Key-gated HPF level
        // changes on note events through the separate controller publication.
        p[0x158] = if b[0] & 0x60 == 0 { level(b[3]) } else { 0 };
        p[0x15a] = level(b[4]);
        p[0x15c] = level(b[7]);
        Ok(p)
    }
}
