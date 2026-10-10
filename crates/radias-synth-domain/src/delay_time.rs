//! Original L/C/R and stereo delay time preparation and host word encoding.
pub struct DelayTimeTables {
    pub free_ratio: [u16; 128],
    pub sync_ratio: [u16; 128],
    pub notes: [u16; 14],
    pub lcr_milliseconds: [u16; 128],
    pub stereo_milliseconds: [u16; 128],
    pub feedback_ratio: [u32; 128],
    pub feedback_threshold: [u16; 128],
    pub feedback_coefficient: [u32; 128],
    pub feedback_before_table: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DelayTimeState {
    pub cached_tempo: u16,
    pub capacity: u32,
    pub ratio: u32,
    pub limited: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayClock {
    pub tempo: u16,
    pub status: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedDelayTimes {
    pub state: DelayTimeState,
    pub frames: [u32; 3],
}
pub(crate) fn divide_1000_unsigned(value: u32) -> u32 {
    ((u64::from(value) * 0x10624dd3) >> 32) as u32 >> 6
}
fn divide_1000_signed(value: u32) -> u32 {
    let high = ((i64::from(value as i32) * 0x10624dd3) >> 32) as i32;
    let value = high >> 6;
    value.wrapping_add(i32::from(value < 0)) as u32
}
pub(crate) fn divide_192(value: u32) -> u32 {
    ((u64::from(value) * 0xaaaaaaab) >> 32) as u32 >> 7
}
fn rescale(mut value: u32, capacity: u32) -> u32 {
    let mut shifts = 0;
    while value > capacity && shifts < 8 {
        value >>= 1;
        shifts += 1;
    }
    shifts
}
/// SYS070EB8 preserves the special 640 word before applying the sample shift.
pub fn encode_delay_frames(frames: u32, shift: u32) -> u32 {
    let value = frames.wrapping_sub(1);
    let value = if (value as i32) < 1 { 640 } else { value };
    let value = if value == 640 {
        value
    } else {
        value.wrapping_shl(shift)
    };
    if (value as i32) > 640 { value } else { 640 }
}
impl DelayTimeTables {
    fn sync_tempo(
        &self,
        ratio: u32,
        longest_note: u32,
        state: &mut DelayTimeState,
        capacity: u32,
        clock: DelayClock,
    ) -> Option<u32> {
        let mut tempo = u32::from(clock.tempo);
        if tempo == 0 {
            return None;
        }
        if clock.status & 3 != 0 {
            let whole_beat = 600000u32 / tempo;
            let duration = divide_192(whole_beat.wrapping_mul(ratio).wrapping_mul(longest_note));
            let frames = divide_1000_unsigned(duration.wrapping_mul(48));
            let unit_frames = frames;
            let duration_limit = divide_1000_unsigned(capacity.wrapping_mul(1007));
            if tempo < u32::from(state.cached_tempo) {
                let shifts = rescale(unit_frames, duration_limit);
                if (unit_frames >> shifts) > capacity {
                    tempo = u32::from(state.cached_tempo);
                }
            }
        }
        state.cached_tempo = tempo as u16;
        (tempo != 0).then_some(tempo)
    }
    pub fn lcr(
        &self,
        parameters: &[u8; 20],
        mut state: DelayTimeState,
        clock: DelayClock,
    ) -> Option<PreparedDelayTimes> {
        let sync = parameters[1] != 0;
        let ratio = u32::from(*if sync {
            self.sync_ratio.get(usize::from(parameters[2]))?
        } else {
            self.free_ratio.get(usize::from(parameters[2]))?
        });
        let indices = if sync {
            [parameters[6], parameters[7], parameters[8]]
        } else {
            [parameters[3], parameters[4], parameters[5]]
        };
        let longest = if indices[0] >= indices[1] && indices[0] >= indices[2] {
            0
        } else if indices[1] >= indices[0] && indices[1] >= indices[2] {
            1
        } else {
            2
        };
        let mut frames = [0; 3];
        if sync {
            let note = u32::from(*self.notes.get(usize::from(indices[longest]))?);
            let capacity = state.capacity;
            let tempo = self.sync_tempo(ratio, note, &mut state, capacity, clock)?;
            let base =
                divide_1000_unsigned((600000u32 / tempo).wrapping_mul(ratio).wrapping_mul(48));
            for (output, index) in frames.iter_mut().zip(indices) {
                *output =
                    divide_192(base.wrapping_mul(u32::from(*self.notes.get(usize::from(index))?)));
            }
        } else {
            for (output, index) in frames.iter_mut().zip(indices) {
                *output = divide_1000_signed(
                    ratio
                        .wrapping_mul(48)
                        .wrapping_mul(u32::from(*self.lcr_milliseconds.get(usize::from(index))?)),
                );
            }
        }
        if frames.iter().any(|&v| v > state.capacity) {
            let shift = rescale(frames[longest], state.capacity);
            frames = frames.map(|v| v >> shift);
            state.limited = 1;
        } else {
            state.limited = 0;
        }
        state.ratio = ratio;
        Some(PreparedDelayTimes { state, frames })
    }
    pub fn stereo(
        &self,
        parameters: &[u8; 20],
        state: DelayTimeState,
        clock: DelayClock,
    ) -> Option<PreparedDelayTimes> {
        self.two_channel(
            parameters,
            state,
            clock,
            state.capacity >> 1,
            &self.stereo_milliseconds,
        )
    }
    /// AutoPanDelay uses the L/C/R duration table and the whole buffer;
    /// St.AutoPanDly uses the Stereo table and half the buffer.
    pub fn auto_pan(
        &self,
        parameters: &[u8; 20],
        state: DelayTimeState,
        clock: DelayClock,
        stereo: bool,
    ) -> Option<PreparedDelayTimes> {
        let mut mapped = [0; 20];
        mapped[2..8].copy_from_slice(&parameters[1..7]);
        self.two_channel(
            &mapped,
            state,
            clock,
            if stereo {
                state.capacity >> 1
            } else {
                state.capacity
            },
            if stereo {
                &self.stereo_milliseconds
            } else {
                &self.lcr_milliseconds
            },
        )
    }
    pub(crate) fn two_channel(
        &self,
        parameters: &[u8; 20],
        state: DelayTimeState,
        clock: DelayClock,
        capacity: u32,
        milliseconds: &[u16; 128],
    ) -> Option<PreparedDelayTimes> {
        self.two_channel_scaled(parameters, state, clock, capacity, milliseconds, 1)
    }
    pub(crate) fn two_channel_scaled(
        &self,
        parameters: &[u8; 20],
        mut state: DelayTimeState,
        clock: DelayClock,
        capacity: u32,
        milliseconds: &[u16; 128],
        scale: u32,
    ) -> Option<PreparedDelayTimes> {
        let sync = parameters[2] != 0;
        let ratio = u32::from(*if sync {
            self.sync_ratio.get(usize::from(parameters[3]))?
        } else {
            self.free_ratio.get(usize::from(parameters[3]))?
        });
        let indices = if sync {
            [parameters[6], parameters[7]]
        } else {
            [parameters[4], parameters[5]]
        };
        // In free mode SYS0721B2 keeps R12 zero: the right channel
        // determines the common shift even when the left delay is longer.
        let longest = if sync {
            usize::from(indices[0] <= indices[1])
        } else {
            1
        };
        let mut frames = [0; 3];
        if sync {
            let note = u32::from(*self.notes.get(usize::from(indices[longest]))?);
            let tempo = self.sync_tempo(ratio, note, &mut state, capacity, clock)?;
            let base = (600000u32 / tempo).wrapping_mul(ratio);
            for (output, index) in frames.iter_mut().zip(indices) {
                *output = divide_1000_unsigned(
                    divide_192(base.wrapping_mul(u32::from(*self.notes.get(usize::from(index))?)))
                        .wrapping_mul(48),
                );
            }
        } else {
            for (output, index) in frames.iter_mut().zip(indices) {
                *output = divide_1000_unsigned(
                    ratio
                        .wrapping_mul(u32::from(*milliseconds.get(usize::from(index))?))
                        .wrapping_mul(48),
                );
            }
        }
        if scale != 1 {
            frames = frames.map(|v| v / scale);
        }
        if frames[..2].iter().any(|&v| v > capacity) {
            let shift = rescale(frames[longest], capacity);
            frames = frames.map(|v| v >> shift);
            state.limited = 1;
        } else {
            state.limited = 0;
        }
        state.ratio = ratio;
        Some(PreparedDelayTimes { state, frames })
    }
    pub fn feedback_limit(&self, left: u32, right: u32, feedback: u8) -> Option<u32> {
        if left == 0 || right == 0 {
            return Some(0);
        }
        let ratio = right.wrapping_mul(*self.feedback_ratio.get(usize::from(feedback))?) / left;
        let target = u32::from(ratio as u16);
        let index = self
            .feedback_threshold
            .partition_point(|&v| u32::from(v) < target);
        Some(if index < 128 {
            self.feedback_coefficient[index]
        } else {
            self.feedback_before_table
        })
    }
}
