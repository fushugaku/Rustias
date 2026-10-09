//! OSC1 parameter construction, SYS0202e0 and immutable041608/0425d2 tables.
//! The ROM supplies algorithm descriptors and constants, never captured voices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimaryInitializationWord {
    pub offset: u16,
    pub value: u16,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimaryInitialization {
    pub generator: u16,
    pub constants: [PrimaryInitializationWord; 13],
    pub count: u8,
}
impl PrimaryInitialization {
    pub fn words(&self) -> &[PrimaryInitializationWord] {
        &self.constants[..usize::from(self.count)]
    }
    /// Caller instruction clocks in the existing functional SH reference.
    /// First wrapper42, template setup55, each subsequent row35, epilogue20.
    /// Readiness stalls belong to the shared sender, not a fitted audio delay.
    pub const FIRST_SENDER_GAP: u16 = 42;
    pub const TEMPLATE_SETUP_GAP: u16 = 55;
    pub const NEXT_CONSTANT_GAP: u16 = 35;
    pub const CALLER_RETURN_GAP: u16 = 20;
    /// A null template skips the iterator and takes the direct return branch.
    pub const NO_CONSTANTS_RETURN_GAP: u16 = 32;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryInitializationTables {
    pub entries: [PrimaryInitialization; 24],
}
impl PrimaryInitializationTables {
    pub fn get(&self, selection: u8) -> Option<&PrimaryInitialization> {
        let index = usize::from(selection & 15) * 4 + usize::from((selection >> 4) & 3);
        self.entries.get(index)
    }
}
