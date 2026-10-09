//! Original AMP parameter packet state, SYS002926/002978/002a6c.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmplifierRateTable {
    pub attack_rates: [u16; 4],
    pub soft_binding_rate: u16,
    pub regular_rate: u16,
    pub termination_rate: u16,
    pub reset_rate: u16,
}
impl AmplifierRateTable {
    /// SYS014724/0029dc share the unscaled attack increment table. The first
    /// three thresholds are its first three entries; later indices clip to3.
    pub fn attack_rate(&self, attack: u8, modulation: i16) -> u16 {
        let time = (i32::from(attack & 127) + i32::from(modulation)).clamp(0, 127);
        self.attack_rates[time.min(3) as usize]
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmplifierPacket {
    RateAndTarget { rate: u16, target: i16 },
    Target(i16),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AmplifierDelivery {
    /// The original signed byte at controller slot1e5. It is independent of
    /// the ADSR stage, the sample smoother and the allocation flag bank.
    pub mode: u8,
}
impl AmplifierDelivery {
    /// SYS015ca4/015ddc consume the envelope's zero/release flag (bit3 in
    /// the controller service bank) before publishing AMP: slot1e5 becomes
    /// signed -1. This is separate from the subsequent release acknowledgement.
    pub fn release_zero(&mut self) {
        self.mode = u8::MAX;
    }
    pub fn binding(
        &mut self,
        tables: &AmplifierRateTable,
        attack: u8,
        modulation: i16,
        target: i16,
        soft: bool,
    ) -> AmplifierPacket {
        self.mode = 0;
        let selected = tables.attack_rate(attack, modulation);
        let rate =
            if soft && (selected == tables.attack_rates[0] || selected == tables.attack_rates[1]) {
                tables.soft_binding_rate
            } else {
                selected
            };
        AmplifierPacket::RateAndTarget { rate, target }
    }
    pub fn service(&mut self, tables: &AmplifierRateTable, target: i16) -> AmplifierPacket {
        if self.mode == 0 {
            self.mode = 1;
            AmplifierPacket::RateAndTarget {
                rate: tables.regular_rate,
                target,
            }
        } else if (self.mode as i8) < 0 {
            AmplifierPacket::RateAndTarget {
                rate: tables.termination_rate,
                target: 0,
            }
        } else {
            if self.mode < 2 {
                self.mode += 1;
            }
            AmplifierPacket::Target(target)
        }
    }
    pub fn reset(tables: &AmplifierRateTable) -> AmplifierPacket {
        AmplifierPacket::RateAndTarget {
            rate: tables.reset_rate,
            target: 0,
        }
    }
}
