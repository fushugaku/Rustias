//! Whole initial filter controller caches, SYS01b648/01b8be/01bc12/01bd62.
use crate::{actor_control_state::ActorControlState, virtual_patch_live::LiveCompilerTables};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterPreparation {
    FirstKey,
    FirstFrequency,
    SecondKey,
    SecondFrequency,
}
impl ActorControlState {
    /// SYS01b710 / SYS01bcc6: linked filter2 uses filter1's velocity intensity.
    pub fn prepare_filter_velocity(&mut self, body: &[u8; 104], second: bool) -> u16 {
        let link = self.bytes[0x1e2] & 128 != 0;
        let parameter = if second && !link { 44 } else { 39 };
        let depth = i32::from(body[parameter] & 127) - 64;
        let value = (depth * i32::from(self.bytes[0x37] & 127) * 4).clamp(-32767, 32767);
        self.set_word(if second { 0x10a } else { 0x108 }, value as i16);
        // The original MULS.W has two functional SH clocks.
        let work = if depth == 0 { 18 } else { 37 };
        work + if second { 5 + u16::from(!link) } else { 0 }
    }

    /// Whole initial resonance arithmetic, without any live DSP publisher.
    pub fn prepare_filter_resonance(
        &mut self,
        body: &[u8; 104],
        second: bool,
        tables: &LiveCompilerTables<'_>,
    ) -> u16 {
        if second {
            self.compile_live_filter2_resonance(body, tables.resonance, tables.comb);
            crate::virtual_patch_live_work::resonance_work(self, body)
        } else {
            self.compile_live_filter1_resonance(body, tables.resonance);
            47 + u16::from(self.bytes[0x1e2] & 0x83 != 0x81)
        }
    }

    /// Whole SYS01c05c. Comb frequency already computes resonance; the final
    /// explicit resonance call is still performed, preserving source order/work.
    pub fn initialize_note_filters(
        &mut self,
        body: &[u8; 104],
        tables: &LiveCompilerTables<'_>,
    ) -> u16 {
        let mut work = 33;
        work += self.prepare_filter_control(FilterPreparation::FirstKey, body, tables);
        work += self.prepare_filter_velocity(body, false);
        work += self.prepare_filter_control(FilterPreparation::FirstFrequency, body, tables);
        work += self.prepare_filter_control(FilterPreparation::SecondKey, body, tables);
        work += self.prepare_filter_velocity(body, true);
        work += self.prepare_filter_control(FilterPreparation::SecondFrequency, body, tables);
        work += self.prepare_filter_resonance(body, false, tables);
        work += self.prepare_filter_resonance(body, true, tables);
        work
    }

    pub fn prepare_filter_control(
        &mut self,
        service: FilterPreparation,
        body: &[u8; 104],
        tables: &LiveCompilerTables<'_>,
    ) -> u16 {
        use crate::virtual_patch_live_work::{
            comb_lookup, envelope_level, filter1_code, frequency, key_work, resonance_work,
        };
        match service {
            FilterPreparation::FirstKey => {
                self.compile_live_filter1_key_tracking(body, tables.frequency);
                // The initial entry omits three live-compiler wrapper clocks.
                key_work(self, body, false, tables) - 3
            }
            FilterPreparation::FirstFrequency => {
                self.compile_live_filter1_frequency(body, tables.frequency, tables.amplifier);
                72 + envelope_level(body[57]) + frequency(filter1_code(self, body, tables))
            }
            FilterPreparation::SecondKey => {
                self.compile_live_filter2_key_tracking(body, tables.frequency);
                key_work(self, body, true, tables)
            }
            FilterPreparation::SecondFrequency => {
                self.compile_live_filter2_frequency(
                    body,
                    tables.frequency,
                    tables.amplifier,
                    tables.resonance,
                    tables.comb,
                );
                let unlinked = u16::from(self.bytes[0x1e2] & 128 == 0);
                let code = i32::from_be_bytes(self.bytes[0xc4..0xc8].try_into().unwrap());
                if self.bytes[0x1e2] & 0x30 == 0x30 {
                    97 + 2 * unlinked
                        + envelope_level(body[57])
                        + comb_lookup(code, true)
                        + resonance_work(self, body)
                } else {
                    89 + 2 * unlinked + envelope_level(body[57]) + frequency(code)
                }
            }
        }
    }
}
