//! Original Comb fractional interpolation, feedback and DC-blocking arithmetic.
use crate::{
    Sample,
    fixed::{high_product, multiply_q15, saturate},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CombFeedbackState {
    pub interpolated: i32,
    pub previous_drive: i32,
    pub dc_blocked: i32,
}
#[derive(Clone, Copy, Debug)]
pub struct CombFeedbackInput {
    pub sample: Sample,
    pub delay_samples: [i16; 2],
    pub fraction: i16,
    pub feedback: i32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CombFeedback {
    pub state: CombFeedbackState,
}
impl CombFeedback {
    pub fn prepare(&mut self, input: CombFeedbackInput) -> Sample {
        let previous = self.state;
        let interpolated = saturate(
            ((input.delay_samples[1] as i64) << 16)
                + high_product(input.delay_samples[0], input.fraction)
                - high_product((previous.interpolated >> 16) as i16, input.fraction),
        );
        let drive = saturate(
            (input.sample.0 >> 1) as i64
                + multiply_q15(input.feedback, (interpolated >> 16) as i16),
        );
        let difference = multiply_q15(drive, 32765) - multiply_q15(previous.previous_drive, 32765);
        let feedback = saturate(multiply_q15(previous.dc_blocked, -32763));
        let output = saturate(difference + multiply_q15(feedback, -32767));
        self.state = CombFeedbackState {
            interpolated,
            previous_drive: drive,
            dc_blocked: output,
        };
        Sample(output)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CombReadPosition {
    pub current: u16,
    pub previous: u16,
    pub fraction: i16,
}
/// Original B7CC..B804 ring selection and B843..B856 allpass coefficient.
pub fn read_position(delay_q16: u32, write_cursor_bytes: u16, group_phase: u8) -> CombReadPosition {
    let current = ((write_cursor_bytes as i32 >> 1) + group_phase as i32
        - (delay_q16 >> 16) as u16 as i16 as i32
        + 5)
        & 4095;
    let fraction = ((delay_q16 as u16 >> 1) & 32767) as i32;
    let first = ((fraction << 16) >> 1) - 0x7fff0000;
    let second = (fraction << 16) - 0x7fff0000;
    let coefficient = saturate(high_product((first >> 16) as i16, (second >> 16) as i16));
    CombReadPosition {
        current: current as u16,
        previous: ((current - 1) & 4095) as u16,
        fraction: (coefficient >> 16) as i16,
    }
}

pub const COMB_DELAY_SAMPLES: usize = 4096;

/// Per-voice16-bit external delay line and the original four-sample staging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CombDelay {
    pub samples: [i16; COMB_DELAY_SAMPLES],
    pub pending: [i16; 4],
    pub read_samples: [i16; 2],
    pub fraction: i16,
    pub write_cursor_bytes: u16,
    pub group_phase: u8,
}
impl Default for CombDelay {
    fn default() -> Self {
        Self {
            samples: [0; COMB_DELAY_SAMPLES],
            pending: [0; 4],
            read_samples: [0; 2],
            fraction: 0,
            write_cursor_bytes: 0,
            group_phase: 0,
        }
    }
}
impl CombDelay {
    pub fn begin_frame(&mut self) {
        if self.group_phase == 0 {
            let start = (self.write_cursor_bytes >> 1) as usize;
            for (n, &sample) in self.pending.iter().enumerate() {
                self.samples[(start + n) & 4095] = sample;
            }
        }
    }
    pub fn finish_frame(&mut self, prepared: Sample, delay_q16: u32) {
        self.pending[self.group_phase as usize] = (prepared.0 >> 16) as i16;
        let position = read_position(delay_q16, self.write_cursor_bytes, self.group_phase);
        self.read_samples = [
            self.samples[position.current as usize],
            self.samples[position.previous as usize],
        ];
        self.fraction = position.fraction;
        self.group_phase = (self.group_phase + 1) & 3;
        if self.group_phase == 0 {
            self.write_cursor_bytes = self.write_cursor_bytes.wrapping_add(8) & 8191;
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comb {
    pub feedback: CombFeedback,
    pub delay: CombDelay,
}
impl Comb {
    pub fn sample(&mut self, input: Sample, feedback: i32, delay_q16: u32) -> Sample {
        self.delay.begin_frame();
        let sample = self.feedback.prepare(CombFeedbackInput {
            sample: input,
            delay_samples: self.delay.read_samples,
            fraction: self.delay.fraction,
            feedback,
        });
        self.delay.finish_frame(sample, delay_q16);
        sample
    }
}
