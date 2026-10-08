use super::{
    BUS_WORDS_PER_SAMPLE, CPU_HZ, SAMPLE_HZ,
    codec::Codec,
    fxd::{HostWindow, Upload},
    nor::{MASK, NorFlash, SIZE},
    registers::RegisterBytes,
    scif::Scif,
};
use crate::{
    backup::BackupExtras,
    controller::{Bus, Interrupt},
    dsp::c55::C55,
};
use std::collections::{BTreeMap, VecDeque};

pub struct Lcd {
    pub data: [u8; 132 * 8],
    pub page: u32,
    pub column: u32,
    pub start_line: u32,
    pub enabled: bool,
    pub invert: bool,
    pub adc_reverse: bool,
    pub com_reverse: bool,
    pub contrast: u32,
    pub parameter: u32,
    pub commands: u64,
    pub writes: u64,
}
impl Default for Lcd {
    fn default() -> Self {
        Self {
            data: [0; 132 * 8],
            page: 0,
            column: 0,
            start_line: 0,
            enabled: false,
            invert: false,
            adc_reverse: false,
            com_reverse: false,
            contrast: 0,
            parameter: 0,
            commands: 0,
            writes: 0,
        }
    }
}
impl Lcd {
    pub fn command(&mut self, v: u8) {
        self.commands += 1;
        if self.parameter != 0 {
            if self.parameter == 0x81 {
                self.contrast = (v & 63) as u32;
            }
            self.parameter = 0;
            return;
        }
        if v & 0xf0 == 0xb0 {
            self.page = (v & 7) as u32;
        } else if v & 0xf0 == 0x10 {
            self.column = (self.column & 15) | ((v as u32 & 15) << 4);
        } else if v & 0xf0 == 0 {
            self.column = (self.column & 0xf0) | (v as u32 & 15);
        } else if v & 0xc0 == 0x40 {
            self.start_line = (v & 63) as u32;
        } else {
            match v {
                0xae => self.enabled = false,
                0xaf => self.enabled = true,
                0xa6 => self.invert = false,
                0xa7 => self.invert = true,
                0xa0 => self.adc_reverse = false,
                0xa1 => self.adc_reverse = true,
                0xc0 => self.com_reverse = false,
                0xc8 => self.com_reverse = true,
                0x81 | 0xac => self.parameter = v as u32,
                0xe2 => {
                    self.page = 0;
                    self.column = 0;
                    self.start_line = 0;
                    self.enabled = false;
                }
                _ => {}
            }
        }
    }
    pub fn write(&mut self, v: u8) {
        if self.column < 132 {
            self.data[(self.page * 132 + self.column) as usize] = v;
        }
        self.column = (self.column + 1) % 132;
        self.writes += 1;
    }
    pub fn pixels(&self) -> Vec<u8> {
        let mut out = vec![0; 128 * 64];
        for y in 0..64 {
            for x in 0..128 {
                let px = if self.adc_reverse { 131 - x } else { x };
                let py =
                    (if self.com_reverse { y } else { 63 - y } + self.start_line as usize) & 63;
                let bit = self.data[(py / 8) * 132 + px] >> (py & 7) & 1 != 0;
                out[y * 128 + x] = u8::from(self.enabled && (bit != self.invert));
            }
        }
        out
    }
}
pub struct Board {
    pub firmware: Vec<u8>,
    pub flash: Vec<u8>,
    pub ram: Vec<u8>,
    pub nor: NorFlash,
    pub backup_global_data: Vec<u8>,
    pub backup_usr_data: Vec<u8>,
    pub backup_librarian: BackupExtras,
    pub regs: RegisterBytes,
    pub inputs: BTreeMap<u32, u8>,
    pub unknown_reads: BTreeMap<u32, u64>,
    pub timer_phase: [u64; 3],
    pub panel_keys: [u8; 8],
    pub panel_adc: [[u16; 8]; 5],
    pub adc_temp: u8,
    pub encoder_bits: u8,
    pub encoder_transitions: VecDeque<u8>,
    pub encoder_clock_phase: u64,
    pub midi_in: VecDeque<u8>,
    pub midi_out: VecDeque<u8>,
    pub scif: Scif,
    pub fxd_upload: Upload,
    pub fxd_host: HostWindow,
    pub dsp_upload: Vec<u8>,
    pub dsp: [C55; 2],
    pub dsp_reads: u64,
    pub dsp_core_clock_phase: u64,
    pub dsp_reset_asserted: bool,
    pub audio_clock_phase: u64,
    pub audio_frames: u64,
    pub dsp_serial_clock_phase: u64,
    pub dsp_serial_frames: u64,
    pub serial_words: u64,
    pub adc_noise_state: u32,
    pub adc_idle_noise: bool,
    pub adc_input: Option<Box<dyn FnMut() -> [u32; 2] + Send>>,
    pub codec: Codec,
    pub fxd_return_zero: bool,
    pub pcm_skipped: bool,
    pub pcm_placeholder_words: u32,
    pub io_log: VecDeque<String>,
    pub lcd: Lcd,
    pub current_pc: u32,
    pub ticks: u64,
    pub usr_reads: u64,
    pub pcm_reads: u64,
    pub io_reads: u64,
    pub io_writes: u64,
    pub strict: bool,
    pub serial_observer: Option<Box<dyn FnMut(u32, u32, u64, u32) + Send>>,
    pub dsp_upload_observer: Option<Box<dyn FnMut(u32, u32, u8) + Send>>,
    pub midi_observer: Option<Box<dyn FnMut(u64, u8) + Send>>,
    pub fxd_observer: Option<Box<dyn FnMut(u64, u32, u32, u8, bool) + Send>>,
}
impl Board {
    pub fn new(
        firmware: Vec<u8>,
        global: Vec<u8>,
        library: Vec<u8>,
        extras: BackupExtras,
    ) -> Result<Self, String> {
        if !global.is_empty() && global.len() != 656 && global.len() != 736 {
            return Err("Unsupported Global flash input".into());
        }
        if !library.is_empty() && !matches!(library.len(), 0x80000 | 0x90000 | 0x100000) {
            return Err("Unsupported native USR flash input".into());
        }
        if firmware.len() > SIZE {
            return Err("SYS input exceeds NOR capacity".into());
        }
        let mut flash = vec![255; SIZE];
        flash[..firmware.len()].copy_from_slice(&firmware);
        flash[0xe0000..0xe0000 + library.len()].copy_from_slice(&library);
        flash[0x16f000..0x16f000 + global.len()].copy_from_slice(&global);
        let mut out = Self {
            firmware,
            flash,
            ram: Vec::new(),
            nor: NorFlash::default(),
            backup_global_data: global,
            backup_usr_data: library,
            backup_librarian: extras,
            regs: RegisterBytes::default(),
            inputs: BTreeMap::new(),
            unknown_reads: BTreeMap::new(),
            timer_phase: [0; 3],
            panel_keys: [0; 8],
            panel_adc: [[512; 8]; 5],
            adc_temp: 0,
            encoder_bits: 3,
            encoder_transitions: VecDeque::new(),
            encoder_clock_phase: 0,
            midi_in: VecDeque::new(),
            midi_out: VecDeque::new(),
            scif: Scif::default(),
            fxd_upload: Upload::default(),
            fxd_host: HostWindow::default(),
            dsp_upload: Vec::new(),
            dsp: [C55::new(), C55::new()],
            dsp_reads: 0,
            dsp_core_clock_phase: 0,
            dsp_reset_asserted: false,
            audio_clock_phase: 0,
            audio_frames: 0,
            dsp_serial_clock_phase: 0,
            dsp_serial_frames: 0,
            serial_words: 0,
            adc_noise_state: 0x52414449,
            adc_idle_noise: true,
            adc_input: None,
            codec: Codec::default(),
            fxd_return_zero: true,
            pcm_skipped: false,
            pcm_placeholder_words: 0,
            io_log: VecDeque::new(),
            lcd: Lcd::default(),
            current_pc: 0,
            ticks: 0,
            usr_reads: 0,
            pcm_reads: 0,
            io_reads: 0,
            io_writes: 0,
            strict: false,
            serial_observer: None,
            dsp_upload_observer: None,
            midi_observer: None,
            fxd_observer: None,
        };
        out.reset();
        Ok(out)
    }
    pub fn reset(&mut self) {
        self.nor.power_cycle();
        self.ram = vec![0; 16 * 1024 * 1024];
        self.fxd_host = HostWindow::default();
        self.regs.clear();
        self.inputs.clear();
        self.unknown_reads.clear();
        self.midi_in.clear();
        self.midi_out.clear();
        self.scif = Scif::default();
        self.fxd_upload = Upload::default();
        self.io_log.clear();
        self.lcd = Lcd::default();
        self.dsp_upload.clear();
        self.dsp_reads = 0;
        self.pcm_skipped = false;
        self.pcm_placeholder_words = 0;
        self.ticks = 0;
        self.usr_reads = 0;
        self.pcm_reads = 0;
        self.io_reads = 0;
        self.io_writes = 0;
        self.audio_clock_phase = 0;
        self.audio_frames = 0;
        self.serial_words = 0;
        self.dsp_serial_clock_phase = 0;
        self.dsp_serial_frames = 0;
        self.dsp_core_clock_phase = 0;
        self.dsp_reset_asserted = false;
        self.adc_noise_state = 0x52414449;
        self.timer_phase.fill(0);
        self.panel_keys.fill(0);
        self.panel_adc = [[512; 8]; 5];
        for i in 4..8 {
            self.panel_adc[4][i] = 0;
        }
        self.adc_temp = 0;
        self.encoder_bits = 3;
        self.encoder_clock_phase = 0;
        self.encoder_transitions.clear();
        for d in &mut self.dsp {
            d.reset();
            d.set_reset(true);
            d.set_reset(false);
        }
        for i in 0..3 {
            self.regs.write32(0xfffffe94 + 12 * i, u32::MAX);
            self.regs.write32(0xfffffe98 + 12 * i, u32::MAX);
        }
        self.regs.set(0xfffffe88, 0x84);
        self.regs.write32(0xffffffd4, 0);
        self.regs.write32(0xffffffd8, 0);
        self.regs.write32(0xa4000000, 0);
        self.codec = Codec::default();
        self.update_codec_pins();
        self.update_fxd_control_lines();
    }
    pub fn physical(a: u32) -> u32 {
        if (0x80000000..0xc0000000).contains(&a) {
            a & 0x1fffffff
        } else {
            a
        }
    }
    pub fn register_address(a: u32) -> u32 {
        let p = Self::physical(a);
        if (0x04000000..0x04000200).contains(&p) {
            p | 0xa0000000
        } else {
            a
        }
    }
    pub fn gpio(&mut self, a: u32) -> u8 {
        let port = (a - 0xa4000120) / 2;
        let control = self.regs.read16(0xa4000100 + 2 * port);
        let written = if self.regs.contains(a) {
            self.regs.get(a)
        } else {
            255
        };
        let mut external = self.inputs.get(&a).copied().unwrap_or(255);
        if a == 0xa4000126 && !self.inputs.contains_key(&a) {
            external = 0xa0;
            for chip in 0..2 {
                if self.dsp[chip].io.get(&0x3400).copied().unwrap_or(0) & 0x10 != 0 {
                    let bit = if chip != 0 { 0x20 } else { 0x80 };
                    external = external & !bit
                        | if self.dsp[chip].io.get(&0x3401).copied().unwrap_or(0) & 0x10 != 0 {
                            bit
                        } else {
                            0
                        };
                }
            }
        }
        if a == 0xa4000120 && !self.inputs.contains_key(&a) {
            let row = self.regs.get(0xa4000130) >> 4 & 7;
            external = 255 ^ if row == 2 { 0x80 } else { 0 } ^ self.panel_keys[row as usize];
        }
        if a == 0xa400012e && !self.inputs.contains_key(&a) {
            external =
                external & !3 | u8::from(!self.dsp[0].hint) | if self.dsp[1].hint { 0 } else { 2 };
        }
        if a == 0xa400012a && !self.inputs.contains_key(&a) {
            external = external & !12 | self.encoder_bits << 2;
        }
        let mut mask = 0;
        for pin in 0..8 {
            if control >> (2 * pin) & 3 == 1 {
                mask |= 1 << pin;
            }
        }
        written & mask | external & !mask
    }
    fn update_codec_pins(&mut self) {
        let pdn = self.gpio(0xa4000136) & 8 != 0;
        let smute = self.gpio(0xa4000128) & 16 != 0;
        self.codec.pins(pdn, smute);
    }
    fn update_fxd_control_lines(&mut self) {
        let scp = self.gpio(0xa4000136);
        self.fxd_host.control_lines(scp & 2 != 0, scp & 4 != 0);
    }
    pub fn refresh_control_pins(&mut self) {
        self.update_codec_pins();
        self.update_fxd_control_lines();
    }
    pub fn codec_bick_rising(&mut self, lrck: bool, so1: bool, so2: bool) {
        self.codec
            .dac
            .rising_bick(lrck, u8::from(so1) | u8::from(so2) << 1 | 4);
    }
    fn log(&mut self, write: bool, a: u32, v: u32) {
        self.io_log.push_back(format!(
            "{:08x} {} {a:08x} = {v:02x}",
            self.current_pc,
            if write { 'W' } else { 'R' }
        ));
        if self.io_log.len() > 48 {
            self.io_log.pop_front();
        }
    }
    fn observe_fxd(&mut self, address: u32, value: u8, write: bool) {
        if let Some(observer) = &mut self.fxd_observer {
            observer(self.ticks, self.current_pc, address, value, write);
        }
    }
    fn count_flash_read(&mut self, a: u32, n: u64) {
        if a >= 0x1e0000 {
            self.pcm_reads += n;
        } else if a >= 0xe0000 {
            self.usr_reads += n;
        }
    }
    pub fn turn_encoder(&mut self, detents: i32) {
        for _ in 0..detents.unsigned_abs() {
            for v in if detents > 0 {
                [1, 0, 2, 3]
            } else {
                [2, 0, 1, 3]
            } {
                self.encoder_transitions.push_back(v);
            }
        }
    }
    fn serial_action(&mut self, chip: usize, action: impl FnOnce(&mut C55)) {
        let d = &mut self.dsp[chip];
        if !d.started || !d.fault.is_empty() {
            return;
        }
        if let Err(error) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(d))) {
            d.fault = error
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "serial device panic".into());
        }
    }
    fn idle_adc_sample(&mut self) -> u32 {
        if !self.adc_idle_noise {
            return 0;
        }
        let mut sum = 0i32;
        for _ in 0..6 {
            self.adc_noise_state ^= self.adc_noise_state << 13;
            self.adc_noise_state ^= self.adc_noise_state >> 17;
            self.adc_noise_state ^= self.adc_noise_state << 5;
            sum += (self.adc_noise_state & 255) as i32;
        }
        ((sum - 765) / 4 * 256) as u32
    }
    fn audio_frame(&mut self) {
        self.audio_frames += 1;
        for chip in 0..2 {
            self.serial_action(chip, |d| d.serial_frame(0));
        }
        let input = if let Some(source) = self.adc_input.as_mut() {
            source()
        } else {
            [self.idle_adc_sample(), self.idle_adc_sample()]
        };
        self.update_codec_pins();
        let input = self.codec.frame(input);
        for sample in input {
            self.serial_action(0, |d| d.serial_receive(0, 0));
            self.serial_action(1, |d| d.serial_receive(0, sample));
        }
    }
    fn dsp_serial_frame(&mut self) {
        self.dsp_serial_frames += 1;
        for chip in 0..2 {
            self.serial_action(chip, |d| {
                d.serial_frame(1);
                d.serial_frame(2);
            });
        }
        let mut master_to_fxd = 0;
        let mut slave_to_master = 0;
        let mut transmitted = false;
        self.serial_action(0, |d| {
            if let Some(v) = d.serial_transmit(1) {
                master_to_fxd = v;
                transmitted = true;
            }
        });
        if transmitted {
            self.serial_words += 1;
            if let Some(observer) = self.serial_observer.as_mut() {
                observer(0, 1, self.dsp_serial_frames, master_to_fxd);
            }
        }
        let zero = self.fxd_return_zero;
        transmitted = false;
        self.serial_action(1, |d| {
            d.serial_receive(1, if zero { 0 } else { master_to_fxd });
            if let Some(v) = d.serial_transmit(2) {
                slave_to_master = v;
                transmitted = true;
            }
        });
        if transmitted {
            self.serial_words += 1;
            if let Some(observer) = self.serial_observer.as_mut() {
                observer(1, 2, self.dsp_serial_frames, slave_to_master);
            }
        }
        self.serial_action(0, |d| d.serial_receive(2, slave_to_master));
    }
}
impl Bus for Board {
    fn read16(&mut self, a: u32) -> u16 {
        let p = Self::physical(a);
        if HostWindow::selected(p) && p & 1 == 0 {
            let v = self.fxd_host.read_word(p);
            self.fxd_host.word_reads += 1;
            self.observe_fxd(p, (v >> 8) as u8, false);
            self.observe_fxd(p + 1, v as u8, false);
            return v;
        }
        if NorFlash::selected(p) && p & 1 == 0 {
            let byte = p & MASK;
            self.count_flash_read(byte, 2);
            if self.nor.array_read_mode(byte) {
                return (self.flash[byte as usize] as u16) << 8
                    | self.flash[byte as usize + 1] as u16;
            }
            return self.nor.read_word(byte, &self.flash);
        }
        let high = self.read8(a) as u16;
        high << 8 | self.read8(a.wrapping_add(1)) as u16
    }
    fn write16(&mut self, a: u32, v: u16) {
        let p = Self::physical(a);
        if HostWindow::selected(p) && p & 1 == 0 {
            self.fxd_host.write_word(p, v);
            self.fxd_upload.write_word(HostWindow::offset(p), v);
            self.fxd_host.word_writes += 1;
            self.observe_fxd(p, (v >> 8) as u8, true);
            self.observe_fxd(p + 1, v as u8, true);
            return;
        }
        if NorFlash::selected(p) && p & 1 == 0 {
            self.log(true, a, (v >> 8) as u32);
            self.log(true, a + 1, (v & 255) as u32);
            self.nor.write_word(p, v);
            return;
        }
        self.write8(a, (v >> 8) as u8);
        self.write8(a.wrapping_add(1), v as u8);
    }
    fn read8(&mut self, address: u32) -> u8 {
        let p = Self::physical(address);
        if NorFlash::selected(p) {
            let byte = p & MASK;
            self.count_flash_read(byte, 1);
            if self.nor.array_read_mode(byte) {
                return self.flash[byte as usize];
            }
            let word = self.nor.read_word(byte, &self.flash);
            return if byte & 1 != 0 {
                word as u8
            } else {
                (word >> 8) as u8
            };
        }
        if (0x0c000000..0x10000000).contains(&p) {
            return self.ram[((p - 0x0c000000) & 0xffffff) as usize];
        }
        if HostWindow::selected(p) {
            let v = self.fxd_host.read(p);
            self.fxd_host.byte_reads += 1;
            self.observe_fxd(p, v, false);
            return v;
        }
        self.io_reads += 1;
        if p == 0x14000000 {
            return 0;
        }
        if p == 0x14000001 {
            return if self.lcd.column < 132 {
                self.lcd.data[(self.lcd.page * 132 + self.lcd.column) as usize]
            } else {
                0
            };
        }
        if (0x18000000..0x1a000000).contains(&p) {
            self.dsp_reads += 1;
            let v = self.dsp[(p >> 24 & 1) as usize].host_read(p & 7);
            self.log(false, address, v as u32);
            return v;
        }
        let a = Self::register_address(address);
        let value = if (0x08000000..0x08000100).contains(&p) {
            self.regs.read(0xa8000000 + (p & 255))
        } else if (0xa4000080..=0xa400008e).contains(&a) && a & 1 == 0 {
            if a & 2 == 0 {
                self.adc_temp = self.regs.get(a + 2);
                self.regs.get(a)
            } else {
                self.adc_temp
            }
        } else if (0xa4000120..=0xa4000136).contains(&a) && a & 1 == 0 {
            self.gpio(a)
        } else if (0xa4000150..=0xa400015f).contains(&a) {
            self.scif.read(a - 0xa4000150)
        } else if a == 0xfffffe88 {
            0x84
        } else if a >= 0xfffff000 || (0xa4000000..0xa4000200).contains(&a) {
            self.regs.read(a)
        } else {
            *self.unknown_reads.entry(address).or_default() += 1;
            if self.strict {
                std::panic::panic_any("Unknown memory-mapped device read".to_string());
            }
            255
        };
        self.log(false, address, value as u32);
        value
    }
    fn write8(&mut self, address: u32, value: u8) {
        let p = Self::physical(address);
        if HostWindow::selected(p) {
            self.fxd_upload.write(HostWindow::offset(p), value);
            self.fxd_host.write(p, value);
            self.fxd_host.byte_writes += 1;
            self.observe_fxd(p, value, true);
            return;
        }
        if NorFlash::selected(p) {
            self.log(true, address, value as u32);
            if p & 1 != 0 {
                self.nor.write_word(p, 0xff00 | value as u16);
            }
            return;
        }
        if (0x0c000000..0x10000000).contains(&p) {
            self.ram[((p - 0x0c000000) & 0xffffff) as usize] = value;
            return;
        }
        self.io_writes += 1;
        self.log(true, address, value as u32);
        if p == 0x14000000 {
            self.lcd.command(value);
            return;
        }
        if p == 0x14000001 {
            self.lcd.write(value);
            return;
        }
        if (0x18000000..0x1a000000).contains(&p) {
            if self.dsp_upload.len() < 8 * 1024 * 1024 {
                self.dsp_upload.push(value);
            }
            let chip = (p >> 24 & 1) as usize;
            let offset = p & 7;
            self.dsp[chip].host_pc = self.current_pc;
            self.dsp[chip].host_write(offset, value);
            if let Some(observer) = self.dsp_upload_observer.as_mut() {
                observer(chip as u32, offset, value);
            }
            return;
        }
        if (0x08000000..0x08000100).contains(&p) {
            let usb = 0xa8000000 + (p & 255);
            if (0x18..0x20).contains(&(p & 255)) {
                let v = self.regs.get(usb) & value;
                self.regs.set(usb, v);
            } else {
                self.regs.set(usb, value);
            }
            return;
        }
        let a = Self::register_address(address);
        if (0xa4000150..=0xa400015f).contains(&a) {
            self.scif.write(a - 0xa4000150, value);
            return;
        }
        self.regs.set(a, value);
        if matches!(a, 0xa4000116 | 0xa4000117 | 0xa4000136) {
            self.update_fxd_control_lines();
        }
        if matches!(
            a,
            0xa4000116 | 0xa4000117 | 0xa4000136 | 0xa4000108 | 0xa4000109 | 0xa4000128
        ) {
            self.update_codec_pins();
        }
        if matches!(a, 0xa4000108 | 0xa4000109 | 0xa4000128) {
            let asserted = self.gpio(0xa4000128) & 0x40 == 0;
            if asserted != self.dsp_reset_asserted {
                self.dsp_reset_asserted = asserted;
                for d in &mut self.dsp {
                    d.set_reset(asserted);
                }
            }
        }
        if self.midi_out.len() > 4096 {
            self.midi_out.pop_front();
        }
        if a == 0xffffffe3 && value & 1 != 0 {
            std::panic::panic_any("MMU enabled: translation not implemented yet".to_string());
        }
        if a == 0xa4000090 && value & 0x20 != 0 {
            let selected = (value & 7) as u32;
            let first = if value & 0x10 != 0 {
                selected & 4
            } else {
                selected
            };
            let mux = self.regs.get(0xa4000128) >> 1 & 7;
            for channel in first..=selected {
                let sample = (if channel < 5 {
                    self.panel_adc[channel as usize][mux as usize]
                } else {
                    512
                }) << 6;
                let data = 0xa4000080 + 4 * (channel & 3);
                self.regs.set(data, (sample >> 8) as u8);
                self.regs.set(data + 1, 0);
                self.regs.set(data + 2, sample as u8);
                self.regs.set(data + 3, 0);
            }
            self.regs.set(a, (value | 0x80) & !0x20);
        }
    }
    fn tick(&mut self, cycles: u32) {
        self.ticks += cycles as u64;
        self.nor.advance(cycles, &mut self.flash);
        self.fxd_host.advance(cycles);
        if !self.encoder_transitions.is_empty() {
            self.encoder_clock_phase += cycles as u64;
            while self.encoder_clock_phase >= 200000 && !self.encoder_transitions.is_empty() {
                self.encoder_clock_phase -= 200000;
                self.encoder_bits = self.encoder_transitions.pop_front().unwrap();
            }
        } else {
            self.encoder_clock_phase = 0;
        }
        let before = self.midi_out.len();
        self.scif
            .tick(cycles, &mut self.midi_in, &mut self.midi_out);
        if let Some(observer) = self.midi_observer.as_mut() {
            for &v in self.midi_out.iter().skip(before) {
                observer(self.ticks, v);
            }
        }
        while self.midi_out.len() > 4096 {
            self.midi_out.pop_front();
        }
        self.dsp_core_clock_phase += cycles as u64 * 25;
        let clocks = self.dsp_core_clock_phase / 12;
        self.dsp_core_clock_phase %= 12;
        for chip in 0..2 {
            let pins = 0xc5
                | if self.gpio(0xa4000126) & if chip != 0 { 0x20 } else { 0x80 } != 0 {
                    0x10
                } else {
                    0
                };
            self.dsp[chip].gpio_pins = pins;
            let d = &mut self.dsp[chip];
            if d.started && d.fault.is_empty() {
                d.run(clocks as u32);
            }
        }
        self.audio_clock_phase += cycles as u64 * SAMPLE_HZ as u64;
        while self.audio_clock_phase >= CPU_HZ {
            self.audio_clock_phase -= CPU_HZ;
            self.audio_frame();
        }
        self.dsp_serial_clock_phase +=
            cycles as u64 * SAMPLE_HZ as u64 * BUS_WORDS_PER_SAMPLE as u64;
        while self.dsp_serial_clock_phase >= CPU_HZ {
            self.dsp_serial_clock_phase -= CPU_HZ;
            self.dsp_serial_frame();
        }
        for i in 0..3 {
            if self.regs.get(0xfffffe92) & (1 << i) == 0 {
                continue;
            }
            let control = 0xfffffe9c + 12 * i as u32;
            let prescale = self.regs.read16(control) & 7;
            if prescale > 4 {
                continue;
            }
            let divisor = [24u64, 96, 384, 1536, 6144][prescale as usize];
            self.timer_phase[i] += cycles as u64;
            let mut elapsed = self.timer_phase[i] / divisor;
            self.timer_phase[i] %= divisor;
            if elapsed == 0 {
                continue;
            }
            let counter_address = 0xfffffe98 + 12 * i as u32;
            let constant = self.regs.read32(0xfffffe94 + 12 * i as u32);
            let mut counter = self.regs.read32(counter_address);
            while elapsed > counter as u64 {
                elapsed -= counter as u64 + 1;
                counter = constant;
                self.regs
                    .write16(control, self.regs.read16(control) | 0x100);
            }
            self.regs
                .write32(counter_address, counter.wrapping_sub(elapsed as u32));
        }
    }
    fn interrupt(&self) -> Interrupt {
        let ipra = self.regs.read16(0xfffffee2);
        let mut result = Interrupt::default();
        for i in 0..3 {
            if self.regs.read16(0xfffffe9c + 12 * i) & 0x120 != 0x120 {
                continue;
            }
            let level = (ipra as u32 >> (12 - 4 * i)) & 15;
            if level > result.level {
                result = Interrupt {
                    level,
                    event: 0x400 + 0x20 * i,
                    event2: 0,
                };
            }
        }
        let event = self.scif.interrupt_event();
        if event != 0 {
            let level = (self.regs.read16(0xa400001a) as u32 >> 4) & 15;
            if level > result.level {
                result = Interrupt {
                    level,
                    event: 0x200 + 0x20 * (15 - level),
                    event2: event,
                };
            }
        }
        result
    }
    fn event(&mut self, code: u32, irq: bool) {
        self.regs
            .write32(if irq { 0xffffffd8 } else { 0xffffffd4 }, code);
        if irq {
            self.regs.write32(0xa4000000, code);
        }
    }
    fn event2(&mut self, code: u32) {
        self.regs.write32(0xa4000000, code);
    }
}
