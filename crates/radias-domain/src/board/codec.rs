//! Digital AK4626 receive/power boundaries; DAC filtering/soft mute is unresolved.
#[derive(Clone, Default, Debug)]
pub struct I2sInput {
    pub shifting: [u32; 3],
    pub assembling: [u32; 6],
    pub samples: [u32; 6],
    pub level_known: bool,
    pub lrck_high: bool,
    pub collecting: bool,
    pub left_complete: bool,
    pub valid: bool,
    pub bits: u32,
    pub edges: u64,
    pub frames: u64,
    pub short_slots: u64,
}
impl I2sInput {
    pub fn reset_signal(&mut self) {
        self.shifting.fill(0);
        self.assembling.fill(0);
        self.samples.fill(0);
        self.level_known = false;
        self.lrck_high = false;
        self.collecting = false;
        self.left_complete = false;
        self.valid = false;
        self.bits = 0;
    }
    pub fn rising_bick(&mut self, lrck: bool, pins: u8) {
        self.edges += 1;
        if !self.level_known {
            self.level_known = true;
            self.lrck_high = lrck;
            return;
        }
        if self.collecting && self.bits < 24 {
            for pin in 0..3 {
                self.shifting[pin] = self.shifting[pin] << 1 | ((pins as u32 >> pin) & 1);
            }
            self.bits += 1;
            if self.bits == 24 {
                for pin in 0..3 {
                    self.assembling[2 * pin + usize::from(self.lrck_high)] =
                        self.shifting[pin] << 8;
                }
                if !self.lrck_high {
                    self.left_complete = true;
                } else if self.left_complete {
                    self.samples = self.assembling;
                    self.valid = true;
                    self.frames += 1;
                    self.left_complete = false;
                }
            }
        }
        if lrck != self.lrck_high {
            if self.collecting && self.bits < 24 {
                self.short_slots += 1;
                self.left_complete = false;
            }
            self.lrck_high = lrck;
            self.bits = 0;
            self.shifting.fill(0);
            self.collecting = true;
        }
    }
}
#[derive(Clone, Default, Debug)]
pub struct DacInput {
    pub pdn_high: bool,
    pub smute_high: bool,
    pub startup_remaining: u32,
    pub serial: I2sInput,
}
impl DacInput {
    pub fn pins(&mut self, pdn: bool, smute: bool) {
        self.smute_high = smute;
        if pdn == self.pdn_high {
            return;
        }
        self.pdn_high = pdn;
        self.startup_remaining = if pdn { 516 } else { 0 };
        self.serial.reset_signal();
    }
    pub fn ready(&self) -> bool {
        self.pdn_high && self.startup_remaining == 0
    }
    pub fn frame_clock(&mut self) {
        if self.pdn_high && self.startup_remaining != 0 {
            self.startup_remaining -= 1;
        }
    }
    pub fn rising_bick(&mut self, lrck: bool, pins: u8) {
        if self.pdn_high {
            self.serial.rising_bick(lrck, pins);
        }
    }
}
#[derive(Clone, Default, Debug)]
pub struct Codec {
    pub pdn_high: bool,
    pub smute_high: bool,
    pub startup_remaining: u32,
    pub dac: DacInput,
}
impl Codec {
    pub fn pins(&mut self, pdn: bool, smute: bool) {
        self.dac.pins(pdn, smute);
        self.smute_high = smute;
        if pdn == self.pdn_high {
            return;
        }
        self.pdn_high = pdn;
        self.startup_remaining = if pdn { 522 } else { 0 };
    }
    pub fn adc_ready(&self) -> bool {
        self.pdn_high && self.startup_remaining == 0
    }
    pub fn frame(&mut self, input: [u32; 2]) -> [u32; 2] {
        self.dac.frame_clock();
        if !self.pdn_high {
            return [0; 2];
        }
        if self.startup_remaining != 0 {
            self.startup_remaining -= 1;
        }
        if !self.adc_ready() {
            return [0; 2];
        }
        [input[0] & 0xffffff00, input[1] & 0xffffff00]
    }
}
