//! Stored parameter-template binding, original SYS00622a/006928/01fcb8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterTemplateAddresses {
    pub drums: [u16; 16],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorTemplateBinding {
    pub program_kind: u8,
    pub timbre_id: u8,
    pub drum_instrument: u8,
    /// Ordinary timbre's compiled source bank, context word34. Template
    /// construction is separate from choosing/copying this existing bank.
    pub ordinary_address: u16,
}
impl ActorTemplateBinding {
    pub fn uses_drum(self) -> bool {
        (self.program_kind >> 5) == (self.timbre_id & 3) + 1
    }
    pub fn source(self, addresses: &ParameterTemplateAddresses) -> u16 {
        if self.uses_drum() {
            addresses.drums[usize::from(self.drum_instrument & 15)]
        } else {
            self.ordinary_address
        }
    }
    /// Complete caller work before entering ED3c, measured in the functional
    /// SH instruction paths. Drum lookup adds four clocks to the ordinary path.
    pub fn sender_gap(self) -> u16 {
        if self.uses_drum() { 61 } else { 57 }
    }
    pub const RETURN_GAP: u16 = 6;
}
