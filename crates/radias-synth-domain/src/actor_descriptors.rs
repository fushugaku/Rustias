//! Direct SYS01e9e4 descriptor/control publication plan, independent of devices.
use crate::{
    controller_noise::{FormantControlTarget, NoiseControl},
    controller_primary::PrimaryControl,
    controller_shaper::{ShaperControl, WaveshaperType},
    parameter_template::{ParameterTemplateTables, TemplateCompilationError},
};

/// Controller shadows are owned upstream; publishing them is a separate use case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActorControlCache {
    pub waveform: i16,
    pub cross: i16,
    pub colored_color: i16,
    pub formant_level: i16,
    pub formant_feedback: i16,
    pub vpm: i16,
    pub unison: i16,
    pub pitch: i16,
    pub control2_modulation: i16,
    pub control2_manual: i8,
    pub shaper_manual: i16,
    pub shaper_modulation: i16,
    pub filter_type_manual: i16,
    pub filter_type_modulation: i16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescriptorOperation {
    Work(u16),
    Send {
        sender: u8,
        offset: u16,
        value: u32,
        before: u16,
        after: u16,
    },
}
#[derive(Clone, Copy, Debug)]
pub struct DescriptorPlan {
    operations: [DescriptorOperation; 48],
    count: u8,
}
impl Default for DescriptorPlan {
    fn default() -> Self {
        Self {
            operations: [DescriptorOperation::Work(0); 48],
            count: 0,
        }
    }
}
impl DescriptorPlan {
    pub fn with_preparation_work(mut self, clocks: u16) -> Self {
        let count = usize::from(self.count);
        self.operations.copy_within(..count, 1);
        self.operations[0] = DescriptorOperation::Work(clocks);
        self.count += 1;
        self
    }
    pub fn operations(&self) -> &[DescriptorOperation] {
        &self.operations[..usize::from(self.count)]
    }
    /// Compact adjacent live callback work while preserving every send boundary.
    pub(crate) fn append_compacted(&mut self, other: &Self) {
        for operation in other.operations() {
            match *operation {
                DescriptorOperation::Work(clocks) if self.count != 0 => {
                    match &mut self.operations[usize::from(self.count) - 1] {
                        DescriptorOperation::Work(previous) => *previous += clocks,
                        DescriptorOperation::Send { after, .. } => *after += clocks,
                    }
                }
                DescriptorOperation::Send {
                    sender,
                    offset,
                    value,
                    mut before,
                    after,
                } => {
                    if self.count != 0
                        && let DescriptorOperation::Work(clocks) =
                            self.operations[usize::from(self.count) - 1]
                    {
                        before += clocks;
                        self.count -= 1;
                    }
                    self.send(sender, offset, value, before, after);
                }
                operation => self.push(operation),
            }
        }
    }
    pub(crate) fn work(&mut self, clocks: u16) {
        self.push(DescriptorOperation::Work(clocks));
    }
    fn push(&mut self, operation: DescriptorOperation) {
        self.operations[usize::from(self.count)] = operation;
        self.count += 1;
    }
    pub(crate) fn send(&mut self, sender: u8, offset: u16, value: u32, before: u16, after: u16) {
        self.push(DescriptorOperation::Send {
            sender,
            offset,
            value,
            before,
            after,
        });
    }
    fn word(&mut self, offset: u16, value: u16, before: u16, after: u16) {
        self.send(0, offset, u32::from(value), before, after);
    }
    /// Procedures0..11 are the whole called functions;12 composes SYS01e9e4.
    pub fn compile(
        procedure: u8,
        body: &[u8; 104],
        cache: ActorControlCache,
        tables: &ParameterTemplateTables,
    ) -> Result<Self, TemplateCompilationError> {
        let mut plan = Self::default();
        if procedure > 12 {
            return Err(TemplateCompilationError::InvalidTemplateIndex);
        }
        if procedure == 12 {
            plan.work(5);
            for index in 0..12 {
                let part = Self::compile(index, body, cache, tables)?;
                for operation in part.operations() {
                    plan.push(*operation);
                }
                plan.work(4);
            }
            return Ok(plan);
        }
        let primary = tables
            .primary
            .get(body[22])
            .ok_or(TemplateCompilationError::PcmOrInputGenerator)?;
        match procedure {
            0 => {
                let constants = primary.words();
                plan.word(
                    1,
                    primary.generator,
                    42,
                    if constants.is_empty() { 32 } else { 0 },
                );
                for (index, word) in constants.iter().enumerate() {
                    plan.word(
                        word.offset,
                        word.value,
                        if index == 0 { 55 } else { 35 },
                        if index + 1 == constants.len() { 20 } else { 0 },
                    );
                }
            }
            1 => plan.word(41, tables.secondary[usize::from(body[27] & 3)], 26, 6),
            2 => {
                plan.word(112, tables.filter2[usize::from((body[33] >> 4) & 3)], 28, 0);
                plan.word(93, u16::from(body[33] & 0x30 == 0x30), 23, 7);
            }
            3 => {
                if body[51] >= 128 {
                    return Err(TemplateCompilationError::InvalidOutputGain);
                }
                let high = body[51] >= 64;
                let value = if high {
                    tables.high_output_gain[usize::from(body[51] - 64)]
                } else {
                    0x1eb8
                };
                plan.word(158, value, if high { 32 } else { 30 }, 6);
            }
            4 => {
                let code = (i32::from(body[34] & 127) * 258
                    + i32::from(cache.filter_type_manual)
                    + 2 * i32::from(cache.filter_type_modulation))
                .clamp(0, 32767);
                plan.send(13, 53, code as u32, 47, 7);
            }
            5 => plan.word(43, if body[27] & 0x20 != 0 { 0x7fff } else { 0 }, 26, 6),
            6 => plan.word(42, if body[27] & 0x10 != 0 { 0x7fff } else { 0 }, 26, 6),
            7 | 8 => {
                let mode = body[46] & 3;
                let selector = if mode == 2 {
                    usize::from(body[47] & 15) + 2
                } else {
                    usize::from(mode)
                };
                let position = usize::from((body[46] >> 4) & 3);
                let value = if procedure == 7 {
                    tables.pre_shaper[selector][position]
                } else {
                    tables.post_shaper[selector][position]
                };
                plan.word(
                    if procedure == 7 { 82 } else { 83 },
                    value,
                    if mode == 2 { 47 } else { 45 },
                    6,
                );
            }
            9 => {
                let mode = body[46] & 3;
                if mode == 0 {
                    plan.work(34);
                } else {
                    let control = ShaperControl {
                        depth: body[48],
                        manual_offset: cache.shaper_manual,
                        modulation: cache.shaper_modulation,
                    };
                    let (depth, before) = if mode == 1 {
                        (control.drive_depth(), 63)
                    } else {
                        let kind = WaveshaperType::from_raw(body[47] & 15)
                            .ok_or(TemplateCompilationError::UnsupportedShaperSubtype)?;
                        let before = match kind {
                            WaveshaperType::HardClip => 225,
                            WaveshaperType::MultiTriangle
                            | WaveshaperType::MultiSine
                            | WaveshaperType::Pickup
                            | WaveshaperType::LevelBoost => 86,
                            _ => 66,
                        };
                        (control.waveshaper_depth(kind), before)
                    };
                    plan.word(84, depth as u16, before, 7);
                }
            }
            10 => plan.word(46, tables.routing[usize::from(body[33] & 3)], 26, 7),
            11 => {
                let selection = body[22] & 63;
                let mode = selection >> 4;
                let wave = selection & 15;
                let control = PrimaryControl {
                    control2: body[24],
                    control2_modulation: cache.control2_modulation,
                    control2_manual_offset: cache.control2_manual,
                    ..Default::default()
                };
                if wave >= 4 && mode != 0 {
                    plan.work(28);
                } else if wave < 4 {
                    match mode {
                        0 => plan.word(
                            if wave == 3 { 11 } else { 6 },
                            cache.waveform as u16,
                            47,
                            10,
                        ),
                        1 => plan.word(6, cache.cross as u16, 47, 10),
                        2 => plan.send(9, 6, u32::from(cache.unison as u16), 48, 10),
                        _ => {
                            plan.word(6, cache.vpm as u16, 49, 0);
                            plan.word(8, control.vpm_ratio() as u16, 28, 10);
                        }
                    }
                } else {
                    let noise = NoiseControl {
                        control2: control.control2,
                        control2_modulation: control.control2_modulation,
                        control2_manual_offset: control.control2_manual_offset,
                    };
                    if wave == 4 {
                        let target = noise.colored(cache.colored_color);
                        plan.word(6, target.color as u16, 48, 0);
                        plan.word(8, target.frequency as u16, 42, 10);
                    } else {
                        let target = noise.formant(
                            FormantControlTarget {
                                level: cache.formant_level,
                                feedback: cache.formant_feedback,
                            },
                            cache.pitch,
                        );
                        plan.send(14, 16, target.shape, 64, 0);
                        plan.word(6, target.input_gain as u16, 35, 0);
                        plan.word(8, target.frequency as u16, 27, 10);
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(plan)
    }
}
