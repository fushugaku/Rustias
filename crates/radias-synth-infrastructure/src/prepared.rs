//! Adapt original parameter observations into a self-contained voice program.
//! This adapter supports qualified VA graph fixtures; it does not
//! claim complete RDL patch compilation or consume recorded audio at runtime.
use radias_synth_application::VoiceControlEvent;
use radias_synth_domain::{
    Phase,
    envelope::EnvelopeLevel,
    filter::{FilterCoefficients, FilterState, ResonantFilter},
    fixed::{high_product, saturate},
    mixer::OscillatorMix,
    oscillator::Oscillator,
    pitch::PhaseIncrement,
    primary_oscillator::{
        PrimaryCrossParameters, PrimaryCrossSineParameters, PrimaryCrossTriangleParameters,
        PrimaryOscillator, PrimaryParameters, PrimaryRampParameters, PrimarySineParameters,
        PrimaryTriangleParameters, PrimaryUnisonCarrierParameters, PrimaryVpmParameters,
    },
    voice::{Voice, VoiceParameters},
    waveform::{ShapeParameters, Transfer},
};

pub struct PreparedVoice {
    pub initial: Voice,
    pub parameters: VoiceParameters,
    pub events: Vec<VoiceControlEvent>,
    pub control_slew: radias_synth_domain::control_slew::SlewWeights,
    pub reference_start_frame: u64,
    pub reference_voice_frames: usize,
    pub bus: radias_synth_domain::pan::VoiceBus,
}

pub struct ControlMap {
    pub frequencies: [i32; 128],
    pub resonances: [i32; 128],
    pub input_gains: [i16; 128],
    pub normalization: i32,
}

impl ControlMap {
    pub fn from_json(raw: &[u8]) -> Result<Self, String> {
        let parsed: serde_json::Value = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
        let get = |kind: &str, index: usize, key: &str| -> Result<u32, String> {
            let row = &parsed[kind][index];
            if row["value"].as_u64() != Some(index as u64) {
                return Err("Original control map index differs".into());
            }
            let value = row[key]
                .as_u64()
                .ok_or("Original control map field absent")?;
            u32::try_from(value).map_err(|e| e.to_string())
        };
        let mut table = Self {
            frequencies: [0; 128],
            resonances: [0; 128],
            input_gains: [0; 128],
            normalization: get("cutoff", 0, "normalization")? as i32,
        };
        for i in 0..128 {
            table.frequencies[i] = get("cutoff", i, "frequency")? as i32;
            table.resonances[i] = get("resonance", i, "resonance")? as i32;
            let gain = get("resonance", i, "input_gain")?;
            if gain > 32767 {
                return Err("Original input gain out of range".into());
            }
            table.input_gains[i] = gain as i16;
            if get("cutoff", i, "normalization")? as i32 != table.normalization
                || get("resonance", i, "normalization")? as i32 != table.normalization
            {
                return Err("Context-dependent filter normalization needs another map".into());
            }
        }
        Ok(table)
    }
    pub fn filter(
        &self,
        mut base: FilterCoefficients,
        cutoff: u8,
        resonance: u8,
    ) -> Result<FilterCoefficients, &'static str> {
        if cutoff > 127 || resonance > 127 {
            return Err("Filter control out of range");
        }
        let compiled = radias_synth_domain::filter_control::compile(
            self.frequencies[cutoff as usize],
            self.resonances[resonance as usize],
            self.normalization,
        );
        base.feedback = compiled.feedback;
        base.integrator_gain = compiled.integrator_gain;
        base.post_gain = compiled.post_gain;
        base.post_feedback = compiled.post_feedback;
        base.input_gain = self.input_gains[resonance as usize];
        Ok(base)
    }
}

fn word(raw: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(raw[index * 4..index * 4 + 4].try_into().unwrap())
}
fn parameter(raw: &[u8], index: usize) -> i16 {
    word(raw, 1 + index) as i16
}
fn pair(raw: &[u8], index: usize) -> i32 {
    ((word(raw, 1 + index) << 16) | word(raw, 2 + index)) as i32
}

pub fn parameters(raw: &[u8]) -> Result<VoiceParameters, &'static str> {
    if raw.len() < 696 {
        return Err("Voice parameter record truncated");
    }
    let p = |i| parameter(raw, i);
    let q = |i| pair(raw, i);
    if ![
        0xbe88, 0xbf1c, 0xbfc4, 0xc018, 0xc0a8, 0xc1a8, 0xc430, 0xc4c4, 0xc51c, 0xc584, 0xc66c,
        0xc790, 0xc868, 0xc97c, 0xca50, 0xcae8,
    ]
    .contains(&(p(1) as u16))
        || ![0xb468, 0xb5a0, 0xb884, 0xbb80].contains(&(p(46) as u16))
    {
        return Err("Unported voice generator or filter routing");
    }
    let dual = p(46) as u16 != 0xb468;
    let shaper = {
        use radias_synth_domain::waveshaper::{
            DriveCoefficients, ShaperCoefficients, ShaperParameters, ShaperPosition,
        };
        let a = p(82) as u16;
        let b = p(83) as u16;
        if a != 0xd058 && b != 0xd058 {
            return Err("Unqualified simultaneous shaper placement");
        }
        let (kind, position) = if a == 0xd058 {
            (b, ShaperPosition::PreAmp)
        } else {
            (a, ShaperPosition::PreFilter)
        };
        let coefficients = match kind {
            0xd058 => None,
            0xcb90 => Some(ShaperCoefficients::Drive(DriveCoefficients {
                depth: p(85),
                normalization: p(86),
                feedback_gain: p(88),
                threshold: p(89),
                curves: [p(90), p(91)],
            })),
            0xccb4 => Some(ShaperCoefficients::HardClip { depth: p(85) }),
            0xcbe4 => Some(ShaperCoefficients::Decimator { depth: p(85) }),
            0xcc1c => Some(ShaperCoefficients::MultiTriangle { depth: p(85) }),
            0xcc64 => Some(ShaperCoefficients::MultiSine { depth: p(85) }),
            0xccd4 => Some(ShaperCoefficients::OctSaw { depth: p(85) }),
            0xd010 => Some(ShaperCoefficients::LevelBoost { depth: p(85) }),
            0xcd98 => Some(ShaperCoefficients::Pickup {
                depth: p(85),
                pitch_current: p(88),
            }),
            0xced0 | 0xce64 | 0xcf48 | 0xcfb0 => {
                use radias_synth_domain::waveshaper::{
                    SubOscillatorCoefficients, SubOscillatorWaveform,
                };
                Some(ShaperCoefficients::SubOscillator(
                    SubOscillatorCoefficients {
                        waveform: match kind {
                            0xced0 => SubOscillatorWaveform::Saw,
                            0xce64 => SubOscillatorWaveform::Square,
                            0xcf48 => SubOscillatorWaveform::Triangle,
                            _ => SubOscillatorWaveform::Sine,
                        },
                        depth: p(85),
                        target_depth: p(84),
                        gain_current: p(88),
                    },
                ))
            }
            _ => return Err("Unported waveshaper"),
        };
        coefficients.map(|coefficients| ShaperParameters {
            position,
            coefficients,
        })
    };
    if radias_synth_domain::pan::VoiceBus::from_offsets(word(raw, 172), word(raw, 173)).is_none() {
        return Err("Unqualified voice bus routing");
    }
    let ramp = PrimaryRampParameters {
        increment: PhaseIncrement(q(4) as u32),
        shape: p(10),
        blend: p(12),
        offset_target: p(6),
        target_gain: p(13),
        memory_gain: p(14),
    };
    let primary = match p(1) as u16 {
        0xbe88 => PrimaryParameters::Ramp(ramp),
        0xbf1c => PrimaryParameters::Pulse(ramp),
        0xbfc4 => PrimaryParameters::Triangle(PrimaryTriangleParameters {
            increment: ramp.increment,
            center: p(10),
            gain: p(11),
            edge_gain: p(7),
            upper: p(12),
            upper_reflection: p(13),
            lower: p(14),
            lower_reflection: p(15),
        }),
        0xc018 => PrimaryParameters::Sine(PrimarySineParameters {
            increment: ramp.increment,
            modulation_gain: p(7),
            control: [p(10), p(11)],
            center: p(12),
            polynomial: [
                ((p(15) as u16 as u32) << 16 | p(14) as u16 as u32) as i32,
                q(16),
                q(18),
            ],
        }),
        0xc0a8 => PrimaryParameters::Noise(
            radias_synth_domain::primary_oscillator::PrimaryNoiseParameters {
                increment: ramp.increment,
                generator: radias_synth_domain::noise::ColoredNoiseParameters {
                    phase_gain: p(7),
                    seed_bias: p(9),
                    curve_scale: p(10),
                    phase_offset: p(11),
                    curve_limit: q(12),
                    feedback: p(18),
                    limits: [q(20), q(22)],
                },
            },
        ),
        0xc1a8 => PrimaryParameters::Formant(
            radias_synth_domain::primary_oscillator::PrimaryFormantParameters {
                increment: ramp.increment,
                generator: radias_synth_domain::noise::FormantParameters {
                    seed_gain: p(7),
                    seed_bias: p(9),
                    frequency: p(11),
                    input_gain: p(16),
                    feedback: p(17),
                    limits: [q(18), q(20)],
                },
            },
        ),
        0xc430 => PrimaryParameters::Cross(PrimaryCrossParameters {
            increment: ramp.increment,
            modulation_gain: p(7),
            shape: p(10),
            offset: p(11),
            blend: p(12),
        }),
        0xc4c4 => PrimaryParameters::CrossTriangle(PrimaryCrossTriangleParameters {
            carrier: PrimaryTriangleParameters {
                increment: ramp.increment,
                center: p(10),
                gain: p(11),
                edge_gain: 0,
                upper: p(12),
                upper_reflection: p(13),
                lower: p(14),
                lower_reflection: p(15),
            },
            modulation_gain: p(7),
        }),
        0xc51c => PrimaryParameters::CrossSine(PrimaryCrossSineParameters {
            increment: ramp.increment,
            modulation_gain: p(7),
            center: p(12),
            polynomial: [
                ((p(15) as u16 as u32) << 16 | p(14) as u16 as u32) as i32,
                q(16),
                q(18),
            ],
        }),
        0xc584 => PrimaryParameters::Unison(radias_synth_domain::unison::UnisonParameters {
            increments: [4, 10, 12, 16, 18].map(|i| PhaseIncrement(q(i) as u32)),
            detune: p(6) as u16,
            correction_gain: p(20),
            level: p(21),
        }),
        0xc66c | 0xc790 | 0xc868 => {
            PrimaryParameters::UnisonCarrier(PrimaryUnisonCarrierParameters {
                parameters: radias_synth_domain::unison::UnisonParameters {
                    increments: [4, 10, 12, 16, 18].map(|i| PhaseIncrement(q(i) as u32)),
                    detune: p(6) as u16,
                    correction_gain: p(20),
                    level: p(21),
                },
                waveform: match p(1) as u16 {
                    0xc66c => {
                        radias_synth_domain::unison::UnisonWaveform::Pulse { bandwidth: p(34) }
                    }
                    0xc790 => radias_synth_domain::unison::UnisonWaveform::Triangle,
                    _ => radias_synth_domain::unison::UnisonWaveform::Sine {
                        normalization: i32::MAX,
                    },
                },
            })
        }
        0xc97c => PrimaryParameters::Vpm(PrimaryVpmParameters {
            increment: ramp.increment,
            modulation_gain: p(7),
            ratio: p(8),
            limit: p(13),
            center: q(14),
            shape: p(10),
            offset: p(11),
            blend: p(12),
        }),
        0xca50 | 0xcae8 => {
            use radias_synth_domain::primary_oscillator::{
                PrimaryVpmCarrier, PrimaryVpmCarrierParameters, PrimaryVpmModulatorParameters,
            };
            PrimaryParameters::VpmCarrier(PrimaryVpmCarrierParameters {
                modulator: PrimaryVpmModulatorParameters {
                    increment: ramp.increment,
                    modulation_gain: p(7),
                    ratio: p(8),
                    limit: p(10),
                    center: q(12),
                },
                carrier: if p(1) as u16 == 0xca50 {
                    PrimaryVpmCarrier::Triangle(PrimaryTriangleParameters {
                        increment: ramp.increment,
                        center: p(14),
                        gain: p(15),
                        edge_gain: 0,
                        upper: p(16),
                        upper_reflection: p(17),
                        lower: p(18),
                        lower_reflection: p(19),
                    })
                } else {
                    PrimaryVpmCarrier::Sine {
                        center: p(14),
                        polynomial: [
                            ((p(17) as u16 as u32) << 16 | p(16) as u16 as u32) as i32,
                            q(18),
                            q(20),
                        ],
                    }
                },
            })
        }
        _ => return Err("Unqualified primary waveform"),
    };
    Ok(VoiceParameters {
        primary,
        primary_pitch_code: p(2) as u16,
        shaper,
        mix: OscillatorMix {
            primary_gain: p(48),
            secondary_gain: p(50),
            noise_gain: p(52),
        },
        filter: FilterCoefficients {
            input_gain: p(55),
            feedback: q(58),
            integrator_gain: q(66),
            post_gain: p(69),
            post_feedback: p(71),
            mix: [p(73), p(75), p(77), p(79), p(81)],
        },
        routing: if dual {
            use radias_synth_domain::filter_routing::{
                DualFilterParameters, Filter2Coefficients, Filter2Output, FilterRouting,
            };
            Some(DualFilterParameters {
                route: match p(46) as u16 {
                    0xb5a0 => FilterRouting::Serial,
                    0xb884 => FilterRouting::Parallel,
                    _ => FilterRouting::Individual,
                },
                first: FilterCoefficients {
                    input_gain: p(55),
                    feedback: q(58),
                    integrator_gain: q(66),
                    post_gain: p(69),
                    post_feedback: p(71),
                    mix: [p(73), p(75), p(77), p(79), p(81)],
                },
                second: Filter2Coefficients {
                    input_gain: p(95),
                    feedback: q(98),
                    integrator_gain: q(106),
                    output: if p(93) != 0 {
                        Filter2Output::Comb
                    } else {
                        match p(112) as u16 {
                            0x4022 => Filter2Output::LowPass,
                            0x401e => Filter2Output::HighPass,
                            0x4020 => Filter2Output::BandPass,
                            _ => return Err("Unported Filter2 selector"),
                        }
                    },
                },
            })
        } else {
            None
        },
        envelope_target: p(125),
        envelope_rate: p(124),
        secondary_modulation: radias_synth_domain::secondary_control::SecondaryModulation {
            ring: p(42) != 0,
            sync: p(43) != 0,
        },
        pan_position: saturate(
            high_product(p(127), word(raw, 163) as i16)
                + high_product(p(128), word(raw, 164) as i16),
        ),
    })
}

impl PreparedVoice {
    /// A declared first per-voice frame snapshot seeds the mixer generator;
    /// all subsequent random words are generated by the native domain.
    pub fn from_noise_reference_va_parameters(
        raw: &[u8],
        noise: &[u8],
    ) -> Result<Self, &'static str> {
        if !raw.len().is_multiple_of(704) || noise.len() != raw.len() / 704 * 20 || noise.is_empty()
        {
            return Err("Noise frame snapshot shape differs");
        }
        let mut plan = Self::from_reference_va_parameters(raw)?;
        let field = |n: usize| u32::from_le_bytes(noise[n * 4..n * 4 + 4].try_into().unwrap());
        if field(0) as u64 != plan.reference_start_frame {
            return Err("Noise frame snapshot clock differs");
        }
        plan.initial.mixer_noise.state = field(3) as i32;
        Ok(plan)
    }
    pub fn from_reference_parameters(raw: &[u8]) -> Result<Self, &'static str> {
        Self::from_records(raw, 696)
    }
    pub fn from_reference_va_parameters(raw: &[u8]) -> Result<Self, &'static str> {
        Self::from_records(raw, 704)
    }
    fn from_records(raw: &[u8], record_bytes: usize) -> Result<Self, &'static str> {
        if raw.is_empty() || !raw.len().is_multiple_of(record_bytes) {
            return Err("Prepared voice parameter record length");
        }
        let first = &raw[..record_bytes];
        let p = |i| parameter(first, i);
        let q = |i| pair(first, i);
        let transfer = match p(41) as u16 {
            0xb2ac => Transfer::CorrectedRamp,
            0xb320 => Transfer::Pulse,
            0xb380 => Transfer::ParabolicSine,
            0xb3e8 => Transfer::FoldedTriangle,
            _ => return Err("Unqualified OSC2 transfer/modulation"),
        };
        let initial = Voice {
            mixer_noise: Default::default(),
            primary: PrimaryOscillator {
                phase: Phase(word(first, 161)),
                offset: p(15),
                modulated_phase: Phase(if record_bytes == 704 {
                    word(first, 174)
                } else {
                    0
                }),
                control_feedback: p(6),
                unison: radias_synth_domain::unison::UnisonOscillator {
                    phases: core::array::from_fn(|i| {
                        let state = q([22, 24, 26, 30, 32][i]) as u32;
                        Phase(
                            if i == 0 && [0xc584, 0xc66c, 0xc790, 0xc868].contains(&(p(1) as u16)) {
                                state.wrapping_add(q(4) as u32)
                            } else {
                                state
                            },
                        )
                    }),
                },
                noise: radias_synth_domain::noise::NoiseFilterState {
                    first: q(14),
                    second: q(16),
                },
                formant: radias_synth_domain::noise::FormantState {
                    counter: p(10),
                    filter: radias_synth_domain::noise::NoiseFilterState {
                        first: q(12),
                        second: q(14),
                    },
                },
            },
            secondary: Oscillator::new(
                Phase(if record_bytes == 704 {
                    word(first, 162).wrapping_sub(q(38) as u32)
                } else {
                    word(first, 162)
                }),
                PhaseIncrement(q(38) as u32),
                (p(40) as i32 as u32) << 16,
                transfer,
                ShapeParameters {
                    subtract_edge: p(43) != 0,
                    edge_coefficient: p(44),
                    waveform_control: p(45),
                    gain: 32767,
                },
            ),
            filter: ResonantFilter {
                state: FilterState {
                    first: q(136),
                    second: q(138),
                    post: [q(140), q(142)],
                },
            },
            second_filter: radias_synth_domain::filter_routing::Filter2 {
                state: radias_synth_domain::filter_routing::Filter2State {
                    first: q(144),
                    second: q(146),
                },
            },
            envelope: EnvelopeLevel(p(126)),
            waveshaper: radias_synth_domain::waveshaper::Waveshaper {
                state: radias_synth_domain::waveshaper::ShaperState {
                    words: [q(152), q(154), q(156), q(158)],
                    startup_counter: p(92) as u16,
                },
            },
            previous_secondary: radias_synth_domain::Sample(if record_bytes == 704 {
                word(first, 175) as i32
            } else {
                0
            }),
            previous_primary: Phase(if record_bytes == 704 {
                word(first, 171)
            } else {
                0
            }),
        };
        let parameters = parameters(first)?;
        let mut previous = parameters;
        let mut events = Vec::new();
        let start = word(first, 0) as u64;
        let bus =
            radias_synth_domain::pan::VoiceBus::from_offsets(word(first, 172), word(first, 173))
                .ok_or("Unqualified voice bus routing")?;
        for (index, r) in raw.chunks_exact(record_bytes).enumerate() {
            if word(r, 0) as u64 != start + index as u64 {
                return Err("Prepared voice sample clock discontinuity");
            }
            if radias_synth_domain::pan::VoiceBus::from_offsets(word(r, 172), word(r, 173))
                != Some(bus)
            {
                return Err("Prepared voice bus changes need a routing event");
            }
            let next = crate::prepared::parameters(r)?;
            if next != previous {
                events.push(VoiceControlEvent {
                    frame: index as u64,
                    parameters: next,
                });
                previous = next;
            }
        }
        // No recorded outputs or subsequent oscillator/filter states enter
        // this product. The renderer receives only initial state and controls.
        Ok(Self {
            initial,
            parameters,
            events,
            control_slew: radias_synth_domain::control_slew::SlewWeights {
                target: p(150),
                memory: p(151),
            },
            reference_start_frame: start,
            reference_voice_frames: raw.len() / record_bytes,
            bus,
        })
    }
}
