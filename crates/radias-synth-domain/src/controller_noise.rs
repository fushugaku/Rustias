//! Original SYS0207B2 /020880 CTRL1 transfers, after common composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormantControlTarget {
    pub level: i16,
    pub feedback: i16,
}

/// Original SYS020E28 chooses the Formant counter by physical controller slot.
pub struct FormantCounterSeeds {
    pub values: [i16; 32],
}
impl FormantCounterSeeds {
    pub fn for_slot(&self, slot: u8) -> i16 {
        self.values[(slot & 31) as usize]
    }
}

/// Native control values passed to the original Noise/Formant DSP compilers.
/// Pitch and CTRL1 shadows are distinct inputs; CTRL2 does not scale CTRL1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoiseControl {
    pub control2: u8,
    pub control2_modulation: i16,
    pub control2_manual_offset: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColoredNoiseControl {
    pub color: i16,
    pub frequency: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormantCompilerControl {
    pub shape: u32,
    pub input_gain: i16,
    pub frequency: i16,
}

impl NoiseControl {
    fn code(self) -> i32 {
        (i32::from(self.control2 & 127)
            + i32::from(self.control2_modulation)
            + i32::from(self.control2_manual_offset))
        .clamp(0, 127)
    }

    /// SYS020CB8: CTRL1 color shadow and the independent CTRL2 transfer.
    pub fn colored(self, color: i16) -> ColoredNoiseControl {
        ColoredNoiseControl {
            color,
            frequency: ((self.code() - 64) * 78 + 10813).clamp(0, 32767) as i16,
        }
    }

    /// SYS020D48: shape pair, pitch-dependent input gain and CTRL2 frequency.
    pub fn formant(self, target: FormantControlTarget, pitch: i16) -> FormantCompilerControl {
        let level = i32::from(target.level);
        // SH mulu.w uses the low 16 bits even when a signed shadow is negative.
        let inverse = (32767 - level) as u16;
        let upper = ((u32::from(inverse) * 0x17ae) >> 16) + 0xf5;
        let inverse_pitch = (32767 - i32::from(pitch)) as u16;
        let square = (u32::from(inverse_pitch) * u32::from(inverse_pitch)) >> 15;
        // The following SH mulu.w narrows the squared accumulator to its
        // unsigned low word. Signed pitch shadows can make the square wider.
        let input = ((u32::from(target.level as u16).wrapping_mul(u32::from(square as u16)) >> 15)
            + 0xccc) as i32;
        FormantCompilerControl {
            shape: ((upper as u16 as u32) << 16) | u32::from(target.feedback as u16),
            input_gain: input.clamp(0, 32767) as i16,
            frequency: (self.code() * 258) as i16,
        }
    }
}
pub fn noise_control1(composed: i32) -> i16 {
    let value = composed.clamp(0, 65535) as i64;
    ((((value * value) >> 15) * 0x6666) >> 16).clamp(0, 32767) as i16
}
pub fn formant_control1(composed: i32) -> FormantControlTarget {
    let level = composed.clamp(0, 32767);
    let inverse = 32767 - level;
    let square = (inverse * inverse) >> 15;
    let feedback = -16 - ((square * square) >> 15);
    FormantControlTarget {
        level: level as i16,
        feedback: feedback.clamp(-32767, 0) as i16,
    }
}
