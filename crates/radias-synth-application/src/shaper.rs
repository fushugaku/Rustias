//! Timbre shaper program; controller arithmetic belongs to the domain.
use radias_synth_domain::{
    controller_shaper::{ShaperControl, WaveshaperType},
    waveshaper::{
        DriveCoefficients, ShaperCoefficients, ShaperParameters, ShaperPosition,
        SubOscillatorCoefficients, SubOscillatorWaveform,
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShaperMode {
    #[default]
    Off,
    Drive,
    HardClip,
    Waveshaper(WaveshaperType),
}
impl ShaperMode {
    pub fn from_panel(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Off),
            1 => Some(Self::Drive),
            2 => Some(Self::HardClip),
            3 => Some(Self::Waveshaper(WaveshaperType::Decimator)),
            4..=12 => WaveshaperType::from_raw(value - 2).map(Self::Waveshaper),
            _ => None,
        }
    }
    pub fn from_allocation(mode: u8, kind: u8) -> Option<Self> {
        match mode {
            0 => Some(Self::Off),
            1 => Some(Self::Drive),
            2 if kind == 1 => Some(Self::HardClip),
            2 => WaveshaperType::from_raw(kind).map(Self::Waveshaper),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShaperProgram {
    pub mode: ShaperMode,
    pub position: ShaperPosition,
    pub control: ShaperControl,
}
impl Default for ShaperProgram {
    fn default() -> Self {
        Self {
            mode: ShaperMode::Off,
            position: ShaperPosition::PreFilter,
            control: ShaperControl::default(),
        }
    }
}
impl ShaperProgram {
    pub fn parameters(self) -> Option<ShaperParameters> {
        self.parameters_with_pitch(0)
    }
    pub fn parameters_with_pitch(self, pitch_code: u16) -> Option<ShaperParameters> {
        let coefficients = match self.mode {
            ShaperMode::Off => return None,
            ShaperMode::Drive => ShaperCoefficients::Drive(DriveCoefficients {
                depth: self.control.drive_depth(),
                normalization: 23920,
                feedback_gain: 0,
                threshold: 19660,
                curves: [26214, -26214],
            }),
            ShaperMode::HardClip => ShaperCoefficients::HardClip {
                depth: self.control.hard_clip_depth(),
            },
            ShaperMode::Waveshaper(kind) => {
                let depth = self.control.waveshaper_depth(kind);
                match kind {
                    WaveshaperType::Decimator => ShaperCoefficients::Decimator { depth },
                    WaveshaperType::HardClip => ShaperCoefficients::HardClip { depth },
                    WaveshaperType::OctSaw => ShaperCoefficients::OctSaw { depth },
                    WaveshaperType::MultiTriangle => ShaperCoefficients::MultiTriangle { depth },
                    WaveshaperType::MultiSine => ShaperCoefficients::MultiSine { depth },
                    WaveshaperType::Pickup => ShaperCoefficients::Pickup {
                        depth,
                        pitch_current: pitch_code as i16,
                    },
                    WaveshaperType::LevelBoost => ShaperCoefficients::LevelBoost { depth },
                    kind => ShaperCoefficients::SubOscillator(SubOscillatorCoefficients {
                        waveform: match kind {
                            WaveshaperType::SubSaw => SubOscillatorWaveform::Saw,
                            WaveshaperType::SubSquare => SubOscillatorWaveform::Square,
                            WaveshaperType::SubTriangle => SubOscillatorWaveform::Triangle,
                            WaveshaperType::SubSine => SubOscillatorWaveform::Sine,
                            _ => unreachable!(),
                        },
                        depth,
                        target_depth: depth,
                        gain_current: 0,
                    }),
                }
            }
        };
        Some(ShaperParameters {
            position: self.position,
            coefficients,
        })
    }
    pub fn allocation_mode(self) -> u8 {
        match self.mode {
            ShaperMode::Off => 0,
            ShaperMode::Drive => 1,
            ShaperMode::HardClip | ShaperMode::Waveshaper(_) => 2,
        }
    }
    pub fn allocation_type(self) -> u8 {
        match self.mode {
            ShaperMode::Waveshaper(kind) => kind as u8,
            ShaperMode::HardClip => 1,
            _ => 0,
        }
    }
}
