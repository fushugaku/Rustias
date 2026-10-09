//! Original FX controller arithmetic and complete program preparation.
//! This does not assign guessed meanings to FXD03 instructions or audio math.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectKind(u8);
impl EffectKind {
    pub fn new(raw: u8) -> Option<Self> {
        (raw < 31).then_some(Self(raw))
    }
    pub fn raw(self) -> u8 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectBank {
    Insert,
    Master,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectLoad {
    Default,
    ParameterSelected,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectOrigins {
    pub program: u16,
    pub data: u16,
    pub coefficients: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectRequest {
    pub kind: EffectKind,
    pub bank: EffectBank,
    pub load: EffectLoad,
    pub work_slot: u16,
    pub selector_byte: u8,
    pub origins: EffectOrigins,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MixContext {
    pub byte1: u8,
    pub byte5: u8,
    pub byte6: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectMix {
    pub dry: i32,
    pub wet: i32,
}
impl EffectMix {
    /// Controller words only. ASIC multiplier precision remains a separate gate.
    pub fn compile(kind: EffectKind, value: u8, context: MixContext) -> Option<Self> {
        if value > 100 {
            return None;
        }
        let v = i64::from(value);
        let full = 0x7fffffi64;
        let (dry, wet) = if matches!(kind.raw(), 1 | 2 | 3 | 6 | 7 | 8 | 9 | 24 | 27 | 29) {
            (full * (100 - v) / 100, full * v / 100)
        } else {
            let (d, w) = if v < 50 {
                ((100000 - 586 * v) / 1000, 1414 * v / 1000)
            } else {
                (1414 * (100 - v) / 1000, (586 * v + 41400) / 1000)
            };
            (d * full / 100, w * full / 100)
        };
        let invert = (kind.raw() == 22 && context.byte6 == 1 && context.byte1 == 0)
            || (kind.raw() == 23 && context.byte5 == 1);
        let sign = if invert { -1 } else { 1 };
        Some(Self {
            dry: (dry * sign) as i32,
            wet: (wet * sign) as i32,
        })
    }
    pub fn host_words(self) -> [u32; 2] {
        [self.dry as u32 & 0xffffff, self.wet as u32 & 0xffffff]
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgramBlock {
    pub destination: u16,
    pub tag: u32,
    pub buffer_address: u32,
    pub word_start: u16,
    pub count: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectPreparationError {
    TemplateLength,
    WorkSlot,
    BufferLayout,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectBufferLayout {
    pub normal_base: u32,
    pub selected_insert_base: u32,
    pub selected_master: u32,
}
pub struct PreparedEffect {
    pub words: [u64; 180],
    pub count: u16,
    pub blocks: [ProgramBlock; 2],
    pub block_count: u8,
    pub extended_insert: bool,
}
pub fn relocate_effect_word(word: u64, origins: EffectOrigins) -> u64 {
    let selector = ((word >> 32) as u16) & 0x7004;
    let middle = (word >> 16) as u16;
    let mut payload = word as u32;
    if matches!(selector, 0x6000 | 0x7000) {
        payload = payload.wrapping_add(u32::from(origins.program) << 19);
    } else if selector != 0x5000 && middle & 0x1800 != 0x1800 {
        payload = payload.wrapping_add(u32::from(origins.data) << 19);
    }
    if selector != 0x5000 && middle & 6 != 6 {
        payload = payload.wrapping_add(u32::from(origins.coefficients) << 9);
    }
    (word & 0xffff00000000) | u64::from(payload)
}
impl PreparedEffect {
    pub fn compile(
        request: EffectRequest,
        template: &[u8],
        layout: EffectBufferLayout,
    ) -> Result<Self, EffectPreparationError> {
        let count = template.len() / 6;
        if !template.len().is_multiple_of(6) || !matches!(count, 90 | 120) {
            return Err(EffectPreparationError::TemplateLength);
        }
        let selected = request.load == EffectLoad::ParameterSelected;
        let master = request.bank == EffectBank::Master;
        // These are the recovered software-buffer domains, not ASIC bank sizes.
        if (request.load == EffectLoad::Default && request.work_slot >= 20)
            || (selected && !master && request.work_slot >= 8)
        {
            return Err(EffectPreparationError::WorkSlot);
        }
        let target = if master {
            120
        } else if count > 90 {
            180
        } else {
            90
        };
        let mut result = Self {
            words: [0; 180],
            count: target,
            blocks: [ProgramBlock::default(); 2],
            block_count: 1,
            extended_insert: !master && count > 90,
        };
        for i in 0..usize::from(target) {
            let raw = if i < count {
                template[6 * i..6 * i + 6]
                    .iter()
                    .fold(0u64, |a, v| a << 8 | u64::from(*v))
            } else {
                0x48009806000e
            };
            result.words[i] = relocate_effect_word(raw, request.origins);
        }
        let slot = u32::from(request.work_slot);
        let (selector, address, stride) = if selected {
            if master {
                (85, layout.selected_master, 0)
            } else {
                (
                    61 + 3 * slot,
                    layout.selected_insert_base.wrapping_add(6 + slot * 0x22e),
                    0x22e,
                )
            }
        } else {
            (
                1 + 3 * slot,
                layout.normal_base.wrapping_add(6 + slot * 0x44a),
                0,
            )
        };
        if address & 1 != 0 {
            return Err(EffectPreparationError::BufferLayout);
        }
        let split = selected && !master && count > 90;
        result.block_count = if split { 2 } else { 1 };
        for i in 0..usize::from(result.block_count) {
            result.blocks[i] = ProgramBlock {
                destination: request.origins.program.wrapping_add((i * 90) as u16),
                tag: 0x02000000 | (selector + 3 * i as u32),
                buffer_address: address.wrapping_add(i as u32 * stride),
                word_start: (i * 90) as u16,
                count: if split { 90 } else { target },
            };
        }
        Ok(result)
    }
}
