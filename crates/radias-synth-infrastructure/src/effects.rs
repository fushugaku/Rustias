//! Read-only SYS 2.00 effects library adapter. Original program words are data
//! for an FX backend; a host upload does not prove their execution or sound.
use radias_synth_domain::effect_control::{
    EffectBank, EffectBufferLayout, EffectKind, EffectLoad, EffectRequest, PreparedEffect,
};
pub struct EffectLibrary<'a> {
    system: &'a [u8],
    layout: EffectBufferLayout,
}
impl<'a> EffectLibrary<'a> {
    pub fn from_system(system: &'a [u8]) -> Result<Self, &'static str> {
        if system.len() != 0xe0000 {
            return Err("Complete SYS2.00 effect library required");
        }
        let mut library = Self {
            system,
            layout: EffectBufferLayout {
                normal_base: 0,
                selected_insert_base: 0,
                selected_master: 0,
            },
        };
        library.layout = EffectBufferLayout {
            normal_base: library.word(0x0c0747cc)?,
            selected_insert_base: library.word(0x0c074d5c)?,
            selected_master: library.word(0x0c074a70)?,
        };
        Ok(library)
    }
    fn bytes(&self, address: u32, count: usize) -> Result<&'a [u8], &'static str> {
        let offset = address
            .checked_sub(0x0c000000)
            .ok_or("Effect pointer outside SYS")? as usize
            + 0x1000;
        self.system
            .get(offset..offset.checked_add(count).ok_or("Effect pointer overflow")?)
            .ok_or("Effect data outside SYS")
    }
    fn word(&self, address: u32) -> Result<u32, &'static str> {
        Ok(u32::from_be_bytes(
            self.bytes(address, 4)?.try_into().unwrap(),
        ))
    }
    fn header(&self, bank: EffectBank, kind: EffectKind) -> Result<&'a [u8], &'static str> {
        let table = if bank == EffectBank::Master {
            0x0c0ccf28
        } else {
            0x0c0cceac
        };
        self.bytes(self.word(table + 4 * u32::from(kind.raw()))?, 32)
    }
    pub fn name(&self, bank: EffectBank, kind: EffectKind) -> Result<&'a str, &'static str> {
        std::str::from_utf8(&self.header(bank, kind)?[..13])
            .map(str::trim_end)
            .map_err(|_| "Effect name invalid")
    }
    pub fn parameter_count(&self, bank: EffectBank, kind: EffectKind) -> Result<u8, &'static str> {
        Ok(self.header(bank, kind)?[0x13])
    }
    pub fn prepare(&self, request: EffectRequest) -> Result<PreparedEffect, &'static str> {
        let master = request.bank == EffectBank::Master;
        let header = self.header(request.bank, request.kind)?;
        let count = usize::from(header[if master { 0x16 } else { 0x15 }]);
        if !matches!(count, 90 | 120) {
            return Err("Unsupported original effect template size");
        }
        let code_field = if master { 0x1c } else { 0x18 };
        let mut pointer =
            u32::from_be_bytes(header[code_field..code_field + 4].try_into().unwrap());
        if request.load == EffectLoad::ParameterSelected {
            let alternative = match (request.bank, request.kind.raw(), request.selector_byte) {
                (EffectBank::Insert, 30, 0) => Some(0x0c074a8c),
                (EffectBank::Insert, 30, _) => Some(0x0c074a88),
                (EffectBank::Master, 30, 0) => Some(0x0c074a94),
                (EffectBank::Master, 30, _) => Some(0x0c074a90),
                (EffectBank::Master, 11, 4 | 5) => Some(0x0c074a9c),
                (EffectBank::Master, 11, _) => Some(0x0c074a98),
                _ => None,
            };
            if let Some(address) = alternative {
                pointer = self.word(address)?;
            }
        }
        PreparedEffect::compile(request, self.bytes(pointer, 6 * count)?, self.layout)
            .map_err(|_| "Native effect preparation rejected inputs")
    }
    pub fn coefficient_update_indices(&self) -> Result<[[u16; 4]; 9], &'static str> {
        let raw = self.bytes(0x0c0b7768, 72)?;
        Ok(core::array::from_fn(|i| {
            core::array::from_fn(|j| {
                let p = i * 8 + j * 2;
                u16::from_be_bytes([raw[p], raw[p + 1]])
            })
        }))
    }
    /// Read a six-byte original parameter-range record. No coefficient outputs
    /// or interpreter state are retained in this adapter.
    pub fn parameter_range(
        &self,
        index: u8,
    ) -> Result<radias_synth_domain::effect_curves::EffectParameterRange, &'static str> {
        let raw = self.bytes(0x0c0cd250 + u32::from(index) * 6, 6)?;
        Ok(radias_synth_domain::effect_curves::EffectParameterRange {
            minimum: i16::from_be_bytes([raw[0], raw[1]]),
            maximum: i16::from_be_bytes([raw[2], raw[3]]),
            encoded_zero: raw[4],
        })
    }
    pub fn decimator_tables(
        &self,
    ) -> Result<radias_synth_domain::decimator_effect::DecimatorEffectTables, &'static str> {
        fn table<const N: usize>(
            library: &EffectLibrary<'_>,
            address: u32,
        ) -> Result<[u32; N], &'static str> {
            let raw = library.bytes(address, N * 4)?;
            Ok(core::array::from_fn(|i| {
                u32::from_be_bytes(raw[4 * i..4 * i + 4].try_into().unwrap())
            }))
        }
        Ok(
            radias_synth_domain::decimator_effect::DecimatorEffectTables {
                bit_depth: table(self, 0x0c0beba4)?,
                sample_rate: table(self, 0x0c0bebf8)?,
                pre_lpf: table(self, 0x0c0bed74)?,
            },
        )
    }
}
