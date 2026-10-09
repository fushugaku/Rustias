//! Program-dependent DSP parameter images, distinct from physical voice state.
use crate::{filter_control::FilterMixTable, primary_initialization::PrimaryInitializationTables};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterTemplate {
    pub words: [u16; 160],
}
impl ParameterTemplate {
    /// Original A6e5..AA76 cold initialization: four ordinary and sixteen
    /// drum templates. Word159 is intentionally retained by the source.
    pub fn boot(index: usize, prior: [u16; 160]) -> Option<Self> {
        if index >= 20 {
            return None;
        }
        let mut words = [0; 160];
        words[159] = prior[159];
        for (offset, value) in [
            (1, 0xbe7c),
            (41, 0xb29c),
            (46, 0xb468),
            (68, 0x7fff),
            (69, 0x7fff),
            (82, 0xd058),
            (83, 0xd058),
            (86, 0x5d70),
            (89, 0x4ccc),
            (90, 0x6666),
            (91, 0x999a),
            (112, 0x4022),
            (132, 0x7fff),
            (133, 0xffff),
            (134, 0x7fff),
            (150, 0x0216),
            (151, 0x7dea),
        ] {
            words[offset] = value;
        }
        if index < 4 {
            words[130] = (index as u16) * 4;
            words[131] = (index as u16) * 4 + 2;
        }
        Some(Self { words })
    }
}
pub struct ParameterTemplateTables {
    pub primary: PrimaryInitializationTables,
    pub secondary: [u16; 4],
    pub routing: [u16; 4],
    pub filter2: [u16; 4],
    pub pre_shaper: [[u16; 4]; 18],
    pub post_shaper: [[u16; 4]; 18],
    pub high_output_gain: [u16; 64],
    pub mix: FilterMixTable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateCompilationError {
    UnsupportedShaperSubtype,
    InvalidTemplateIndex,
    PcmOrInputGenerator,
    InvalidOutputGain,
}
impl ParameterTemplate {
    /// Complete static template publication sequence SYS0227f4. Dynamic
    /// per-note controls and physical phases are separate construction steps.
    pub fn compile(
        &mut self,
        body: &[u8; 104],
        tables: &ParameterTemplateTables,
    ) -> Result<(), TemplateCompilationError> {
        let primary = tables
            .primary
            .get(body[22])
            .ok_or(TemplateCompilationError::PcmOrInputGenerator)?;
        if body[51] >= 128 {
            return Err(TemplateCompilationError::InvalidOutputGain);
        }
        let shaper = body[46] & 3;
        let shaper = if shaper == 2 {
            usize::from(body[47] & 15) + 2
        } else {
            usize::from(shaper)
        };
        let position = usize::from((body[46] >> 4) & 3);
        let word = &mut self.words;
        word[158] = if body[51] < 64 {
            0x1eb8
        } else {
            tables.high_output_gain[usize::from(body[51] - 64)]
        };
        word[1] = primary.generator;
        for constant in primary.words() {
            word[usize::from(constant.offset)] = constant.value;
        }
        word[41] = tables.secondary[usize::from(body[27] & 3)];
        word[43] = if body[27] & 0x20 != 0 { 0x7fff } else { 0 };
        word[42] = if body[27] & 0x10 != 0 { 0x7fff } else { 0 };
        word[82] = tables.pre_shaper[shaper][position];
        word[83] = tables.post_shaper[shaper][position];
        word[46] = tables.routing[usize::from(body[33] & 3)];
        let filter_type = u16::from(body[34] & 127) * 258;
        word[53] = filter_type;
        for (index, value) in tables.mix.weights(filter_type).into_iter().enumerate() {
            word[72 + 2 * index] = value as u16;
        }
        word[112] = tables.filter2[usize::from((body[33] >> 4) & 3)];
        word[93] = u16::from(body[33] & 0x30 == 0x30);
        Ok(())
    }
}
