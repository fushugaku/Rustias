//! Original external-clock measurement, independent of a host timer or MIDI API.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockTimestamp {
    pub countdown: u32,
    pub timeout_ticks: u8,
    pub flags: u8,
}

impl ClockTimestamp {
    /// SYS 02e90c expires on the service after the counter reaches zero.
    pub fn service_timeout(&mut self) {
        if self.timeout_ticks == 0 {
            self.flags |= 1;
            self.timeout_ticks = 250;
        } else {
            self.timeout_ticks -= 1;
        }
    }
    /// SYS 02e874 reads the countdown once, rearms the timeout and consumes
    /// only the expired flag. The first observation after expiry is invalid.
    pub fn observe(&mut self, current_countdown: u32) -> u32 {
        let elapsed = self.countdown.wrapping_sub(current_countdown);
        self.countdown = current_countdown;
        self.timeout_ticks = 250;
        if self.flags & 1 != 0 {
            self.flags &= !1;
            u32::MAX
        } else {
            elapsed
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTimerReload {
    pub constant: u32,
    pub counter: u32,
    pub last_external_interval: u32,
}

/// Nominal 1.5 MHz timer ticks per native 48 kHz synthesis frame. Retain the
/// fractional phase instead of rounding every frame to a fixed tick count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioTimerClock {
    pub fraction: u32,
}
impl AudioTimerClock {
    pub fn next_ticks(&mut self) -> u32 {
        let phase = self.fraction + 1_500_000;
        self.fraction = phase % crate::SAMPLE_RATE;
        phase / crate::SAMPLE_RATE
    }
}

impl Default for ClockTimerReload {
    fn default() -> Self {
        Self {
            constant: 0x1e84,
            counter: 0x1e84,
            last_external_interval: 0,
        }
    }
}

impl ClockTimerReload {
    /// Countdown/reload boundary of the original TMU peripheral. A period
    /// includes zero; multiple expirations preserve the remainder exactly.
    pub fn advance_ticks(&mut self, ticks: u32) -> u32 {
        let first = self.counter as u64 + 1;
        if (ticks as u64) < first {
            self.counter -= ticks;
            return 0;
        }
        let remaining = ticks as u64 - first;
        let period = self.constant as u64 + 1;
        self.counter = self.constant - (remaining % period) as u32;
        (1 + remaining / period) as u32
    }
    /// SYS 02e658 truncates its input to a word and clamps 20.0..300.0 BPM.
    /// Its source gate preserves both TMU1 registers in mode 2.
    pub fn set_tempo(&mut self, raw_tempo: u32, source_mode: u8) {
        if source_mode == 2 {
            return;
        }
        let tempo = (raw_tempo as u16 as u32).clamp(200, 3000);
        let reload = 9_375_000 / tempo;
        self.constant = reload;
        self.counter = reload;
    }

    /// SYS 02e6c0 keeps the low product word, then divides before shifting.
    /// A zero divisor follows a distinct original path, retaining the product.
    pub fn set_external_interval(&mut self, interval: u32, divisor: u32) {
        if interval != 0 {
            self.last_external_interval = interval;
        }
        let product = interval.wrapping_mul(24);
        let reload = if let Some(quotient) = product.checked_div(divisor) {
            (quotient >> 4).max(1)
        } else {
            product
        };
        self.constant = reload;
        self.counter = reload;
    }
}

/// Mode 2's original 00ad98 timer branch; distinct from LFO clock delivery.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SequencedClockCounter {
    pub countdown: u16,
    pub remaining: u16,
    pub block_length: u16,
    pub completed_blocks: u8,
}
impl SequencedClockCounter {
    pub fn service(&mut self) {
        self.countdown = self.countdown.wrapping_sub(1);
        if self.remaining < 24 {
            self.completed_blocks = self.completed_blocks.wrapping_add(1);
            self.remaining = (self.block_length as i16 as i32 - 24) as u16;
        } else {
            self.remaining -= 24;
        }
    }
}

/// SYS 02e81c: rounded unsigned conversion before the caller's tempo clamp.
pub fn interval_to_tenths_bpm(interval: u32) -> u32 {
    37_500_000u32
        .wrapping_add(interval >> 1)
        .checked_div(interval)
        .unwrap_or(0)
}

/// Firmware contexts 0c14a9cc and 0c14aa54 respectively. Transport names
/// belong to the MIDI adapter once the original routing is established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockInputPort {
    A,
    B,
    Other,
}

/// Original source-selection bits in global setting byte +7.
pub fn accepts_clock(settings: u8, port: ClockInputPort) -> bool {
    match settings & 12 {
        0 => true,
        4 => false,
        8 => port == ClockInputPort::B,
        12 => port == ClockInputPort::A,
        _ => unreachable!(),
    }
}

/// SYS 026f88 suppresses propagation from context A when this link bit is set.
pub fn propagation_suppressed(port: ClockInputPort, link_flags: u8) -> bool {
    port == ClockInputPort::A && link_flags & 2 != 0
}

/// SYS 028bdc prioritizes context B in automatic mode. The upper mode bit
/// distinguishes automatic selection from the corresponding forced source.
pub fn selected_clock_mode(settings: u8, flags_a: u8, flags_b: u8) -> u8 {
    match settings & 12 {
        4 => 0,
        12 => 1,
        8 => 2,
        _ => {
            if flags_b & 2 != 0 {
                6
            } else if flags_a & 2 != 0 {
                5
            } else {
                4
            }
        }
    }
}

/// Clock portion of 027b84, with the original busy-transfer early return.
pub fn service_clock_liveness(
    settings: u8,
    busy: bool,
    timeout: &mut u16,
    refresh: &mut u16,
    flags: &mut u8,
) {
    if busy || *timeout == 0 {
        return;
    }
    *timeout -= 1;
    if settings & 12 == 4 {
        return;
    }
    if *timeout == 0 {
        *flags &= !3;
        return;
    }
    let next = *refresh as u32 + 1;
    if next >= 1000 {
        *refresh = 0;
        if *flags & 8 != 0 {
            *flags |= 4;
        }
    } else {
        *refresh = next as u16;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockIntervalMeasurement {
    pub timestamp: ClockTimestamp,
    pub interval: u32,
    pub flags: u8,
    pub warmup_count: u8,
}

/// Original 0289c0 deferred tempo notification, serviced by 0289f4/028a0c.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TempoUpdateLatch {
    pub tenths_bpm: u16,
    pub pending: u16,
}

impl TempoUpdateLatch {
    pub fn request(&mut self, raw: u32) {
        let previous = self.tenths_bpm;
        self.tenths_bpm = raw as u16;
        if raw != previous as u32 && self.pending == 0 {
            self.pending = 125;
        }
    }
    /// The timer service retains one until the main loop delivers the update.
    pub fn service(&mut self) {
        if self.pending > 1 {
            self.pending -= 1;
        }
    }
    pub fn take_ready(&mut self) -> Option<u16> {
        if self.pending == 1 {
            self.pending = 0;
            Some(self.tenths_bpm)
        } else {
            None
        }
    }
}

/// SYS 029128/0291aa interval history and adaptive fixed-point filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockIntervalHistory {
    pub changed: u8,
    pub cursor: u8,
    pub instability: u8,
    pub intervals: [u32; 16],
    pub half_sum: u32,
    pub value: u32,
    pub scaled_value: u32,
    pub accepted_value: u32,
    pub delay: u8,
    pub direction: u8,
}

impl ClockIntervalHistory {
    /// Original 029344 consumes the change flag before its source/gate checks.
    /// Accepted history is eight times the raw interval; conversion shifts
    /// before division and uses the timer's 20.0..300.0 BPM range.
    pub fn take_accepted_tempo(
        &mut self,
        source_mode: u8,
        expected_mode: u32,
        measurement_flags: u8,
        controller_mode: u8,
    ) -> Option<u16> {
        if self.changed == 0 {
            return None;
        }
        self.changed = 0;
        if (source_mode & 3) as u32 != expected_mode
            || measurement_flags & 2 == 0
            || controller_mode == 2
        {
            return None;
        }
        Some(interval_to_tenths_bpm(self.accepted_value >> 3).clamp(200, 3000) as u16)
    }
    pub fn observe(&mut self, interval: u32, acceptance_delay: u8) {
        self.cursor = self.cursor.wrapping_add(4) & 0x3c;
        if self.instability == 3 {
            self.intervals.fill(interval);
        } else {
            self.intervals[(self.cursor >> 2) as usize] = interval;
        }
        self.half_sum = self
            .intervals
            .iter()
            .fold(0u32, |sum, v| sum.wrapping_add(*v))
            >> 1;
        let difference = self.value.wrapping_sub(self.half_sum) as i32;
        let deviation = if difference < 0 {
            difference.wrapping_neg() as u32
        } else {
            difference as u32
        };
        if deviation >= 0x9999 {
            self.instability = 3;
            self.scaled_value = self.half_sum.wrapping_shl(4);
            self.value = self.half_sum;
        } else {
            let mut instability = self.instability as i32;
            let mut strength = 0;
            if deviation >= 0x999 {
                instability += 1;
                if instability >= 3 {
                    self.instability = 3;
                    strength = if deviation >= 0x1ccc {
                        3
                    } else if deviation >= 0x1333 {
                        2
                    } else {
                        1
                    };
                }
            }
            let removed = match strength {
                0 => self.scaled_value >> 3,
                1 => self.scaled_value >> 1,
                2 => self.scaled_value,
                _ => self.scaled_value.wrapping_shl(1),
            };
            let inserted = self.half_sum.wrapping_shl(match strength {
                0 => 1,
                1 => 3,
                2 => 4,
                _ => 5,
            });
            self.scaled_value = self
                .scaled_value
                .wrapping_shl(4)
                .wrapping_sub(removed)
                .wrapping_add(inserted)
                >> 4;
            self.value = self.scaled_value >> 4;
            if strength == 0 {
                self.instability = (instability - 1).max(0) as u8;
            }
        }
        let difference = self.accepted_value.wrapping_sub(self.value) as i32;
        if difference == 0 {
            return;
        }
        let direction = u8::from(difference >= 0);
        let same_direction = self.direction == direction;
        self.direction = direction;
        if !same_direction {
            self.delay = acceptance_delay;
        } else {
            if self.delay != 0 {
                self.delay -= 1;
                if self.delay != 0 {
                    return;
                }
            }
            self.accepted_value = self.value;
            self.changed = 1;
        }
    }
}

impl ClockIntervalMeasurement {
    /// SYS 026df4..026e6c, after selection and pending-start processing.
    /// Returns the measured value passed to subsequent interval smoothing.
    pub fn observe(&mut self, current_countdown: u32) -> u32 {
        if self.flags & 1 == 0 {
            self.flags = (self.flags | 1) & !2;
            self.interval = self.timestamp.observe(current_countdown);
            self.warmup_count = 0;
            return 0;
        }
        let measured = self.timestamp.observe(current_countdown);
        let count = self.warmup_count as u32 + 1;
        if count < 6 {
            self.interval = measured;
            self.warmup_count = count as u8;
        } else {
            self.flags |= 2;
            self.warmup_count = 0;
        }
        if self.flags & 2 != 0 { measured } else { 0 }
    }

    /// SYS 026e86..026e9a. Signed half-difference with the original +1 rule.
    pub fn smooth_interval(&mut self, measured: u32) {
        let difference = measured.wrapping_sub(self.interval) as i32;
        let mut adjustment = difference >> 1;
        if difference != 0 && adjustment == 0 {
            adjustment = 1;
        }
        self.interval = self.interval.wrapping_add(adjustment as u32);
    }
}
