use radias_synth_domain::amplifier_control::AmplifierTables;
use radias_synth_domain::bandlimit::BandwidthTable;
use radias_synth_domain::envelope_segment::{EnvelopeCurves, EnvelopeTimingTables};
use radias_synth_domain::filter_control::FilterMixTable;
use radias_synth_domain::pitch::PitchTable;
use radias_synth_domain::voice_allocation::VoiceCostTables;
use radias_synth_domain::waveform::WaveformTable;
pub fn voice_group_tables(
    system: &[u8],
) -> Result<radias_synth_domain::voice_group::VoiceGroupTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    let bank = |base: usize| {
        core::array::from_fn(|i| {
            let p = base + 0x1000 + 4 * i;
            let pointer = u32::from_be_bytes(system[p..p + 4].try_into().unwrap());
            core::array::from_fn(|n| {
                if pointer == 0 {
                    0
                } else {
                    let p = pointer as usize - 0x0c000000 + 0x1000 + 2 * n;
                    i16::from_be_bytes(system[p..p + 2].try_into().unwrap())
                }
            })
        })
    };
    Ok(radias_synth_domain::voice_group::VoiceGroupTables {
        detune: bank(0x3cf20),
        pan: bank(0x3cf44),
    })
}
pub fn portamento_rates(
    system: &[u8],
) -> Result<radias_synth_domain::portamento::PortamentoRates, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    Ok(radias_synth_domain::portamento::PortamentoRates {
        values: core::array::from_fn(|i| {
            let p = 0x41e88 + 0x1000 + 4 * i;
            u32::from_be_bytes(system[p..p + 4].try_into().unwrap())
        }),
    })
}
pub fn portamento_curves(
    system: &[u8],
) -> Result<radias_synth_domain::portamento::PortamentoCurves, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let word = |p: usize| u16::from_be_bytes(system[p + 0x1000..p + 0x1002].try_into().unwrap());
    Ok(radias_synth_domain::portamento::PortamentoCurves {
        values: core::array::from_fn(|curve| {
            let p = 0x3e8b8 + 4 * curve;
            let pointer =
                u32::from_be_bytes(system[p + 0x1000..p + 0x1004].try_into().unwrap()) as usize;
            core::array::from_fn(|i| word(pointer - 0x0c000000 + 2 * i))
        }),
    })
}
pub fn note_pitch_tables(
    system: &[u8],
) -> Result<radias_synth_domain::note_pitch::NotePitchTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let bytes = |address: usize| core::array::from_fn(|i| system[address + 0x1000 + i] as i8);
    Ok(radias_synth_domain::note_pitch::NotePitchTables {
        fine_tune: core::array::from_fn(|i| {
            let p = 0x41c88 + 0x1000 + 4 * i;
            i32::from_be_bytes(system[p..p + 4].try_into().unwrap())
        }),
        cents: core::array::from_fn(|i| bytes(0x42eca + 12 * i)),
        scaled_root: [bytes(0x42f1e), bytes(0x42f36)],
        scaled_note: [bytes(0x42f12), bytes(0x42f2a)],
        vibrato: fine_tune_table(system)?,
    })
}
pub fn formant_counter_seeds(
    system: &[u8],
) -> Result<radias_synth_domain::controller_noise::FormantCounterSeeds, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    Ok(radias_synth_domain::controller_noise::FormantCounterSeeds {
        values: core::array::from_fn(|index| {
            let p = 0x42a7a + 0x1000 + index * 2;
            i16::from_be_bytes(system[p..p + 2].try_into().unwrap())
        }),
    })
}
pub fn fine_tune_table(
    system: &[u8],
) -> Result<radias_synth_domain::controller_secondary::FineTuneTable, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    Ok(radias_synth_domain::controller_secondary::FineTuneTable {
        values: core::array::from_fn(|i| {
            let p = 0x426ea + 0x1000 + 2 * i;
            i16::from_be_bytes(system[p..p + 2].try_into().unwrap())
        }),
    })
}
pub fn mixer_scales(
    system: &[u8],
) -> Result<radias_synth_domain::controller_mixer::MixerScales, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let word = |address: usize| {
        u16::from_be_bytes(
            system[address + 0x1000..address + 0x1002]
                .try_into()
                .unwrap(),
        )
    };
    Ok(radias_synth_domain::controller_mixer::MixerScales {
        primary: core::array::from_fn(|i| word(0x4265a + 2 * i)),
        secondary: core::array::from_fn(|bank| {
            core::array::from_fn(|i| word(0x426da + 8 * bank + 2 * i))
        }),
    })
}
pub fn pan_tables(
    system: &[u8],
) -> Result<radias_synth_domain::controller_pan::PanTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    Ok(radias_synth_domain::controller_pan::PanTables {
        targets: core::array::from_fn(|i| {
            let p = 0x3c808 + 0x1000 + 2 * i;
            u16::from_be_bytes(system[p..p + 2].try_into().unwrap())
        }),
    })
}

pub fn controller_filter_tables(
    system: &[u8],
) -> Result<radias_synth_domain::controller_filter::ControllerFilterTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    Ok(
        radias_synth_domain::controller_filter::ControllerFilterTables {
            frequency: core::array::from_fn(|i| {
                let p = 0x40540 + 0x1000 + 4 * i;
                u32::from_be_bytes(system[p..p + 4].try_into().unwrap())
            }),
            key_depth: core::array::from_fn(|i| {
                let p = 0x3c908 + 0x1000 + 2 * i;
                i16::from_be_bytes(system[p..p + 2].try_into().unwrap())
            }),
        },
    )
}

pub fn filter2_control_tables(
    system: &[u8],
) -> Result<radias_synth_domain::controller_filter2::Filter2ControlTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let long = |address: usize| {
        i32::from_be_bytes(
            system[address + 0x1000..address + 0x1004]
                .try_into()
                .unwrap(),
        )
    };
    let word = |address: usize| {
        i16::from_be_bytes(
            system[address + 0x1000..address + 0x1002]
                .try_into()
                .unwrap(),
        )
    };
    Ok(
        radias_synth_domain::controller_filter2::Filter2ControlTables {
            resonance: core::array::from_fn(|i| long(0x40340 + 4 * i)),
            input_gain: core::array::from_fn(|i| word(0x40ed0 + 2 * i)),
            linked_serial_input_gain: core::array::from_fn(|i| word(0x40fd0 + 2 * i)),
        },
    )
}

pub fn comb_control_tables(
    system: &[u8],
) -> Result<radias_synth_domain::controller_comb::CombControlTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    let row = |address: usize, i: usize| {
        let offset = address - 0x0c000000 + 0x1000 + 4 * i;
        u32::from_be_bytes(system[offset..offset + 4].try_into().unwrap())
    };
    Ok(radias_synth_domain::controller_comb::CombControlTables {
        delays: core::array::from_fn(|i| row(0x0c0407d0, i)),
        feedback: core::array::from_fn(|i| row(0x0c0409d0, i)),
    })
}

pub fn lfo_tables(system: &[u8]) -> Result<radias_synth_domain::lfo::LfoTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let word = |address: usize| {
        let offset = address - 0x0c000000 + 0x1000;
        u16::from_be_bytes([system[offset], system[offset + 1]])
    };
    let long = |address: usize| {
        let offset = address - 0x0c000000 + 0x1000;
        u32::from_be_bytes(system[offset..offset + 4].try_into().unwrap())
    };
    Ok(radias_synth_domain::lfo::LfoTables {
        warp: core::array::from_fn(|i| word(0x0c03fe32 + i * 2)),
        sine: core::array::from_fn(|i| word(0x0c03f8ae + i * 2) as i16),
        frequency: core::array::from_fn(|i| long(0x0c03e918 + i * 4)),
        initial_phase: core::array::from_fn(|i| word(0x0c03feb4 + i * 2)),
        frequency_scale: core::array::from_fn(|i| long(0x0c03ec18 + i * 4)),
    })
}

pub fn lfo_tempo_tables(
    system: &[u8],
) -> Result<radias_synth_domain::lfo_tempo::LfoTempoTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let word = |address: usize| {
        let offset = address - 0x0c000000 + 0x1000;
        u16::from_be_bytes([system[offset], system[offset + 1]])
    };
    let long = |address: usize| {
        let offset = address - 0x0c000000 + 0x1000;
        u32::from_be_bytes(system[offset..offset + 4].try_into().unwrap())
    };
    Ok(radias_synth_domain::lfo_tempo::LfoTempoTables {
        clock_steps: core::array::from_fn(|i| word(0x0c03fdb2 + 2 * i)),
        increments: core::array::from_fn(|i| long(0x0c03eb98 + 4 * i)),
        tempo_coefficients: core::array::from_fn(|i| long(0x0c03eb18 + 4 * i)),
        minimum_increment: long(0x0c03e918),
        maximum_increment: long(0x0c03eb14),
    })
}

pub fn modulation_tables(
    system: &[u8],
) -> Result<radias_synth_domain::modulation::ModulationTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let table = |address: usize| {
        core::array::from_fn(|n| {
            let offset = address - 0x0c000000 + 0x1000 + n * 2;
            i16::from_be_bytes([system[offset], system[offset + 1]])
        })
    };
    Ok(radias_synth_domain::modulation::ModulationTables {
        pitch_depth: table(0x0c0426ea),
        lfo_rate_depth: table(0x0c042aca),
        key_linear_depth: table(0x0c042cca),
        key_cutoff_depth: table(0x0c042dca),
        key_lfo_rate_depth: table(0x0c042bca),
    })
}

pub fn voice_cost_tables(system: &[u8]) -> Result<VoiceCostTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let word = |address: usize| {
        let p = address - 0x0c000000 + 0x1000;
        u16::from_be_bytes([system[p], system[p + 1]])
    };
    Ok(VoiceCostTables {
        base: word(0x0c0410d0),
        primary: core::array::from_fn(|n| word(0x0c0410d6 + 2 * n)),
        secondary: core::array::from_fn(|n| word(0x0c041156 + 2 * n)),
        filters: core::array::from_fn(|n| word(0x0c04115e + 2 * n)),
        shaper: core::array::from_fn(|n| word(0x0c04117e + 2 * n)),
    })
}

/// Read-only tables from the preserved SYS 2.00 controller image.
pub fn envelope_curves(system: &[u8]) -> Result<EnvelopeCurves, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let mut curves = EnvelopeCurves {
        values: [[0; 256]; 8],
    };
    for (index, row) in curves.values.iter_mut().enumerate() {
        let pointer = 0x03e8b8 + 0x1000 + index * 4;
        let address = u32::from_be_bytes(system[pointer..pointer + 4].try_into().unwrap());
        let offset = address
            .checked_sub(0x0c000000)
            .ok_or("Curve outside controller RAM")? as usize
            + 0x1000;
        let bytes = system
            .get(offset..offset + 512)
            .ok_or("Curve outside controller image")?;
        for (value, pair) in row.iter_mut().zip(bytes.chunks_exact(2)) {
            *value = u16::from_be_bytes([pair[0], pair[1]]);
        }
    }
    Ok(curves)
}

pub fn envelope_timing_tables(system: &[u8]) -> Result<EnvelopeTimingTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let read = |address: usize, length: usize| -> Result<&[u8], &'static str> {
        let offset = address
            .checked_sub(0x0c000000)
            .ok_or("Timing table outside controller RAM")?
            + 0x1000;
        system
            .get(offset..offset + length)
            .ok_or("Timing table outside controller image")
    };
    let mut tables = EnvelopeTimingTables {
        increments: [[0; 128]; 8],
        scale: [0; 128],
        key_tracking: [0; 128],
    };
    for (i, row) in tables.increments.iter_mut().enumerate() {
        let pointer = u32::from_be_bytes(read(0x0c03e8d8 + 4 * i, 4)?.try_into().unwrap());
        for (value, raw) in row
            .iter_mut()
            .zip(read(pointer as usize, 512)?.chunks_exact(4))
        {
            *value = u32::from_be_bytes(raw.try_into().unwrap());
        }
    }
    for (value, raw) in tables
        .scale
        .iter_mut()
        .zip(read(0x0c03fef4, 256)?.chunks_exact(2))
    {
        *value = u16::from_be_bytes([raw[0], raw[1]]);
    }
    for (value, raw) in tables
        .key_tracking
        .iter_mut()
        .zip(read(0x0c042cca, 256)?.chunks_exact(2))
    {
        *value = i16::from_be_bytes([raw[0], raw[1]]);
    }
    Ok(tables)
}

pub fn amplifier_tables(system: &[u8]) -> Result<AmplifierTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    let mut tables = AmplifierTables {
        velocity: [0; 128],
        midi_volume: [0; 128],
        program_volume: [0; 128],
        key_tracking: [0; 128],
    };
    for i in 0..128 {
        let word = |address: usize| {
            let offset = address - 0x0c000000 + 0x1000;
            u16::from_be_bytes([system[offset], system[offset + 1]])
        };
        tables.velocity[i] = word(0x0c03fff4 + 2 * i) as i16;
        tables.midi_volume[i] = word(0x0c03cc08 + 2 * i);
        tables.program_volume[i] = word(0x0c03cf68 + 2 * i);
        tables.key_tracking[i] = word(0x0c03cb08 + 2 * i) as i16;
    }
    Ok(tables)
}

/// A read-only normal Master host stream. Executable bytes are never run here.
pub struct MasterTables<'a> {
    bytes: &'a [u8],
    origin: usize,
}

impl<'a> MasterTables<'a> {
    pub fn noise_pitch(
        &self,
    ) -> Result<radias_synth_domain::noise_control::NoisePitchTable, &'static str> {
        let mut table = radias_synth_domain::noise_control::NoisePitchTable {
            curve_scales: [0; 128],
        };
        for (index, value) in table.curve_scales.iter_mut().enumerate() {
            *value = self.word(0x484b + index)? as i16;
        }
        Ok(table)
    }
    pub fn filter_mix(&self) -> Result<FilterMixTable, &'static str> {
        let mut table = FilterMixTable {
            weights: [[0; 128]; 5],
        };
        for (row, values) in table.weights.iter_mut().enumerate() {
            for (i, value) in values.iter_mut().enumerate() {
                *value = self.word(0x44ca + 128 * row + i)? as i16;
            }
        }
        Ok(table)
    }
    pub fn bandwidth(&self) -> Result<BandwidthTable, &'static str> {
        let mut table = BandwidthTable { gains: [0; 129] };
        for (i, value) in table.gains.iter_mut().enumerate() {
            *value = self.word(0x43b3 + i)? as i16;
        }
        Ok(table)
    }
    pub fn from_host_stream(bytes: &'a [u8]) -> Result<Self, &'static str> {
        if bytes.len() != 65_290 {
            return Err("Expected normal SYS 2.00 Master host stream");
        }
        let origin = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
        let image = Self { bytes, origin };
        image.word(0x4032)?;
        image.word(0x43b1)?;
        Ok(image)
    }

    pub fn word(&self, address: usize) -> Result<u16, &'static str> {
        let word = address
            .checked_sub(self.origin)
            .ok_or("Table precedes loaded image")?;
        let offset = 2 + word * 2;
        if offset + 2 > self.bytes.len() - 8 {
            return Err("Table lies outside loaded image");
        }
        Ok(u16::from_be_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
        ]))
    }

    pub fn pitch(&self) -> Result<PitchTable, &'static str> {
        let mut table = PitchTable {
            notes: [0; 128],
            fractions: [0; 128],
        };
        for i in 0..128 {
            table.notes[i] =
                ((self.word(0x4032 + i * 2)? as u32) << 16) | self.word(0x4033 + i * 2)? as u32;
            table.fractions[i] = self.word(0x4332 + i)? as i16;
        }
        Ok(table)
    }

    pub fn waveform(&self) -> Result<WaveformTable, &'static str> {
        let mut table = WaveformTable {
            correction: [0; 129],
            shapers: radias_synth_domain::waveshaper::ShaperTables {
                sub_edges: [0; 129],
            },
        };
        for (i, value) in table.correction.iter_mut().enumerate() {
            *value = self.word(0x4474 - 64 + i)? as i16;
        }
        for (i, value) in table.shapers.sub_edges.iter_mut().enumerate() {
            *value = self.word(0x48cb + i)? as i16;
        }
        Ok(table)
    }
}
