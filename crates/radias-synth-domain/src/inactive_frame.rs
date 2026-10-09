//! Native frame ingress, input follower, twelve inactive stereo tails and
//! output scaling. Memory operations retain their functional service clocks.
use crate::{
    dsp_buffers::{DspBufferBus, DspRole},
    fixed::{high_product, saturate},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InactiveFrameError {
    InvalidFrame,
    ActiveVoice(u8),
    VocoderEnabled,
}
pub struct InactiveFrameWork {
    role: DspRole,
    frame: u8,
    elapsed: u32,
    signal: i64,
    other: i64,
    delta: i64,
    follower_source: u16,
    routing: [u16; 2],
    output: u16,
}
impl InactiveFrameWork {
    pub fn prepare(
        role: DspRole,
        frame: u8,
        bus: &impl DspBufferBus,
    ) -> Result<Self, InactiveFrameError> {
        if frame >= 4 {
            return Err(InactiveFrameError::InvalidFrame);
        }
        if bus.read_data(0x3800) != 0 {
            return Err(InactiveFrameError::VocoderEnabled);
        }
        for slot in 0..12 {
            if bus.read_data(0x2000 + 160 * slot) != 0 {
                return Err(InactiveFrameError::ActiveVoice(slot as u8));
            }
        }
        Ok(Self {
            role,
            frame,
            elapsed: 0,
            signal: 0,
            other: 0,
            delta: 0,
            follower_source: 0,
            routing: [0; 2],
            output: 0,
        })
    }
    pub fn elapsed(&self) -> u32 {
        self.elapsed
    }
    pub fn complete(&self) -> bool {
        self.elapsed == 92 + u32::from(self.frame) + 12 * 33 + 42
    }
    fn pair(bus: &impl DspBufferBus, address: u16) -> i32 {
        ((u32::from(bus.read_data(address)) << 16) | u32::from(bus.read_data(address ^ 1))) as i32
    }
    fn store(bus: &mut impl DspBufferBus, clock: u32, address: u16, value: i32) {
        bus.write_data(clock, address, (value as u32 >> 16) as u16);
        bus.write_data(clock, address ^ 1, value as u16);
    }
    fn copy(bus: &mut impl DspBufferBus, clock: u32, source: u16, target: u16) {
        let value = Self::pair(bus, source);
        Self::store(bus, clock, target, value);
    }
    fn low_product(bus: &impl DspBufferBus, sample: u16) -> i64 {
        i64::from(bus.read_data(sample + 1)) * i64::from(bus.read_data(0x401c) as i16) * 2
    }
    fn high_product(bus: &impl DspBufferBus, sample: u16) -> i64 {
        high_product(bus.read_data(sample) as i16, bus.read_data(0x401c) as i16)
    }
    fn ingress(&mut self, phase: u32, clock: u32, bus: &mut impl DspBufferBus) {
        let source = 0x462 + u16::from(self.frame) * 32;
        match phase {
            9 => bus.write_data(clock, 0x4001, source),
            13..=16 => Self::copy(
                bus,
                clock,
                source + 2 * (phase - 13) as u16,
                0x4002 + 2 * (phase - 13) as u16,
            ),
            18..=41 => {
                let pair = (phase - 18) / 3;
                match (phase - 18) % 3 {
                    0 => self.signal = i64::from(Self::pair(bus, source + 8 + 2 * pair as u16)),
                    1 => self.signal >>= 4,
                    _ => Self::store(bus, clock, 0x400a + 2 * pair as u16, self.signal as i32),
                }
            }
            43 => self.follower_source = bus.read_data(0x4028),
            45 => self.signal = i64::from(Self::pair(bus, 0x4002)),
            46 => Self::store(bus, clock, 0x401a, self.signal as i32),
            47 => self.other = i64::from(Self::pair(bus, 0x4004)),
            48 => Self::store(bus, clock, 0x401c, self.other as i32),
            49 => self.other += self.signal,
            50 => self.other >>= 1,
            51 => Self::store(bus, clock, 0x401e, saturate(self.other)),
            52 => self.signal = i64::from(Self::pair(bus, 0x4006)),
            53 => self.signal += i64::from(Self::pair(bus, 0x4008)),
            54 => self.signal >>= 1,
            55 => Self::store(bus, clock, 0x4020, saturate(self.signal)),
            56 => bus.write_data(clock, 0x4022, 0),
            57 => bus.write_data(clock, 0x4023, 0),
            58 => {
                self.signal = i64::from(bus.read_data(self.follower_source.wrapping_add(1)))
                    * i64::from(bus.read_data(0x4029) as i16)
                    * 2
            }
            59 => {
                self.signal = (self.signal >> 16)
                    + high_product(
                        bus.read_data(self.follower_source) as i16,
                        bus.read_data(0x4029) as i16,
                    )
            }
            60 => self.signal <<= 4,
            61 => self.signal = i64::from(saturate(self.signal)),
            62 => self.signal |= 1,
            63 => self.signal = self.signal.abs(),
            65 => bus.write_data(clock, 0x4020, bus.read_data(0x402a)),
            66 => self.other = self.signal,
            67 => self.delta = self.signal - (i64::from(bus.read_data(0x402c) as i16) << 16),
            68 if self.delta < 0 => bus.write_data(clock, 0x4020, bus.read_data(0x402b)),
            69 => {
                // The signed 17-bit quotient multiplies fractionally; that
                // product wraps to 32 bits before the 40-bit subtraction.
                let product = i64::from(bus.read_data(0x4020) as i16) * (self.delta >> 16) * 2;
                self.other -= i64::from(product as i32);
            }
            70 => bus.write_data(clock, 0x402c, (saturate(self.other) >> 16) as u16),
            73 => self.signal = i64::from(Self::pair(bus, 0x4002)),
            74 => self.signal |= 1,
            75 => self.signal = self.signal.abs(),
            77 => self.delta = self.signal - (i64::from(bus.read_data(0x402e) as i16) << 16),
            80 => self.other = i64::from(Self::pair(bus, 0x4004)),
            81 => self.other |= 1,
            82 if self.delta > 0 => bus.write_data(clock, 0x402e, (self.signal >> 16) as u16),
            84 => self.other = self.other.abs(),
            86 => self.delta = self.other - (i64::from(bus.read_data(0x402f) as i16) << 16),
            90 if self.delta > 0 => bus.write_data(clock, 0x402f, (self.other >> 16) as u16),
            _ => {}
        }
    }
    fn inactive_voice(
        &mut self,
        slot: u16,
        phase: u32,
        clock: u32,
        bus: &mut impl DspBufferBus,
    ) -> Result<(), InactiveFrameError> {
        let frame = 0x3000 + 64 * slot;
        match phase {
            1 if bus.read_data(0x2000 + 160 * slot) != 0 => {
                return Err(InactiveFrameError::ActiveVoice(slot as u8));
            }
            7 => self.routing = [bus.read_data(frame + 10), bus.read_data(frame + 11)],
            8 => Self::copy(bus, clock, frame + 12, 0x401c),
            10 => self.signal = Self::low_product(bus, frame + 14),
            11 => self.signal = (self.signal >> 16) + Self::high_product(bus, frame + 14),
            12 => Self::store(bus, clock, frame + 14, saturate(self.signal)),
            14 => self.other = Self::low_product(bus, frame + 16),
            15 => self.other = (self.other >> 16) + Self::high_product(bus, frame + 16),
            16 => Self::store(bus, clock, frame + 16, saturate(self.other)),
            17 => bus.write_data(clock, 0x401c, 0x8001),
            19 => self.signal = Self::low_product(bus, frame + 14),
            20 => self.signal = (self.signal >> 16) + Self::high_product(bus, frame + 14),
            21 => Self::store(bus, clock, frame + 14, saturate(self.signal)),
            23 => self.other = Self::low_product(bus, frame + 16),
            24 => self.other = (self.other >> 16) + Self::high_product(bus, frame + 16),
            25 => Self::store(bus, clock, frame + 16, saturate(self.other)),
            26 => {
                self.signal += i64::from(Self::pair(bus, 0x400au16.wrapping_add(self.routing[0])))
            }
            27 => self.other += i64::from(Self::pair(bus, 0x400au16.wrapping_add(self.routing[1]))),
            28 => Self::store(
                bus,
                clock,
                0x400au16.wrapping_add(self.routing[0]),
                saturate(self.signal),
            ),
            29 => Self::store(
                bus,
                clock,
                0x400au16.wrapping_add(self.routing[1]),
                saturate(self.other),
            ),
            _ => {}
        }
        Ok(())
    }
    fn output(
        &mut self,
        phase: u32,
        clock: u32,
        bus: &mut impl DspBufferBus,
    ) -> Result<(), InactiveFrameError> {
        match phase {
            3..=26 => {
                let pair = (phase - 3) / 3;
                match (phase - 3) % 3 {
                    0 => self.signal = i64::from(Self::pair(bus, 0x400a + 2 * pair as u16)),
                    1 => self.signal <<= if self.role == DspRole::Master { 5 } else { 4 },
                    _ => Self::store(bus, clock, 0x400a + 2 * pair as u16, saturate(self.signal)),
                }
            }
            27 if bus.read_data(0x3800) != 0 => return Err(InactiveFrameError::VocoderEnabled),
            28 => self.output = bus.read_data(0x4001),
            29 => self.output = self.output.wrapping_add(8),
            32..=39 => Self::copy(
                bus,
                clock,
                0x400a + 2 * (phase - 32) as u16,
                self.output.wrapping_add(2 * (phase - 32) as u16),
            ),
            _ => {}
        }
        Ok(())
    }
    pub fn step(&mut self, bus: &mut impl DspBufferBus) -> Result<bool, InactiveFrameError> {
        if self.complete() {
            return Ok(true);
        }
        let clock = self.elapsed + 1;
        let prefix = 92 + u32::from(self.frame);
        if self.elapsed == 1 {
            bus.write_data(clock, 0x4000, u16::from(self.frame));
        }
        if self.elapsed < prefix {
            if self.elapsed >= 8 + u32::from(self.frame) {
                self.ingress(self.elapsed - u32::from(self.frame), clock, bus);
            }
        } else if self.elapsed < prefix + 12 * 33 {
            let relative = self.elapsed - prefix;
            self.inactive_voice((relative / 33) as u16, relative % 33, clock, bus)?;
        } else {
            self.output(self.elapsed - prefix - 12 * 33, clock, bus)?;
        }
        self.elapsed += 1;
        Ok(self.complete())
    }
}
