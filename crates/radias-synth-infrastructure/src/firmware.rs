use radias_synth_domain::amplifier_control::AmplifierTables;
use radias_synth_domain::bandlimit::BandwidthTable;
use radias_synth_domain::envelope_segment::{EnvelopeCurves, EnvelopeTimingTables};
use radias_synth_domain::filter_control::FilterMixTable;
use radias_synth_domain::pitch::PitchTable;
use radias_synth_domain::voice_allocation::VoiceCostTables;
use radias_synth_domain::waveform::WaveformTable;
pub fn primary_pitch_sender_table(
    system: &[u8],
) -> Result<radias_synth_domain::primary_pitch_dispatch::PrimaryPitchSendTable, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    let mut senders = [0; 24];
    for (index, sender) in senders.iter_mut().enumerate() {
        let address = 0x41308 + 0x1000 + 4 * index;
        let target = u32::from_be_bytes(system[address..address + 4].try_into().unwrap());
        *sender = match target {
            0x0c01fd2a => 1,
            0x0c01fd40 => 2,
            0x0c01fd54 => 3,
            0x0c01fd68 => 4,
            0x0c01fd7c => 5,
            0x0c01fda4 => 7,
            _ => return Err("Unqualified primary pitch word sender"),
        };
    }
    Ok(radias_synth_domain::primary_pitch_dispatch::PrimaryPitchSendTable { senders })
}
pub fn amplifier_rate_table(
    system: &[u8],
) -> Result<radias_synth_domain::amplifier_delivery::AmplifierRateTable, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    let word = |a: usize| u16::from_be_bytes(system[a + 0x1000..a + 0x1002].try_into().unwrap());
    let long = |a: usize| u32::from_be_bytes(system[a + 0x1000..a + 0x1004].try_into().unwrap());
    let table = long(0x29fc) as usize - 0xc000000;
    Ok(
        radias_synth_domain::amplifier_delivery::AmplifierRateTable {
            attack_rates: core::array::from_fn(|i| word(table + 2 * i)),
            soft_binding_rate: word(0x29cc),
            regular_rate: (long(0x2a34) >> 16) as u16,
            termination_rate: (long(0x2ad8) >> 16) as u16,
            reset_rate: (long(0x2b10) >> 16) as u16,
        },
    )
}
pub fn primary_initialization_tables(
    system: &[u8],
) -> Result<radias_synth_domain::primary_initialization::PrimaryInitializationTables, &'static str>
{
    use radias_synth_domain::primary_initialization::{
        PrimaryInitialization, PrimaryInitializationTables, PrimaryInitializationWord,
    };
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    let mut entries = [PrimaryInitialization::default(); 24];
    for (index, entry) in entries.iter_mut().enumerate() {
        let generator = 0x1000 + 0x425d2 + 2 * index;
        entry.generator = u16::from_be_bytes(system[generator..generator + 2].try_into().unwrap());
        let pointer = 0x1000 + 0x41608 + 4 * index;
        let pointer = u32::from_be_bytes(system[pointer..pointer + 4].try_into().unwrap());
        if pointer == 0 {
            continue;
        }
        let Some(mut offset) = pointer.checked_sub(0x0c000000).map(|v| v as usize + 0x1000) else {
            return Err("OSC1 initialization pointer outside SYS");
        };
        loop {
            let Some(bytes) = system.get(offset..offset + 4) else {
                return Err("OSC1 initialization constants outside SYS");
            };
            let value = u32::from_be_bytes(bytes.try_into().unwrap());
            if value == u32::MAX {
                break;
            }
            if usize::from(entry.count) == entry.constants.len() || value >> 16 >= 160 {
                return Err("OSC1 initialization constants outside actor bank");
            }
            entry.constants[usize::from(entry.count)] = PrimaryInitializationWord {
                offset: (value >> 16) as u16,
                value: value as u16,
            };
            entry.count += 1;
            offset += 4;
        }
    }
    Ok(PrimaryInitializationTables { entries })
}
pub fn physical_phase_tables(
    system: &[u8],
) -> Result<radias_synth_domain::actor_startup::PhysicalPhaseTables, &'static str> {
    use radias_synth_domain::actor_startup::{
        PhysicalPhaseInitialization, PhysicalPhaseTables, PhysicalPhaseWord,
    };
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    let mut entries = [PhysicalPhaseInitialization::default(); 24];
    for (index, entry) in entries.iter_mut().enumerate() {
        let p = 0x1000 + 0x41808 + index * 4;
        let pointer = u32::from_be_bytes(system[p..p + 4].try_into().unwrap());
        if pointer == 0 {
            continue;
        }
        let mut p = pointer
            .checked_sub(0x0c000000)
            .ok_or("Physical phase pointer outside SYS")? as usize
            + 0x1000;
        loop {
            let bytes = system
                .get(p..p + 4)
                .ok_or("Physical phase table outside SYS")?;
            let offset = u32::from_be_bytes(bytes.try_into().unwrap());
            if offset == u32::MAX {
                break;
            }
            if entry.count == 2 || offset as u16 >= 63 {
                return Err("Physical phase table outside frame");
            }
            let bytes = system
                .get(p + 4..p + 8)
                .ok_or("Physical phase value outside SYS")?;
            entry.words[usize::from(entry.count)] = PhysicalPhaseWord {
                offset: offset as u16,
                value: u32::from_be_bytes(bytes.try_into().unwrap()),
            };
            entry.count += 1;
            p += 8;
        }
    }
    Ok(PhysicalPhaseTables { entries })
}
pub fn phase_callback_tables(
    system: &[u8],
) -> Result<radias_synth_domain::actor_startup::PhaseCallbackTables, &'static str> {
    use radias_synth_domain::actor_startup::{PhaseCallback, PhaseCallbackTables};
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    let mut phases = [PhaseCallback::None; 24];
    let mut counters = [false; 24];
    for index in 0..24 {
        let p = 0x1000 + 0x41708 + index * 4;
        phases[index] = match u32::from_be_bytes(system[p..p + 4].try_into().unwrap()) {
            0 => PhaseCallback::None,
            0x0c01e644 => PhaseCallback::CachedWord,
            0x0c01e664 => PhaseCallback::Unison,
            0x0c01e674 => PhaseCallback::UnisonTriangle,
            _ => return Err("Unsupported non-PCM phase callback"),
        };
        let p = 0x1000 + 0x41208 + index * 4;
        counters[index] = match u32::from_be_bytes(system[p..p + 4].try_into().unwrap()) {
            0 => false,
            0x0c01e608 => true,
            _ => return Err("Unsupported non-PCM counter callback"),
        };
    }
    Ok(PhaseCallbackTables { phases, counters })
}
pub fn parameter_template_tables(
    system: &[u8],
    mix: radias_synth_domain::filter_control::FilterMixTable,
) -> Result<radias_synth_domain::parameter_template::ParameterTemplateTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    let word = |offset: usize| {
        u16::from_be_bytes(system[offset + 0x1000..offset + 0x1002].try_into().unwrap())
    };
    Ok(
        radias_synth_domain::parameter_template::ParameterTemplateTables {
            primary: primary_initialization_tables(system)?,
            secondary: core::array::from_fn(|i| word(0x42652 + 2 * i)),
            routing: core::array::from_fn(|i| word(0x411a0 + 2 * i)),
            filter2: core::array::from_fn(|i| word(0x411a8 + 2 * i)),
            pre_shaper: core::array::from_fn(|kind| {
                core::array::from_fn(|position| word(0x3cd98 + 8 * kind + 2 * position))
            }),
            post_shaper: core::array::from_fn(|kind| {
                core::array::from_fn(|position| word(0x3cd08 + 8 * kind + 2 * position))
            }),
            high_output_gain: core::array::from_fn(|i| word(0x427ea + 2 * i)),
            mix,
        },
    )
}
pub fn parameter_template_addresses(
    system: &[u8],
) -> Result<radias_synth_domain::actor_copy::ParameterTemplateAddresses, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    Ok(
        radias_synth_domain::actor_copy::ParameterTemplateAddresses {
            drums: core::array::from_fn(|index| {
                let offset = 0x1000 + 0x3cf7a + 2 * index;
                u16::from_be_bytes(system[offset..offset + 2].try_into().unwrap())
            }),
        },
    )
}
pub fn controller_service_timer(
    system: &[u8],
) -> Result<radias_synth_domain::controller_service::ControllerServiceTimer, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    let constant = u32::from_be_bytes(system[0x2f4a4..0x2f4a8].try_into().unwrap());
    Ok(
        radias_synth_domain::controller_service::ControllerServiceTimer {
            constant,
            counter: constant,
            prescaler_phase: 0,
        },
    )
}
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

pub fn construction_group_tables(
    system: &[u8],
) -> Result<radias_synth_domain::actor_construction::ConstructionGroupTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    let bank = |base: usize| {
        core::array::from_fn(|i| {
            let p = base + 0x1000 + 4 * i;
            let pointer = u32::from_be_bytes(system[p..p + 4].try_into().unwrap());
            (pointer != 0).then(|| {
                core::array::from_fn(|n| {
                    let p = (i64::from(pointer) - 0x0c000000
                        + 0x1000
                        + 2 * i64::from(n as u8 as i8)) as usize;
                    i16::from_be_bytes(system[p..p + 2].try_into().unwrap())
                })
            })
        })
    };
    Ok(
        radias_synth_domain::actor_construction::ConstructionGroupTables {
            detune: bank(0x3cf20),
            pan: bank(0x3cf44),
            drum_output_routes: core::array::from_fn(|i| {
                let p = 0x42320 + 0x1000 + 4 * i;
                u32::from_be_bytes(system[p..p + 4].try_into().unwrap())
            }),
        },
    )
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
pub fn raw_note_scale_tables(
    system: &[u8],
) -> Result<radias_synth_domain::raw_note_scale::RawNoteScaleTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS2.00 controller image");
    }
    fn bytes<const N: usize>(system: &[u8], address: usize) -> [i8; N] {
        core::array::from_fn(|i| system[address + 0x1000 + i] as i8)
    }
    Ok(radias_synth_domain::raw_note_scale::RawNoteScaleTables {
        pitch_classes: bytes(system, 0x42f42 - 192),
        cents: core::array::from_fn(|i| bytes(system, 0x42eca + 12 * i - 128)),
        scaled_note: [bytes(system, 0x42f12 - 128), bytes(system, 0x42f2a - 128)],
        scaled_root: [bytes(system, 0x42f1e), bytes(system, 0x42f36)],
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
pub fn live_filter_resonance_tables(
    system: &[u8],
) -> Result<radias_synth_domain::virtual_patch_live::LiveFilterResonanceTables, &'static str> {
    if system.len() != 0xe0000 {
        return Err("Expected original SYS 2.00 controller image");
    }
    Ok(
        radias_synth_domain::virtual_patch_live::LiveFilterResonanceTables {
            gain: core::array::from_fn(|i| {
                let p = 0x40340 + 0x1000 + 4 * i;
                i32::from_be_bytes(system[p..p + 4].try_into().unwrap())
            }),
            normalization: core::array::from_fn(|bank| {
                core::array::from_fn(|i| {
                    let p = if bank == 0 { 0x40ed0 } else { 0x40fd0 } + 0x1000 + 2 * i;
                    u16::from_be_bytes(system[p..p + 2].try_into().unwrap())
                })
            }),
        },
    )
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
    pub fn pitch_receiver_rom(
        &self,
    ) -> Result<radias_synth_domain::pitch_receiver::PitchReceiverRom, &'static str> {
        let mut words = [0; 0x900];
        for (i, word) in words.iter_mut().enumerate() {
            *word = self.word(0x4000 + i)?;
        }
        Ok(radias_synth_domain::pitch_receiver::PitchReceiverRom { words })
    }
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
