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
    pub fn effect_property_tables(
        &self,
    ) -> Result<radias_synth_domain::effect_property::EffectPropertyTables, &'static str> {
        let mut ranges = [self.parameter_range(0)?; 256];
        for (index, range) in ranges.iter_mut().enumerate() {
            *range = self.parameter_range(index as u8)?;
        }
        let mut indices = [[[75u8; 20]; 31]; 2];
        for (bank, kinds) in indices.iter_mut().enumerate() {
            for (kind, parameters) in kinds.iter_mut().enumerate() {
                let descriptor =
                    self.word((if bank == 0 { 0x0c0cceac } else { 0x0c0ccf28 }) + 4 * kind as u32)?;
                let count = usize::from(self.bytes(descriptor + 19, 1)?[0]);
                let definitions = self.word(descriptor + 48)?;
                for (parameter, value) in parameters.iter_mut().enumerate().take(count) {
                    *value = self.bytes(definitions + 20 * parameter as u32 + 10, 1)?[0];
                }
            }
        }
        Ok(radias_synth_domain::effect_property::EffectPropertyTables {
            ranges,
            insert_range_indices: indices[0],
            master_range_indices: indices[1],
            tempo_rate: self.bytes(0x0c0ca970, 30)?.try_into().unwrap(),
            free_rate: self.bytes(0x0c0ca98e, 100)?.try_into().unwrap(),
            tempo_rate_display: self.bytes(0x0c0ca88c, 128)?.try_into().unwrap(),
            free_rate_display: self.bytes(0x0c0ca80c, 128)?.try_into().unwrap(),
        })
    }
    pub fn master_parameter_caller_tables(
        &self,
    ) -> Result<
        radias_synth_domain::master_parameter_caller::MasterParameterCallerTables,
        &'static str,
    > {
        self.effect_parameter_caller_tables()
    }
    pub fn effect_parameter_caller_tables(
        &self,
    ) -> Result<
        radias_synth_domain::effect_parameter_caller::EffectParameterCallerTables,
        &'static str,
    > {
        let time = self.bytes(0x0c0be800, 62)?;
        let modes = self.bytes(0x0c0b77cf, 186)?;
        Ok(
            radias_synth_domain::effect_parameter_caller::EffectParameterCallerTables {
                time_parameters: core::array::from_fn(|i| {
                    time[2 * i..2 * i + 2].try_into().unwrap()
                }),
                owner_modes: core::array::from_fn(|i| modes[6 * i..6 * i + 6].try_into().unwrap()),
            },
        )
    }
    pub fn program_buffer_layout(
        &self,
    ) -> radias_synth_domain::effect_transition_queue::EffectProgramBufferLayout {
        radias_synth_domain::effect_transition_queue::EffectProgramBufferLayout {
            normal: self.layout.normal_base,
            selected_insert: self.layout.selected_insert_base,
            // The preparation path addresses the body; selector 84 starts
            // at the six-byte prefix before it.
            selected_master: self.layout.selected_master.wrapping_sub(6),
        }
    }
    pub fn pair_transition_tables(
        &self,
    ) -> Result<radias_synth_domain::effect_pair_transition::EffectPairTransitionTables, &'static str>
    {
        let raw = self.bytes(0x0c0c5ccc, 62)?;
        let mut second_active = [false; 31];
        for (i, active) in second_active.iter_mut().enumerate() {
            let pointer = self.word(0x0c0cceac + 4 * i as u32)?;
            *active = self.bytes(pointer + 14, 1)?[0] != 1;
        }
        Ok(
            radias_synth_domain::effect_pair_transition::EffectPairTransitionTables {
                offsets: core::array::from_fn(|i| [raw[2 * i], raw[2 * i + 1]]),
                second_active,
            },
        )
    }
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
    fn table<const N: usize>(&self, address: u32) -> Result<[u32; N], &'static str> {
        let raw = self.bytes(address, N * 4)?;
        Ok(core::array::from_fn(|i| {
            u32::from_be_bytes(raw[4 * i..4 * i + 4].try_into().unwrap())
        }))
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
                high_dump_peak: self.word(0x0c0b971c + 9 * 12 + 4)?,
                high_dump_range: self
                    .parameter_range(self.bytes(0x0c0b971c + 9 * 12 + 9, 1)?[0])?,
                output_peak: self.word(0x0c0b971c + 10 * 12 + 4)?,
                output_range: self.parameter_range(self.bytes(0x0c0b971c + 10 * 12 + 9, 1)?[0])?,
                fs_mod_peak: self.word(0x0c0b971c + 11 * 12 + 4)?,
                fs_mod_range: self.parameter_range(self.bytes(0x0c0b971c + 11 * 12 + 9, 1)?[0])?,
            },
        )
    }
    pub fn lfo_mapping(
        &self,
        bank: EffectBank,
        kind: EffectKind,
    ) -> Result<radias_synth_domain::effect_lfo_program::EffectLfoMapping, &'static str> {
        let table = if bank == EffectBank::Insert {
            0x0c0cceac
        } else {
            0x0c0ccf28
        };
        let descriptor = self.word(table + 4 * u32::from(kind.raw()))?;
        let raw = self.bytes(descriptor + 0x24, 9)?;
        Ok(radias_synth_domain::effect_lfo_program::EffectLfoMapping {
            fields: raw[..8].try_into().unwrap(),
            definition_mode: raw[8],
        })
    }
    pub fn dynamics_tables(
        &self,
    ) -> Result<radias_synth_domain::dynamics_effect::DynamicsEffectTables, &'static str> {
        use radias_synth_domain::dynamics_effect::{
            DynamicsCoefficientGroup, DynamicsEffectDefinition, DynamicsEffectTables,
        };
        fn definition(
            library: &EffectLibrary<'_>,
            kind: u8,
        ) -> Result<DynamicsEffectDefinition, &'static str> {
            let descriptor = library.word(0x0c0cceac + 4 * u32::from(kind))?;
            let parameters = library.word(descriptor + 0x30)?;
            let group_pointer = library.word(0x0c0ccfa4 + 4 * u32::from(kind))?;
            let zero = library.parameter_range(0)?;
            let mut result = DynamicsEffectDefinition {
                parameter_ranges: [zero; 6],
                dependencies: [0; 6],
                groups: [DynamicsCoefficientGroup {
                    first: 0,
                    second: 0,
                    action: 0,
                    range: zero,
                }; 19],
            };
            for i in 0..if kind == 1 { 5 } else { 6 } {
                let record = parameters + 20 * i as u32;
                result.parameter_ranges[i] =
                    library.parameter_range(library.bytes(record + 10, 1)?[0])?;
                let dependencies = library.word(record + 12)?;
                if dependencies & 0x1fff != 0 || library.word(record + 16)? != 0 {
                    return Err("Dynamics dependency outside compiled groups");
                }
                result.dependencies[i] = dependencies;
            }
            for (i, group) in result.groups.iter_mut().enumerate() {
                let record = group_pointer + 12 * i as u32;
                *group = DynamicsCoefficientGroup {
                    first: library.word(record + 4)? as i32,
                    second: library.word(record)? as i32,
                    action: library.bytes(record + 8, 1)?[0],
                    range: library.parameter_range(library.bytes(record + 9, 1)?[0])?,
                };
            }
            Ok(result)
        }
        Ok(DynamicsEffectTables {
            definitions: [
                definition(self, 1)?,
                definition(self, 2)?,
                definition(self, 3)?,
            ],
            limiter_threshold: self.table(0x0c0be840)?,
            // SYS0762E8's 0xA4 immediate is signed (-92), so encoded 23
            // addresses 0C0BE8E4; it is not an unsigned pointer increment.
            gain: self.table(0x0c0be8e4)?,
            ratio: self.table(0x0c0be9ec)?,
            gate_threshold: self.table(0x0c0c2758)?,
            sensitivity: self.table(0x0c0c2958)?,
            raw_master_sensitivity: self.table(0x0c0c2954)?,
            attack: self.table(0x0c0c2b54)?,
            release: self.table(0x0c0c2d54)?,
            raw_lookup: [
                self.table(0x0c0be840 - 128 * 4)?,
                self.table(0x0c0be8e4 - 92 - 128 * 4)?,
                self.table(0x0c0be9ec - 128 * 4)?,
                self.table(0x0c0c2758 - 128 * 4)?,
                self.table(0x0c0c2954 - 128 * 4)?,
                self.table(0x0c0c2b54 - 128 * 4)?,
                self.table(0x0c0c2d54 - 128 * 4)?,
            ],
        })
    }
    pub fn filter_effect_tables(
        &self,
    ) -> Result<radias_synth_domain::filter_effect::FilterEffectTables, &'static str> {
        let raw = self.bytes(0x0c040ed0, 256)?;
        Ok(radias_synth_domain::filter_effect::FilterEffectTables {
            frequency: crate::firmware::controller_filter_tables(self.system)?,
            resonance: core::array::from_fn(|i| u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]])),
            resonance_gain: self.table(0x0c040340)?,
            response: self.table(0x0c0c3354)?,
            response_complement: self.table(0x0c0c3554)?,
        })
    }
    pub fn insert_type_construction_tables(&self) -> Result<radias_synth_domain::insert_type_construction::InsertTypeConstructionTables, &'static str> {
        use radias_synth_domain::insert_type_construction::InsertTypeConstructionTables;
        let mut tables = InsertTypeConstructionTables { parameter_counts: [0;31], defaults: [[0;20];31], owners: [[0;2];31], ranges: [[self.parameter_range(0)?;20];31] };
        for kind in 0..31usize {
            let descriptor = self.word(0x0c0cceac+4*kind as u32)?;
            let count = self.bytes(descriptor+19,1)?[0];
            if count>20 { return Err("Insert type constructor outside parameter bounds"); }
            tables.parameter_counts[kind] = count;
            tables.owners[kind].copy_from_slice(self.bytes(descriptor+15,2)?);
            let parameters = self.word(descriptor+48)?;
            for index in 0..usize::from(count) {
                tables.defaults[kind][index] = self.bytes(parameters+20*index as u32+9,1)?[0];
                tables.ranges[kind][index] = self.parameter_range(self.bytes(parameters+20*index as u32+10,1)?[0])?;
            }
        }
        Ok(tables)
    }

    pub fn insert_construction_tables(
        &self,
    ) -> Result<
        radias_synth_domain::insert_effect_construction::InsertConstructionTables,
        &'static str,
    > {
        use radias_synth_domain::insert_effect_construction::{
            InsertConstructionDefinition, InsertConstructionTables,
        };
        let zero = self.parameter_range(0)?;
        let mut definitions = Vec::new();
        for kind in 0..31 {
            let descriptor = self.word(0x0c0cceac + 4 * kind)?;
            let count = usize::from(self.bytes(descriptor + 19, 1)?[0]);
            if count > 20 {
                return Err("Insert constructor parameter count outside native bounds");
            }
            let parameter = self.word(descriptor + 48)?;
            let mut d = InsertConstructionDefinition {
                parameter_count: count,
                ranges: [zero; 20],
            };
            for (i, range) in d.ranges[..count].iter_mut().enumerate() {
                *range = self.parameter_range(self.bytes(parameter + 20 * i as u32 + 10, 1)?[0])?;
            }
            definitions.push(d);
        }
        Ok(InsertConstructionTables {
            definitions: definitions
                .try_into()
                .map_err(|_| "Wrong insert construction definition count")?,
        })
    }
    pub fn insert_control_tables(
        &self,
    ) -> Result<radias_synth_domain::insert_effect_control::InsertControlTables, &'static str> {
        use radias_synth_domain::insert_effect_control::{
            InsertControlDefinition, InsertControlTables,
        };
        let constructor = self.insert_construction_tables()?;
        let mut definitions = Vec::new();
        for (kind, d) in constructor.definitions.into_iter().enumerate() {
            let descriptor = self.word(0x0c0cceac + 4 * kind as u32)?;
            definitions.push(InsertControlDefinition {
                parameter_count: d.parameter_count,
                ranges: d.ranges,
                initialization_mask: self.word(descriptor + 52)?,
                lfo_mapping: self
                    .lfo_mapping(EffectBank::Insert, EffectKind::new(kind as u8).unwrap())?,
            });
        }
        Ok(InsertControlTables {
            definitions: definitions
                .try_into()
                .map_err(|_| "Wrong Insert control definition count")?,
            common: self.master_control_tables()?,
            tube: self.tube_tables()?,
            equalizer: self.equalizer_effect_tables()?,
            reverb: self.reverb_effect_tables()?,
            delay: self.delay_effect_tables()?,
            auto_pan: self.auto_pan_delay_tables()?,
            mod_delay: self.mod_delay_tables()?,
            chorus: self.chorus_effect_tables()?,
            vibrato: self.vibrato_tables()?,
        })
    }
    pub fn timbre_output_tables(
        &self,
    ) -> Result<radias_synth_domain::timbre_output::TimbreOutputTables, &'static str> {
        let global_input_owners: [u8; 16] = self.bytes(0x0c04c7c4, 16)?.try_into().unwrap();
        if global_input_owners.iter().any(|&p| p >= 4) {
            return Err("Unsupported global input owner binding");
        }
        Ok(radias_synth_domain::timbre_output::TimbreOutputTables {
            global_input_owners,
        })
    }
    pub fn effect_rack_rebuild_tables(
        &self,
    ) -> Result<radias_synth_domain::effect_rack_rebuild::EffectRackRebuildTables, &'static str>
    {
        use radias_synth_domain::effect_rack_rebuild::EffectRackRebuildTables;
        let read_words = |pointers, counts| -> Result<[u64; 5], &'static str> {
            let mut words = [0; 5];
            for (part, word) in words.iter_mut().enumerate() {
                let getter = self.word(counts + 4 * part as u32)?;
                if self.bytes(getter, 4)? != [0, 11, 0xe0, 1] {
                    return Err("Unsupported rack prefix/tail word count");
                }
                *word = self
                    .bytes(self.word(pointers + 4 * part as u32)?, 6)?
                    .iter()
                    .fold(0u64, |v, &b| (v << 8) | u64::from(b));
            }
            Ok(words)
        };
        let raw = self.bytes(0x0c0c5d0a, 62)?;
        let mut temporary_indices = [[0; 4]; 9];
        for (slot, indices) in temporary_indices.iter_mut().enumerate() {
            for (i, index) in indices.iter_mut().enumerate() {
                *index = u16::from_be_bytes(
                    self.bytes(0x0c0b7768 + (slot * 8 + i * 2) as u32, 2)?
                        .try_into()
                        .unwrap(),
                );
            }
        }
        Ok(EffectRackRebuildTables {
            rack: self.effect_rack_initialization_tables()?,
            constructors: self.insert_construction_tables()?,
            pair: self.pair_transition_tables()?,
            equalizer: self.equalizer_tables()?,
            prefixes: read_words(0x0c0ccbf0, 0x0c0ccc04)?,
            tails: read_words(0x0c0ccc18, 0x0c0ccc2c)?,
            master_transition_offsets: core::array::from_fn(|i| [raw[i * 2], raw[i * 2 + 1]]),
            temporary_indices,
        })
    }
    pub fn effect_rack_initialization_tables(
        &self,
    ) -> Result<
        radias_synth_domain::effect_rack_initialization::EffectRackInitializationTables,
        &'static str,
    > {
        use radias_synth_domain::{
            effect_rack_initialization::EffectRackInitializationTables,
            master_effect_initialization::MasterInitialProgram,
        };
        let read_selected = |kind: u8,
                             pointer_address: u32|
         -> Result<MasterInitialProgram, &'static str> {
            let source_words = self.header(EffectBank::Master, EffectKind::new(kind).unwrap())?[22];
            if !matches!(source_words, 90 | 120) {
                return Err("Unsupported selected Master program size");
            }
            let mut bytes = [0; 720];
            bytes[..usize::from(source_words) * 6].copy_from_slice(
                self.bytes(self.word(pointer_address)?, usize::from(source_words) * 6)?,
            );
            Ok(MasterInitialProgram {
                bytes,
                source_words,
            })
        };
        let mut occupies_pair = [false; 31];
        for (kind, paired) in occupies_pair.iter_mut().enumerate() {
            *paired =
                self.header(EffectBank::Insert, EffectKind::new(kind as u8).unwrap())?[14] != 0;
        }
        Ok(EffectRackInitializationTables {
            control: self.insert_control_tables()?,
            insert_programs: self.insert_program_initialization_tables()?,
            insert_coefficients: self.insert_initialization_tables()?,
            master_coefficients: self.master_initialization_tables()?,
            master_talking_programs: [
                read_selected(30, 0x0c074a94)?,
                read_selected(30, 0x0c074a90)?,
            ],
            master_reverb_programs: [
                read_selected(11, 0x0c074a98)?,
                read_selected(11, 0x0c074a9c)?,
            ],
            occupies_pair,
        })
    }
    pub fn insert_program_initialization_tables(
        &self,
    ) -> Result<
        radias_synth_domain::insert_program_initialization::InsertProgramInitializationTables,
        &'static str,
    > {
        use radias_synth_domain::insert_program_initialization::{
            InsertInitialProgram, InsertProgramInitializationTables,
        };
        let read_program = |pointer, source_words| -> Result<InsertInitialProgram, &'static str> {
            if !matches!(source_words, 90 | 120) {
                return Err("Unsupported Insert initial program size");
            }
            let mut bytes = [0; 720];
            bytes[..usize::from(source_words) * 6]
                .copy_from_slice(self.bytes(pointer, usize::from(source_words) * 6)?);
            Ok(InsertInitialProgram {
                bytes,
                source_words,
            })
        };
        let programs = (0..31u8)
            .map(|kind| {
                let header = self.header(EffectBank::Insert, EffectKind::new(kind).unwrap())?;
                let pointer = u32::from_be_bytes(header[24..28].try_into().unwrap());
                read_program(pointer, header[21])
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        let count = self.header(EffectBank::Insert, EffectKind::new(30).unwrap())?[21];
        Ok(InsertProgramInitializationTables {
            programs: programs
                .try_into()
                .map_err(|_| "Wrong Insert initial program count")?,
            talking: [
                read_program(self.word(0x0c074a8c)?, count)?,
                read_program(self.word(0x0c074a88)?, count)?,
            ],
            layout: self.layout,
        })
    }
    pub fn insert_initialization_tables(
        &self,
    ) -> Result<
        radias_synth_domain::insert_effect_initialization::InsertInitializationTables,
        &'static str,
    > {
        use radias_synth_domain::insert_effect_initialization::{
            InsertInitialCoefficients, InsertInitializationTables,
        };
        let read_coefficients = |pointer_table: u32,
                                 count_table: u32,
                                 index: u32|
         -> Result<InsertInitialCoefficients, &'static str> {
            let getter = self.word(count_table + index * 4)?;
            let leaf = self.bytes(getter, 4)?;
            if leaf[..3] != [0, 11, 0xe0] || leaf[3] > 73 {
                return Err("Unsupported Insert initial coefficient count getter");
            }
            let count = leaf[3];
            let pointer = self.word(pointer_table + index * 4)?;
            let mut words = [0; 73];
            for (word, raw) in words
                .iter_mut()
                .zip(self.bytes(pointer, usize::from(count) * 4)?.chunks_exact(4))
            {
                *word = u32::from_be_bytes(raw.try_into().unwrap());
            }
            Ok(InsertInitialCoefficients { words, count })
        };
        let coefficients = (0..31)
            .map(|i| read_coefficients(0x0c0ccc40, 0x0c0ccd68, i))
            .collect::<Result<Vec<_>, _>>()?;
        let reverb = (0..3)
            .map(|i| read_coefficients(0x0c0cccbc, 0x0c0ccde4, i))
            .collect::<Result<Vec<_>, _>>()?;
        let talking = (0..3)
            .map(|i| read_coefficients(0x0c0cccc8, 0x0c0ccdf0, i))
            .collect::<Result<Vec<_>, _>>()?;
        let lfo_mappings = (0..31)
            .map(|i| self.lfo_mapping(EffectBank::Insert, EffectKind::new(i).unwrap()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(InsertInitializationTables {
            coefficients: coefficients.try_into().unwrap(),
            reverb: reverb.try_into().unwrap(),
            talking: talking.try_into().unwrap(),
            lfo_mappings: lfo_mappings.try_into().unwrap(),
            tempo: crate::firmware::lfo_tempo_tables(self.system)?,
            allocation: self.buffer_allocation_tables()?,
        })
    }
    pub fn master_initialization_tables(
        &self,
    ) -> Result<
        radias_synth_domain::master_effect_initialization::MasterInitializationTables,
        &'static str,
    > {
        use radias_synth_domain::master_effect_initialization::{
            MasterInitialCoefficients, MasterInitialProgram, MasterInitializationTables,
        };
        // Whole SYS0750AA leaf return counts, distinct from dependency groups.
        let counts = [
            9, 30, 30, 31, 30, 57, 40, 43, 59, 24, 20, 73, 69, 31, 30, 32, 33, 31, 31, 33, 31, 31,
            35, 20, 13, 21, 42, 30, 28, 69, 63,
        ];
        let read_coefficients =
            |pointer, count| -> Result<MasterInitialCoefficients, &'static str> {
                let mut words = [0; 73];
                let raw = self.bytes(pointer, usize::from(count) * 4)?;
                for (word, bytes) in words.iter_mut().zip(raw.chunks_exact(4)) {
                    *word = u32::from_be_bytes(bytes.try_into().unwrap());
                }
                Ok(MasterInitialCoefficients { words, count })
            };
        let mut programs = Vec::new();
        let mut coefficients = Vec::new();
        for kind in 0..31u8 {
            let header = self.header(EffectBank::Master, EffectKind::new(kind).unwrap())?;
            let source_words = header[22];
            if !matches!(source_words, 90 | 120) {
                return Err("Unsupported Master initial program size");
            }
            let pointer = u32::from_be_bytes(header[28..32].try_into().unwrap());
            let mut bytes = [0; 720];
            bytes[..usize::from(source_words) * 6]
                .copy_from_slice(self.bytes(pointer, usize::from(source_words) * 6)?);
            programs.push(MasterInitialProgram {
                bytes,
                source_words,
            });
            coefficients.push(read_coefficients(
                self.word(0x0c0cccd4 + 4 * u32::from(kind))?,
                counts[usize::from(kind)],
            )?);
        }
        let mut reverb = Vec::new();
        for kind in 0..6 {
            reverb.push(read_coefficients(self.word(0x0c0ccd50 + 4 * kind)?, 73)?);
        }
        let mut talking = Vec::new();
        for mode in 0..3 {
            talking.push(read_coefficients(self.word(0x0c0cccc8 + 4 * mode)?, 63)?);
        }
        Ok(MasterInitializationTables {
            programs: programs
                .try_into()
                .map_err(|_| "Wrong Master initial program count")?,
            coefficients: coefficients
                .try_into()
                .map_err(|_| "Wrong Master initial coefficient count")?,
            reverb: reverb.try_into().unwrap(),
            talking: talking.try_into().unwrap(),
            program_layout: self.layout,
        })
    }
    pub fn master_reverb_tables(
        &self,
    ) -> Result<radias_synth_domain::master_reverb_control::MasterReverbTables, &'static str> {
        use radias_synth_domain::master_reverb_control::MasterReverbTables;
        let addresses = [
            0x0c0b6d8c, 0x0c0b6eb0, 0x0c0b70f8, 0x0c0b6fd4, 0x0c0b6b44, 0x0c0b6c68,
        ];
        let mut coefficients = [[0; 73]; 6];
        for (words, address) in coefficients.iter_mut().zip(addresses) {
            *words = self.table(address)?;
        }
        Ok(MasterReverbTables {
            time: self.reverb_time_tables()?,
            coefficients,
            programs: [
                self.bytes(0x0c0b17f8, 720)?.try_into().unwrap(),
                self.bytes(0x0c0b1528, 720)?.try_into().unwrap(),
            ],
            pre_delay: self.bytes(0x0c0aafec, 128)?.try_into().unwrap(),
            damping_range: self.parameter_range(145)?,
            depth_range: self.parameter_range(17)?,
        })
    }
    pub fn master_control_tables(
        &self,
    ) -> Result<radias_synth_domain::master_effect_control::MasterControlTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup,
            master_effect_control::{MasterControlTables, MasterDefinition},
        };
        let zero = self.parameter_range(0)?;
        let mut definitions = Vec::new();
        for kind in 0..31u8 {
            let d = self.word(0x0c0ccf28 + 4 * u32::from(kind))?;
            let count = usize::from(self.bytes(d + 19, 1)?[0]);
            let groups_count = usize::from(self.bytes(d + 20, 1)?[0]);
            if count > 20 || groups_count > 73 {
                return Err("Master descriptor outside native bounds");
            }
            let p = self.word(d + 48)?;
            let g = self.word(0x0c0cd020 + 4 * u32::from(kind))?;
            let mut definition = MasterDefinition {
                parameter_count: count,
                defaults: [0; 20],
                default_owner: self.bytes(d + 17, 1)?[0],
                initialization_mask: self.word(d + 52)?,
                ranges: [zero; 20],
                dependencies: [[0; 2]; 20],
                group_count: groups_count,
                groups: [EffectCoefficientGroup {
                    first: 0,
                    second: 0,
                    action: 0,
                    range: zero,
                }; 73],
                lfo_mapping: self
                    .lfo_mapping(EffectBank::Master, EffectKind::new(kind).unwrap())?,
            };
            for i in 0..count {
                let a = p + 20 * i as u32;
                definition.defaults[i] = self.bytes(a + 9, 1)?[0];
                definition.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
                definition.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
            }
            for (i, group) in definition.groups[..groups_count].iter_mut().enumerate() {
                let a = g + 12 * i as u32;
                *group = EffectCoefficientGroup {
                    first: self.word(a + 4)? as i32,
                    second: self.word(a)? as i32,
                    action: self.bytes(a + 8, 1)?[0],
                    range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
                };
            }
            definitions.push(definition);
        }
        Ok(MasterControlTables {
            definitions: definitions
                .try_into()
                .map_err(|_| "Wrong master type count")?,
            dynamics: self.dynamics_tables()?,
            ensemble: self.ensemble_effect_tables()?,
            ring: self.tremolo_ring_mod_tables()?,
            pitch: self.pitch_grain_tables()?,
            decimator: self.decimator_tables()?,
            equalizer: self.equalizer_tables()?,
            equalizer_gain: self.table(0x0c0c13d4)?,
            distortion_gain: self.table(0x0c0c3954)?,
            delay_times: {
                let mut time = self.delay_time_tables()?;
                let words = |address| -> Result<[u16; 128], &'static str> {
                    let raw = self.bytes(address, 256)?;
                    Ok(core::array::from_fn(|i| {
                        u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]])
                    }))
                };
                time.lcr_milliseconds = words(0x0c0ab56c)?;
                time.stereo_milliseconds = words(0x0c0ab46c)?;
                time
            },
            tape_milliseconds: {
                let raw = self.bytes(0x0c0aba6c, 256)?;
                core::array::from_fn(|i| u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]]))
            },
            time_owners: self.bytes(0x0c0b77b0, 31)?.try_into().unwrap(),
            stereo_mod_milliseconds: {
                let raw = self.bytes(0x0c0ab96c, 256)?;
                core::array::from_fn(|i| u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]]))
            },
            chorus_milliseconds: {
                let raw = self.bytes(0x0c0abb6c, 256)?;
                core::array::from_fn(|i| u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]]))
            },
            modulation_rate: self.table(0x0c0c3754)?,
            flanger: self.flanger_phaser_tables()?,
            cabinet: self.cabinet_tables()?,
            filter: self.filter_effect_tables()?,
            wah: self.wah_tables()?,
            rotary: self.rotary_tables()?,
            talking: self.talking_tables()?,
            master_talking_programs: [
                self.bytes(0x0c0b2608, 720)?.try_into().unwrap(),
                self.bytes(0x0c0b28d8, 720)?.try_into().unwrap(),
            ],
            early_reflect: self.early_reflect_effect_tables()?,
            early_reflect_decay: self.table(0x0c0bf190)?,
            reverb: self.master_reverb_tables()?,
            grain_milliseconds: {
                let raw = self.bytes(0x0c0ab66c, 256)?;
                core::array::from_fn(|i| u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]]))
            },
            tempo: crate::firmware::lfo_tempo_tables(self.system)?,
        })
    }
    pub fn talking_tables(
        &self,
    ) -> Result<radias_synth_domain::talking_effect::TalkingTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup, talking_effect::TalkingTables,
        };
        let descriptor = self.word(0x0c0cceac + 30 * 4)?;
        if self.bytes(descriptor + 19, 1)?[0] != 20 || self.bytes(descriptor + 20, 1)?[0] != 63 {
            return Err("Unexpected Talking definition size");
        }
        let p = self.word(descriptor + 48)?;
        let g = self.word(0x0c0ccfa4 + 30 * 4)?;
        let zero = self.parameter_range(0)?;
        let early = self.early_reflect_effect_tables()?;
        let mut result = TalkingTables {
            ranges: [zero; 20],
            dependencies: [[0; 2]; 20],
            groups: [EffectCoefficientGroup {
                first: 0,
                second: 0,
                action: 0,
                range: zero,
            }; 63],
            voices: core::array::from_fn(|_| [0; 8]),
            response: self.table(0x0c0c2f54)?,
            damping: self.table(0x0c0c3154)?,
            positive_range: self.parameter_range(17)?,
            centered_range: self.parameter_range(24)?,
            lfo_mapping: self.lfo_mapping(EffectBank::Insert, EffectKind::new(30).unwrap())?,
            tempo: crate::firmware::lfo_tempo_tables(self.system)?,
            routing: early.routing,
            pair_transition: early.pair_transition,
            transition_programs: early.transition_programs,
            programs: [
                self.bytes(0x0c0b0448, 720)?.try_into().unwrap(),
                self.bytes(0x0c0b0718, 720)?.try_into().unwrap(),
            ],
            program_layout: self.layout,
        };
        for i in 0..20 {
            let a = p + 20 * i as u32;
            result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
            result.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
        }
        for (i, group) in result.groups.iter_mut().enumerate() {
            let a = g + 12 * i as u32;
            *group = EffectCoefficientGroup {
                first: self.word(a + 4)? as i32,
                second: self.word(a)? as i32,
                action: self.bytes(a + 8, 1)?[0],
                range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
            };
        }
        for (i, v) in result.voices.iter_mut().enumerate() {
            *v = self.table(0x0c0beb04 + 32 * i as u32)?;
        }
        Ok(result)
    }
    pub fn rotary_tables(
        &self,
    ) -> Result<radias_synth_domain::rotary_effect::RotaryTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup, rotary_effect::RotaryTables,
        };
        let descriptor = self.word(0x0c0cceac + 29 * 4)?;
        if self.bytes(descriptor + 19, 1)?[0] != 19 || self.bytes(descriptor + 20, 1)?[0] != 69 {
            return Err("Unexpected Rotary definition size");
        }
        let p = self.word(descriptor + 48)?;
        let g = self.word(0x0c0ccfa4 + 29 * 4)?;
        let zero = self.parameter_range(0)?;
        let mut result = RotaryTables {
            ranges: [zero; 19],
            dependencies: [[0; 2]; 19],
            groups: [EffectCoefficientGroup {
                first: 0,
                second: 0,
                action: 0,
                range: zero,
            }; 69],
            acceleration_range: self.parameter_range(17)?,
        };
        for i in 0..19 {
            let a = p + 20 * i as u32;
            result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
            result.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
        }
        for (i, group) in result.groups.iter_mut().enumerate() {
            let a = g + 12 * i as u32;
            *group = EffectCoefficientGroup {
                first: self.word(a + 4)? as i32,
                second: self.word(a)? as i32,
                action: self.bytes(a + 8, 1)?[0],
                range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
            };
        }
        Ok(result)
    }
    pub fn vibrato_tables(
        &self,
    ) -> Result<radias_synth_domain::vibrato_effect::VibratoTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup, vibrato_effect::VibratoTables,
        };
        let descriptor = self.word(0x0c0cceac + 28 * 4)?;
        if self.bytes(descriptor + 19, 1)?[0] != 10 || self.bytes(descriptor + 20, 1)?[0] != 28 {
            return Err("Unexpected Vibrato definition size");
        }
        let parameters = self.word(descriptor + 48)?;
        let groups = self.word(0x0c0ccfa4 + 28 * 4)?;
        let zero = self.parameter_range(0)?;
        let mut result = VibratoTables {
            ranges: [zero; 10],
            dependencies: [[0; 2]; 10],
            groups: [EffectCoefficientGroup {
                first: 0,
                second: 0,
                action: 0,
                range: zero,
            }; 28],
            lfo_mapping: self.lfo_mapping(EffectBank::Insert, EffectKind::new(28).unwrap())?,
            tempo: crate::firmware::lfo_tempo_tables(self.system)?,
        };
        for i in 0..10 {
            let a = parameters + 20 * i as u32;
            result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
            result.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
        }
        for (i, g) in result.groups.iter_mut().enumerate() {
            let a = groups + 12 * i as u32;
            *g = EffectCoefficientGroup {
                first: self.word(a + 4)? as i32,
                second: self.word(a)? as i32,
                action: self.bytes(a + 8, 1)?[0],
                range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
            };
        }
        Ok(result)
    }
    pub fn pitch_grain_tables(
        &self,
    ) -> Result<radias_synth_domain::pitch_grain_shifter::PitchGrainTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup,
            pitch_grain_shifter::{PitchGrainDefinition, PitchGrainTables},
        };
        let zero = self.parameter_range(0)?;
        let definition =
            |kind: u8, count: u8, group_count: u8| -> Result<PitchGrainDefinition, &'static str> {
                let descriptor = self.word(0x0c0cceac + 4 * u32::from(kind))?;
                if self.bytes(descriptor + 19, 1)?[0] != count
                    || self.bytes(descriptor + 20, 1)?[0] != group_count
                {
                    return Err("Unexpected Pitch/Grain insert definition size");
                }
                let parameters = self.word(descriptor + 48)?;
                let groups = self.word(0x0c0ccfa4 + 4 * u32::from(kind))?;
                let mut result = PitchGrainDefinition {
                    ranges: [zero; 12],
                    dependencies: [[0; 2]; 12],
                    groups: [EffectCoefficientGroup {
                        first: 0,
                        second: 0,
                        action: 0,
                        range: zero,
                    }; 42],
                    group_count: usize::from(group_count),
                    lfo_mapping: self
                        .lfo_mapping(EffectBank::Insert, EffectKind::new(kind).unwrap())?,
                };
                for i in 0..usize::from(count) {
                    let a = parameters + 20 * i as u32;
                    result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
                    result.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
                }
                for (i, g) in result.groups[..usize::from(group_count)]
                    .iter_mut()
                    .enumerate()
                {
                    let a = groups + 12 * i as u32;
                    *g = EffectCoefficientGroup {
                        first: self.word(a + 4)? as i32,
                        second: self.word(a)? as i32,
                        action: self.bytes(a + 8, 1)?[0],
                        range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
                    };
                }
                Ok(result)
            };
        if self.bytes(0x0c0b77b0 + 27, 1)?[0] != 0 {
            return Err("Unexpected Grain delay owner");
        }
        Ok(PitchGrainTables {
            definitions: [definition(26, 12, 42)?, definition(27, 11, 30)?],
            pitch_ratio: self.table(0x0c0c2260)?,
            fine_ratio: self.table(0x0c0c238c)?,
            feedback_range: self.parameter_range(17)?,
            time: self.delay_time_tables()?,
            grain_period: self.table(0x0c0ac240)?,
            clock_notes: self.table(0x0c0ac440)?,
            tempo: crate::firmware::lfo_tempo_tables(self.system)?,
        })
    }
    pub fn tremolo_ring_mod_tables(
        &self,
    ) -> Result<radias_synth_domain::tremolo_ring_mod_effect::TremoloRingModTables, &'static str>
    {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup,
            tremolo_ring_mod_effect::{TremoloRingModDefinition, TremoloRingModTables},
        };
        let zero = self.parameter_range(0)?;
        let definition = |kind: u8,
                          count: u8,
                          group_count: u8|
         -> Result<TremoloRingModDefinition, &'static str> {
            let descriptor = self.word(0x0c0cceac + 4 * u32::from(kind))?;
            if self.bytes(descriptor + 19, 1)?[0] != count
                || self.bytes(descriptor + 20, 1)?[0] != group_count
            {
                return Err("Unexpected Tremolo/Ring Mod insert definition size");
            }
            let parameters = self.word(descriptor + 48)?;
            let groups = self.word(0x0c0ccfa4 + 4 * u32::from(kind))?;
            let mut result = TremoloRingModDefinition {
                ranges: [zero; 15],
                dependencies: [[0; 2]; 15],
                groups: [EffectCoefficientGroup {
                    first: 0,
                    second: 0,
                    action: 0,
                    range: zero,
                }; 21],
                group_count: usize::from(group_count),
                lfo_mapping: self
                    .lfo_mapping(EffectBank::Insert, EffectKind::new(kind).unwrap())?,
            };
            for i in 0..usize::from(count) {
                let a = parameters + 20 * i as u32;
                result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
                result.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
            }
            for (i, g) in result.groups[..usize::from(group_count)]
                .iter_mut()
                .enumerate()
            {
                let a = groups + 12 * i as u32;
                *g = EffectCoefficientGroup {
                    first: self.word(a + 4)? as i32,
                    second: self.word(a)? as i32,
                    action: self.bytes(a + 8, 1)?[0],
                    range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
                };
            }
            Ok(result)
        };
        Ok(TremoloRingModTables {
            definitions: [definition(24, 10, 13)?, definition(25, 15, 21)?],
            fixed_frequency: self.table(0x0c0c2100)?,
            note_frequency: self.table(0x0c0c1f00)?,
            fine_frequency: self.table(0x0c0c238c)?,
            tempo: crate::firmware::lfo_tempo_tables(self.system)?,
        })
    }
    pub fn flanger_phaser_tables(
        &self,
    ) -> Result<radias_synth_domain::flanger_phaser_effect::FlangerPhaserTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup,
            flanger_phaser_effect::{FlangerPhaserDefinition, FlangerPhaserTables},
        };
        let zero = self.parameter_range(0)?;
        let definition = |kind: u8,
                          count: u8,
                          group_count: u8|
         -> Result<FlangerPhaserDefinition, &'static str> {
            let descriptor = self.word(0x0c0cceac + 4 * u32::from(kind))?;
            if self.bytes(descriptor + 19, 1)?[0] != count
                || self.bytes(descriptor + 20, 1)?[0] != group_count
            {
                return Err("Unexpected Flanger/Phaser insert definition size");
            }
            let parameters = self.word(descriptor + 48)?;
            let groups = self.word(0x0c0ccfa4 + 4 * u32::from(kind))?;
            let mut result = FlangerPhaserDefinition {
                ranges: [zero; 16],
                dependencies: [[0; 2]; 16],
                groups: [EffectCoefficientGroup {
                    first: 0,
                    second: 0,
                    action: 0,
                    range: zero,
                }; 34],
                group_count: usize::from(group_count),
                lfo_mapping: self
                    .lfo_mapping(EffectBank::Insert, EffectKind::new(kind).unwrap())?,
            };
            for i in 0..usize::from(count) {
                let a = parameters + 20 * i as u32;
                result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
                result.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
            }
            for (i, g) in result.groups[..usize::from(group_count)]
                .iter_mut()
                .enumerate()
            {
                let a = groups + 12 * i as u32;
                *g = EffectCoefficientGroup {
                    first: self.word(a + 4)? as i32,
                    second: self.word(a)? as i32,
                    action: self.bytes(a + 8, 1)?[0],
                    range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
                };
            }
            Ok(result)
        };
        let raw = self.bytes(0x0c0abc5c, 228)?;
        Ok(FlangerPhaserTables {
            definitions: [definition(22, 16, 34)?, definition(23, 15, 20)?],
            feedback_range: self.parameter_range(17)?,
            response: self.table(0x0c0c2558)?,
            milliseconds: core::array::from_fn(|i| {
                u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]])
            }),
            cutoff: self.table(0x0c0ab06c)?,
            routing: self.routing_tables()?,
            tempo: crate::firmware::lfo_tempo_tables(self.system)?,
        })
    }
    pub fn chorus_effect_tables(
        &self,
    ) -> Result<radias_synth_domain::chorus_effect::ChorusEffectTables, &'static str> {
        use radias_synth_domain::{
            chorus_effect::ChorusEffectTables, effect_parameters::EffectCoefficientGroup,
        };
        let descriptor = self.word(0x0c0cceac + 20 * 4)?;
        let parameters = self.word(descriptor + 48)?;
        let groups = self.word(0x0c0ccfa4 + 20 * 4)?;
        let zero = self.parameter_range(0)?;
        let bytes = self.bytes(0x0c0abb6c, 256)?;
        let mut result = ChorusEffectTables {
            ranges: [zero; 9],
            dependencies: [0; 9],
            groups: [EffectCoefficientGroup {
                first: 0,
                second: 0,
                action: 0,
                range: zero,
            }; 31],
            time: self.delay_time_tables()?,
            milliseconds: core::array::from_fn(|i| {
                u16::from_be_bytes([bytes[2 * i], bytes[2 * i + 1]])
            }),
            modulation_rate: self.table(0x0c0c3754)?,
        };
        if self.bytes(descriptor + 19, 1)?[0] != 9
            || self.bytes(descriptor + 20, 1)?[0] != 31
            || self.bytes(0x0c0b77b0 + 20, 1)?[0] != 0
        {
            return Err("Unexpected Chorus insert definition or delay owner");
        }
        for i in 0..9 {
            let a = parameters + 20 * i as u32;
            result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
            result.dependencies[i] = self.word(a + 12)?;
            if self.word(a + 16)? != 0 {
                return Err("Unexpected Chorus upper dependency bank");
            }
        }
        for (i, g) in result.groups.iter_mut().enumerate() {
            let a = groups + 12 * i as u32;
            *g = EffectCoefficientGroup {
                first: self.word(a + 4)? as i32,
                second: self.word(a)? as i32,
                action: self.bytes(a + 8, 1)?[0],
                range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
            };
        }
        Ok(result)
    }
    pub fn ensemble_effect_tables(
        &self,
    ) -> Result<radias_synth_domain::ensemble_effect::EnsembleEffectTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup, ensemble_effect::EnsembleEffectTables,
        };
        let descriptor = self.word(0x0c0cceac + 21 * 4)?;
        let parameters = self.word(descriptor + 48)?;
        let groups = self.word(0x0c0ccfa4 + 21 * 4)?;
        let zero = self.parameter_range(0)?;
        let mut result = EnsembleEffectTables {
            ranges: [zero; 3],
            dependencies: [0; 3],
            groups: [EffectCoefficientGroup {
                first: 0,
                second: 0,
                action: 0,
                range: zero,
            }; 27],
            speed_words: [self.table(0x0c0c14f8)?, self.table(0x0c0c16f4)?],
            speed_shape_range: self.parameter_range(31)?,
        };
        if self.bytes(descriptor + 19, 1)?[0] != 3 || self.bytes(descriptor + 20, 1)?[0] != 27 {
            return Err("Unexpected Ensemble insert definition size");
        }
        for i in 0..3 {
            let a = parameters + 20 * i as u32;
            result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
            result.dependencies[i] = self.word(a + 12)?;
            if self.word(a + 16)? != 0 {
                return Err("Unexpected Ensemble upper dependency bank");
            }
        }
        for (i, g) in result.groups.iter_mut().enumerate() {
            let a = groups + 12 * i as u32;
            *g = EffectCoefficientGroup {
                first: self.word(a + 4)? as i32,
                second: self.word(a)? as i32,
                action: self.bytes(a + 8, 1)?[0],
                range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
            };
        }
        Ok(result)
    }
    pub fn early_reflect_effect_tables(
        &self,
    ) -> Result<radias_synth_domain::early_reflect_effect::EarlyReflectEffectTables, &'static str>
    {
        use radias_synth_domain::{
            early_reflect_effect::EarlyReflectEffectTables,
            effect_parameters::EffectCoefficientGroup,
        };
        let descriptor = self.word(0x0c0cceac + 12 * 4)?;
        let parameters = self.word(descriptor + 48)?;
        let groups = self.word(0x0c0ccfa4 + 12 * 4)?;
        let zero = self.parameter_range(0)?;
        let mut result = EarlyReflectEffectTables {
            ranges: [zero; 9],
            dependencies: [[0; 2]; 9],
            groups: [EffectCoefficientGroup {
                first: 0,
                second: 0,
                action: 0,
                range: zero,
            }; 61],
            time: self.early_reflect_time_tables()?,
            routing: self.routing_tables()?,
            pair_transition: self.pair_transition_tables()?,
            type_coefficients: [[0; 16]; 4],
            transition_programs: [[0; 2]; 5],
        };
        if self.bytes(descriptor + 19, 1)?[0] != 9 || self.bytes(descriptor + 20, 1)?[0] != 61 {
            return Err("Unexpected Early Reflect insert definition size");
        }
        for i in 0..9 {
            let a = parameters + 20 * i as u32;
            result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
            result.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
        }
        for (i, g) in result.groups.iter_mut().enumerate() {
            let a = groups + 12 * i as u32;
            *g = EffectCoefficientGroup {
                first: self.word(a + 4)? as i32,
                second: self.word(a)? as i32,
                action: self.bytes(a + 8, 1)?[0],
                range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
            };
        }
        for (kind, words) in result.type_coefficients.iter_mut().enumerate() {
            for (tap, v) in words.iter_mut().enumerate() {
                *v = self.word(0x0c0bf090 + 16 * tap as u32 + 4 * kind as u32)?;
            }
        }
        for (part, words) in result.transition_programs.iter_mut().enumerate() {
            for (role, v) in words.iter_mut().enumerate() {
                let pointer =
                    self.word(if role == 0 { 0x0c0ccbf0 } else { 0x0c0ccc18 } + part as u32 * 4)?;
                *v = self
                    .bytes(pointer, 6)?
                    .iter()
                    .fold(0u64, |word, byte| (word << 8) | u64::from(*byte));
            }
        }
        Ok(result)
    }
    pub fn reverb_effect_tables(
        &self,
    ) -> Result<radias_synth_domain::reverb_effect::ReverbEffectTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup, reverb_effect::ReverbEffectTables,
        };
        let descriptor = self.word(0x0c0cceac + 11 * 4)?;
        let parameters = self.word(descriptor + 48)?;
        let groups = self.word(0x0c0ccfa4 + 11 * 4)?;
        let zero = self.parameter_range(0)?;
        let mut result = ReverbEffectTables {
            ranges: [zero; 11],
            dependencies: [[0; 2]; 11],
            groups: [EffectCoefficientGroup {
                first: 0,
                second: 0,
                action: 0,
                range: zero,
            }; 49],
            allocation: self.buffer_allocation_tables()?,
            time: self.reverb_time_tables()?,
            type_coefficients: [[0; 4]; 3],
        };
        if self.bytes(descriptor + 19, 1)?[0] != 11 || self.bytes(descriptor + 20, 1)?[0] != 49 {
            return Err("Unexpected Reverb insert definition size");
        }
        for i in 0..11 {
            let a = parameters + 20 * i as u32;
            result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
            result.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
        }
        for (i, g) in result.groups.iter_mut().enumerate() {
            let a = groups + 12 * i as u32;
            *g = EffectCoefficientGroup {
                first: self.word(a + 4)? as i32,
                second: self.word(a)? as i32,
                action: self.bytes(a + 8, 1)?[0],
                range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
            };
        }
        for (kind, values) in result.type_coefficients.iter_mut().enumerate() {
            for (plane, v) in values.iter_mut().enumerate() {
                *v = self.word(0x0c0c5c9c + 4 * (plane * 3 + kind) as u32)?;
            }
        }
        Ok(result)
    }
    pub fn buffer_allocation_tables(
        &self,
    ) -> Result<
        radias_synth_domain::effect_buffer_allocation::EffectBufferAllocationTables,
        &'static str,
    > {
        use radias_synth_domain::effect_buffer_allocation::EffectBufferAllocationTables;
        let words = |address: u32, count: usize| -> Result<Vec<u16>, &'static str> {
            Ok(self
                .bytes(address, count * 2)?
                .chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect())
        };
        let mut templates = [[0; 80]; 31];
        for (kind, template) in templates.iter_mut().enumerate() {
            let descriptor = self.word(0x0c0cceac + 4 * kind as u32)?;
            let count = usize::from(self.bytes(descriptor + 20, 1)?[0]);
            if count > 80 {
                return Err("Effect coefficient template too long");
            }
            let pointer = self.word(0x0c0ccc40 + 4 * kind as u32)?;
            if pointer != 0 {
                for (i, word) in template[..count].iter_mut().enumerate() {
                    *word = self.word(pointer + 4 * i as u32)?;
                }
            }
        }
        let mut reverb_templates = [[0; 80]; 3];
        for (kind, template) in reverb_templates.iter_mut().enumerate() {
            let pointer = self.word(0x0c0cccbc + 4 * kind as u32)?;
            for (i, word) in template[..49].iter_mut().enumerate() {
                *word = self.word(pointer + 4 * i as u32)?;
            }
        }
        Ok(EffectBufferAllocationTables {
            buffers: self.buffer_tables()?,
            time: self.delay_time_tables()?,
            early: self.early_reflect_time_tables()?,
            milliseconds: [
                words(0x0c0ab86c, 128)?.try_into().unwrap(),
                words(0x0c0ab76c, 128)?.try_into().unwrap(),
                words(0x0c0abb6c, 128)?.try_into().unwrap(),
            ],
            templates,
            reverb_templates,
            grain_period: self.table(0x0c0ac240)?,
            clock_notes: self.table(0x0c0ac440)?,
            flanger_response: self.table(0x0c0c2558)?,
            flanger_milliseconds: words(0x0c0abc5c, 114)?.try_into().unwrap(),
            flanger_sync_words: self.table(0x0c0ab06c)?,
        })
    }
    pub fn buffer_tables(
        &self,
    ) -> Result<radias_synth_domain::effect_buffers::EffectBufferTables, &'static str> {
        use radias_synth_domain::effect_buffers::{EffectBufferTables, effect_uses_buffer};
        let mut profiles = [0u8; 31];
        for (kind, profile) in profiles.iter_mut().enumerate() {
            let descriptor = self.word(0x0c0cceac + 4 * kind as u32)?;
            *profile = self.bytes(descriptor + 45, 1)?[0];
            if *profile >= 10 {
                return Err("Unknown effect buffer profile");
            }
            let expected = if !(8..30).contains(&kind) {
                false
            } else {
                let raw = self.bytes(0x0c074244 + 2 * (kind as u32 - 8), 2)?;
                let displacement = i16::from_be_bytes(raw.try_into().unwrap());
                match 0x0c074226u32.wrapping_add(displacement as i32 as u32) {
                    0x0c074270 => true,
                    0x0c074272 => false,
                    _ => return Err("Unknown buffer eligibility branch"),
                }
            };
            if effect_uses_buffer(kind as u32) != expected {
                return Err("Native buffer eligibility does not match SYS");
            }
        }
        Ok(EffectBufferTables {
            profiles,
            frames: self.table(0x0c0ac484)?,
        })
    }
    pub fn early_reflect_time_tables(
        &self,
    ) -> Result<radias_synth_domain::early_reflect_time::EarlyReflectTimeTables, &'static str> {
        use radias_synth_domain::early_reflect_time::EarlyReflectTimeTables;
        let size = self.bytes(0x0c0bef70, 256)?;
        let taps = self.bytes(0x0c0bf070, 32)?;
        Ok(EarlyReflectTimeTables {
            pre_delay: self.bytes(0x0c0beef0, 128)?.try_into().unwrap(),
            size: core::array::from_fn(|i| u16::from_be_bytes([size[2 * i], size[2 * i + 1]])),
            tap_time: core::array::from_fn(|i| u16::from_be_bytes([taps[2 * i], taps[2 * i + 1]])),
        })
    }
    pub fn reverb_time_tables(
        &self,
    ) -> Result<radias_synth_domain::reverb_time::ReverbTimeTables, &'static str> {
        use radias_synth_domain::reverb_time::ReverbTimeTables;
        let large_indices: [u8; 128] = self.bytes(0x0c0ca80c, 128)?.try_into().unwrap();
        let small_indices: [u8; 128] = self.bytes(0x0c0ca88c, 128)?.try_into().unwrap();
        if large_indices.iter().any(|&i| i >= 100) || small_indices.iter().any(|&i| i >= 30) {
            return Err("Unexpected Reverb Time mapping domain");
        }
        let addresses: [([u32; 4], usize, usize); 8] = [
            ([0x0c0c533c, 0x0c0c54cc, 0x0c0c565c, 0], 100, 3),
            ([0x0c0c57ec, 0x0c0c597c, 0x0c0c5b0c, 0], 100, 3),
            ([0x0c0c51d4, 0x0c0c524c, 0x0c0c52c4, 0], 30, 3),
            ([0x0c0c3b54, 0x0c0c3ce4, 0x0c0c3e74, 0x0c0c4004], 100, 4),
            ([0x0c0c4194, 0x0c0c4324, 0x0c0c44b4, 0x0c0c4644], 100, 4),
            ([0x0c0c47d4, 0x0c0c4964, 0x0c0c4af4, 0x0c0c4c84], 100, 4),
            ([0x0c0c4e14, 0x0c0c4e8c, 0x0c0c4f04, 0x0c0c4f7c], 30, 4),
            ([0x0c0c4ff4, 0x0c0c506c, 0x0c0c50e4, 0x0c0c515c], 30, 4),
        ];
        let mut coefficients = [[[0; 4]; 100]; 8];
        for (profile, (planes, rows, count)) in addresses.into_iter().enumerate() {
            for (plane, address) in planes[..count].iter().copied().enumerate() {
                for (i, row) in coefficients[profile][..rows].iter_mut().enumerate() {
                    row[plane] = self.word(address + 4 * i as u32)?;
                }
            }
        }
        Ok(ReverbTimeTables {
            large_indices,
            small_indices,
            coefficients,
        })
    }
    pub fn mod_delay_tables(
        &self,
    ) -> Result<radias_synth_domain::mod_delay::ModDelayTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup,
            mod_delay::{ModDelayDefinition, ModDelayTables},
        };
        let definition = |kind: u32,
                          count: usize,
                          group_count: usize|
         -> Result<ModDelayDefinition, &'static str> {
            let descriptor = self.word(0x0c0cceac + 4 * kind)?;
            let parameters = self.word(descriptor + 48)?;
            let groups = self.word(0x0c0ccfa4 + 4 * kind)?;
            let zero = self.parameter_range(0)?;
            let mut result = ModDelayDefinition {
                ranges: [zero; 18],
                dependencies: [[0; 2]; 18],
                groups: [EffectCoefficientGroup {
                    first: 0,
                    second: 0,
                    action: 0,
                    range: zero,
                }; 33],
                group_count,
                lfo_mapping: self
                    .lfo_mapping(EffectBank::Insert, EffectKind::new(kind as u8).unwrap())?,
            };
            if usize::from(self.bytes(descriptor + 19, 1)?[0]) != count
                || usize::from(self.bytes(descriptor + 20, 1)?[0]) != group_count
            {
                return Err("Unexpected Mod Delay definition size");
            }
            for i in 0..count {
                let a = parameters + 20 * i as u32;
                result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
                result.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
            }
            for (i, group) in result.groups[..group_count].iter_mut().enumerate() {
                let a = groups + 12 * i as u32;
                *group = EffectCoefficientGroup {
                    first: self.word(a + 4)? as i32,
                    second: self.word(a)? as i32,
                    action: self.bytes(a + 8, 1)?[0],
                    range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
                };
            }
            Ok(result)
        };
        let milliseconds = |address| -> Result<[u16; 128], &'static str> {
            let raw = self.bytes(address, 256)?;
            Ok(core::array::from_fn(|i| {
                u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]])
            }))
        };
        Ok(ModDelayTables {
            definitions: [
                definition(17, 11, 31)?,
                definition(18, 11, 31)?,
                definition(19, 18, 33)?,
            ],
            milliseconds: [milliseconds(0x0c0ab86c)?, milliseconds(0x0c0ab76c)?],
            modulation_rate: self.table(0x0c0c3754)?,
            time: self.delay_time_tables()?,
            tempo: crate::firmware::lfo_tempo_tables(self.system)?,
        })
    }
    pub fn auto_pan_delay_tables(
        &self,
    ) -> Result<radias_synth_domain::auto_pan_delay::AutoPanDelayTables, &'static str> {
        use radias_synth_domain::{
            auto_pan_delay::{AutoPanDelayDefinition, AutoPanDelayTables},
            effect_parameters::EffectCoefficientGroup,
        };
        let definition =
            |kind: u32, count: usize| -> Result<AutoPanDelayDefinition, &'static str> {
                let descriptor = self.word(0x0c0cceac + 4 * kind)?;
                let parameters = self.word(descriptor + 48)?;
                let groups = self.word(0x0c0ccfa4 + 4 * kind)?;
                let zero = self.parameter_range(0)?;
                let mut result = AutoPanDelayDefinition {
                    ranges: [zero; 20],
                    dependencies: [0; 20],
                    groups: [EffectCoefficientGroup {
                        first: 0,
                        second: 0,
                        action: 0,
                        range: zero,
                    }; 32],
                    lfo_mapping: self
                        .lfo_mapping(EffectBank::Insert, EffectKind::new(kind as u8).unwrap())?,
                };
                if self.bytes(descriptor + 20, 1)?[0] != 32
                    || usize::from(self.bytes(descriptor + 19, 1)?[0]) != count
                {
                    return Err("Unexpected AutoPanDelay definition size");
                }
                for i in 0..count {
                    let a = parameters + 20 * i as u32;
                    result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
                    result.dependencies[i] = self.word(a + 12)?;
                    if self.word(a + 16)? != 0 {
                        return Err("Unexpected second AutoPanDelay dependency mask");
                    }
                }
                for (i, group) in result.groups.iter_mut().enumerate() {
                    let a = groups + 12 * i as u32;
                    *group = EffectCoefficientGroup {
                        first: self.word(a + 4)? as i32,
                        second: self.word(a)? as i32,
                        action: self.bytes(a + 8, 1)?[0],
                        range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
                    };
                }
                Ok(result)
            };
        Ok(AutoPanDelayTables {
            definitions: [definition(15, 19)?, definition(16, 20)?],
            time: self.delay_time_tables()?,
            tempo: crate::firmware::lfo_tempo_tables(self.system)?,
        })
    }
    pub fn delay_effect_tables(
        &self,
    ) -> Result<radias_synth_domain::delay_effect::DelayEffectTables, &'static str> {
        use radias_synth_domain::{
            delay_effect::{DelayEffectDefinition, DelayEffectTables},
            effect_parameters::EffectCoefficientGroup,
        };
        let definition = |kind: u32, count: usize| -> Result<DelayEffectDefinition, &'static str> {
            let descriptor = self.word(0x0c0cceac + 4 * kind)?;
            let parameters = self.word(descriptor + 48)?;
            let groups = self.word(0x0c0ccfa4 + 4 * kind)?;
            let zero = self.parameter_range(0)?;
            let mut result = DelayEffectDefinition {
                ranges: [zero; 17],
                dependencies: [0; 17],
                groups: [EffectCoefficientGroup {
                    first: 0,
                    second: 0,
                    action: 0,
                    range: zero,
                }; 31],
                group_count: usize::from(self.bytes(descriptor + 20, 1)?[0]),
            };
            if result.group_count > 31 || usize::from(self.bytes(descriptor + 19, 1)?[0]) != count {
                return Err("Unexpected delay definition size");
            }
            for i in 0..count {
                let a = parameters + 20 * i as u32;
                result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
                result.dependencies[i] = self.word(a + 12)?;
                if self.word(a + 16)? != 0 {
                    return Err("Unexpected second delay dependency mask");
                }
            }
            for (i, group) in result.groups[..result.group_count].iter_mut().enumerate() {
                let a = groups + 12 * i as u32;
                *group = EffectCoefficientGroup {
                    first: self.word(a + 4)? as i32,
                    second: self.word(a)? as i32,
                    action: self.bytes(a + 8, 1)?[0],
                    range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
                };
            }
            Ok(result)
        };
        Ok(DelayEffectTables {
            definitions: [definition(13, 17)?, definition(14, 13)?],
            time: self.delay_time_tables()?,
        })
    }
    pub fn delay_time_tables(
        &self,
    ) -> Result<radias_synth_domain::delay_time::DelayTimeTables, &'static str> {
        let words = |address: u32, count: usize| -> Result<Vec<u16>, &'static str> {
            Ok(self
                .bytes(address, count * 2)?
                .chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect())
        };
        Ok(radias_synth_domain::delay_time::DelayTimeTables {
            free_ratio: words(0x0c0aadd0, 128)?.try_into().unwrap(),
            sync_ratio: words(0x0c0aaed0, 128)?.try_into().unwrap(),
            notes: words(0x0c0aafd0, 14)?.try_into().unwrap(),
            lcr_milliseconds: words(0x0c0ab36c, 128)?.try_into().unwrap(),
            stereo_milliseconds: words(0x0c0ab26c, 128)?.try_into().unwrap(),
            feedback_ratio: self.table(0x0c0abd40)?,
            feedback_threshold: words(0x0c0abf40, 128)?.try_into().unwrap(),
            feedback_coefficient: self.table(0x0c0ac040)?,
            feedback_before_table: self.word(0x0c0ac03c)?,
        })
    }
    pub fn wah_tables(
        &self,
    ) -> Result<radias_synth_domain::wah_effect::WahEffectTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup, wah_effect::WahEffectTables,
        };
        let descriptor = self.word(0x0c0cceac + 5 * 4)?;
        let parameters = self.word(descriptor + 48)?;
        let groups = self.word(0x0c0ccfa4 + 5 * 4)?;
        let zero = self.parameter_range(0)?;
        let frequency = self.table::<762>(0x0c0bf3f0)?;
        let resonance = self.table::<762>(0x0c0bffd8)?;
        let first = self.bytes(0x0c0c1aec, 202)?;
        let second = self.bytes(0x0c0c1bb6, 202)?;
        let mut result = WahEffectTables {
            ranges: [zero; 17],
            dependencies: [[0; 2]; 17],
            groups: [EffectCoefficientGroup {
                first: 0,
                second: 0,
                action: 0,
                range: zero,
            }; 56],
            mode: self.table(0x0c0bf390)?,
            resonance_bound: self.table(0x0c0bf3c0)?,
            frequency: core::array::from_fn(|i| core::array::from_fn(|j| frequency[6 * i + j])),
            resonance: core::array::from_fn(|i| core::array::from_fn(|j| resonance[6 * i + j])),
            frequency_modulation: self.table(0x0c0c18f0)?,
            response: self.table(0x0c0c2f54)?,
            response_complement: self.table(0x0c0c3154)?,
            free_rate: self.bytes(0x0c0c1c80, 128)?.try_into().unwrap(),
            sync_divisors: self.table(0x0c0ac440)?,
            rate_coefficients: core::array::from_fn(|i| {
                [
                    u16::from_be_bytes([first[2 * i], first[2 * i + 1]]),
                    u16::from_be_bytes([second[2 * i], second[2 * i + 1]]),
                ]
            }),
            lfo_mapping: self.lfo_mapping(EffectBank::Insert, EffectKind::new(5).unwrap())?,
        };
        for i in 0..17 {
            let a = parameters + 20 * i as u32;
            result.ranges[i] = self.parameter_range(self.bytes(a + 10, 1)?[0])?;
            result.dependencies[i] = [self.word(a + 12)?, self.word(a + 16)?];
        }
        for (i, group) in result.groups.iter_mut().enumerate() {
            let a = groups + 12 * i as u32;
            *group = EffectCoefficientGroup {
                first: self.word(a + 4)? as i32,
                second: self.word(a)? as i32,
                action: self.bytes(a + 8, 1)?[0],
                range: self.parameter_range(self.bytes(a + 9, 1)?[0])?,
            };
        }
        Ok(result)
    }
    pub fn equalizer_effect_tables(
        &self,
    ) -> Result<radias_synth_domain::equalizer_effect::EqualizerEffectTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup,
            equalizer_effect::{EqualizerEffectDefinition, EqualizerEffectTables},
        };
        let definition = |kind: u32| -> Result<EqualizerEffectDefinition, &'static str> {
            let descriptor = self.word(0x0c0cceac + kind * 4)?;
            let parameters = self.word(descriptor + 48)?;
            let coefficients = self.word(0x0c0ccfa4 + kind * 4)?;
            let zero = self.parameter_range(0)?;
            let parameter_count = self.bytes(descriptor + 19, 1)?[0];
            let group_count = self.bytes(descriptor + 20, 1)?[0];
            let mut result = EqualizerEffectDefinition {
                ranges: [zero; 15],
                dependencies: [[0; 2]; 15],
                groups: [EffectCoefficientGroup {
                    first: 0,
                    second: 0,
                    action: 0,
                    range: zero,
                }; 43],
                parameter_count,
                group_count,
            };
            for i in 0..usize::from(parameter_count) {
                let record = parameters + 20 * i as u32;
                result.ranges[i] = self.parameter_range(self.bytes(record + 10, 1)?[0])?;
                result.dependencies[i] = [self.word(record + 12)?, self.word(record + 16)?];
            }
            for i in 0..usize::from(group_count) {
                let record = coefficients + 12 * i as u32;
                result.groups[i] = EffectCoefficientGroup {
                    first: self.word(record + 4)? as i32,
                    second: self.word(record)? as i32,
                    action: self.bytes(record + 8, 1)?[0],
                    range: self.parameter_range(self.bytes(record + 9, 1)?[0])?,
                };
            }
            Ok(result)
        };
        Ok(EqualizerEffectTables {
            definitions: [definition(6)?, definition(7)?],
            gain: self.table(0x0c0c13d4)?,
            distortion_gain: self.table(0x0c0c3954)?,
        })
    }
    pub fn equalizer_tables(
        &self,
    ) -> Result<radias_synth_domain::effect_equalizer::EffectEqualizerTables, &'static str> {
        let frequency = self.bytes(0x0c089bd8, 118)?;
        let a = self.table::<80>(0x0c089fe4)?;
        let b = self.table::<128>(0x0c08a124)?;
        Ok(
            radias_synth_domain::effect_equalizer::EffectEqualizerTables {
                frequency: core::array::from_fn(|i| {
                    u16::from_be_bytes([frequency[2 * i], frequency[2 * i + 1]])
                }),
                pole: self.table(0x0c089c50)?,
                gain: self.table(0x0c089d40)?,
                q: self.table(0x0c089e64)?,
                curve_a: core::array::from_fn(|i| core::array::from_fn(|j| a[5 * i + j])),
                curve_b: core::array::from_fn(|i| core::array::from_fn(|j| b[4 * i + j])),
            },
        )
    }
    pub fn tube_tables(
        &self,
    ) -> Result<radias_synth_domain::tube_effect::TubeEffectTables, &'static str> {
        use radias_synth_domain::{
            effect_parameters::EffectCoefficientGroup, tube_effect::TubeEffectTables,
        };
        let descriptor = self.word(0x0c0cceac + 9 * 4)?;
        let parameters = self.word(descriptor + 48)?;
        let coefficients = self.word(0x0c0ccfa4 + 9 * 4)?;
        let zero = self.parameter_range(0)?;
        let mut result = TubeEffectTables {
            parameter_ranges: [zero; 13],
            dependencies: [0; 13],
            groups: [EffectCoefficientGroup {
                first: 0,
                second: 0,
                action: 0,
                range: zero,
            }; 24],
            gain: self.table(0x0c0be8e4)?,
        };
        for index in 0..13 {
            let record = parameters + 20 * index as u32;
            result.parameter_ranges[index] =
                self.parameter_range(self.bytes(record + 10, 1)?[0])?;
            result.dependencies[index] = self.word(record + 12)?;
            if result.dependencies[index] & 255 != 0 || self.word(record + 16)? != 0 {
                return Err("Tube dependencies outside original coefficient bank");
            }
        }
        for (index, group) in result.groups.iter_mut().enumerate() {
            let record = coefficients + 12 * index as u32;
            *group = EffectCoefficientGroup {
                first: self.word(record + 4)? as i32,
                second: self.word(record)? as i32,
                action: self.bytes(record + 8, 1)?[0],
                range: self.parameter_range(self.bytes(record + 9, 1)?[0])?,
            };
        }
        Ok(result)
    }
    pub fn cabinet_tables(
        &self,
    ) -> Result<radias_synth_domain::cabinet_effect::CabinetEffectTables, &'static str> {
        use radias_synth_domain::cabinet_effect::CabinetEffectTables;
        let air = self.table::<44>(0x0c0c0bc0)?;
        let coefficients = self.table::<473>(0x0c0c0c70)?;
        let groups = self.word(0x0c0ccfa4 + 8 * 4)?;
        let trim = groups + 5 * 12;
        if self.bytes(trim + 8, 1)?[0] != 11 {
            return Err("Unexpected Cabinet trim operation");
        }
        Ok(CabinetEffectTables {
            coefficients: core::array::from_fn(|kind| {
                core::array::from_fn(|index| coefficients[index * 11 + kind])
            }),
            air: core::array::from_fn(|kind| {
                core::array::from_fn(|index| air[index * 11 + kind] as i32)
            }),
            trim_first: self.word(trim + 4)? as i32,
            trim_second: self.word(trim)? as i32,
            trim_range: self.parameter_range(self.bytes(trim + 9, 1)?[0])?,
        })
    }
    pub fn routing_tables(
        &self,
    ) -> Result<radias_synth_domain::effect_routing::EffectRoutingTables, &'static str> {
        use radias_synth_domain::effect_routing::{EffectRoutingProfile, EffectRoutingTables};
        let profiles = |address| -> Result<[EffectRoutingProfile; 31], &'static str> {
            let raw = self.bytes(address, 31 * 16)?;
            Ok(core::array::from_fn(|i| {
                let record = &raw[i * 16..i * 16 + 16];
                EffectRoutingProfile {
                    parameter: record[0],
                    offset: record[1],
                    constant: u32::from_be_bytes(record[4..8].try_into().unwrap()),
                    first: i32::from_be_bytes(record[8..12].try_into().unwrap()),
                    second: i32::from_be_bytes(record[12..16].try_into().unwrap()),
                }
            }))
        };
        let raw = self.bytes(0x0c040ed0, 256)?;
        Ok(EffectRoutingTables {
            insert: profiles(0x0c0c5d48)?,
            master: profiles(0x0c0c5f38)?,
            resonance: core::array::from_fn(|i| u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]])),
            range: self.parameter_range(17)?,
        })
    }
    pub fn lfo_value_tables(
        &self,
    ) -> Result<radias_synth_domain::effect_lfo_values::EffectLfoValueTables, &'static str> {
        use radias_synth_domain::lfo::LfoWave;
        let raw = self.bytes(0x0c040146, 8)?;
        let mut waves = [LfoWave::Zero; 8];
        for (wave, index) in waves.iter_mut().zip(raw) {
            *wave = match self.word(0x0c03e8f8 + 4 * u32::from(*index & 7))? {
                0x0c0164f4 => LfoWave::Saw,
                0x0c01650c => LfoWave::Pulse,
                0x0c016528 => LfoWave::BipolarPulse,
                0x0c016544 => LfoWave::Triangle,
                0x0c016568 => LfoWave::SampleHold,
                0x0c016574 => LfoWave::Sine,
                0x0c0165b8 => LfoWave::Zero,
                _ => return Err("Unknown effect LFO waveform"),
            };
        }
        let phase = self.bytes(0x0c03ee64 - 18 * 4, 37 * 4)?;
        let offset = self.bytes(0x0c040118 - 18 * 2, 37 * 2)?;
        Ok(
            radias_synth_domain::effect_lfo_values::EffectLfoValueTables {
                waves,
                stereo_phase: core::array::from_fn(|i| {
                    u16::from_be_bytes([phase[4 * i], phase[4 * i + 1]])
                }),
                hold_offset: core::array::from_fn(|i| {
                    i16::from_be_bytes([offset[2 * i], offset[2 * i + 1]])
                }),
            },
        )
    }
    pub fn modulation_availability(&self) -> Result<[bool; 31], &'static str> {
        let mut result = [false; 31];
        for (i, value) in result.iter_mut().enumerate() {
            *value = match self.word(0x0c0cd1d4 + 4 * i as u32)? {
                0x0c077a9c => false,
                0x0c077aa0 => true,
                _ => return Err("Unknown effect modulation predicate"),
            };
        }
        Ok(result)
    }
    pub fn blocks_next_insert(&self, kind: EffectKind) -> Result<u8, &'static str> {
        Ok(self.header(EffectBank::Insert, kind)?[14])
    }
}
