//! Physical phase tables and working-coefficient priming, SYS01e684/01fe98.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicalPhaseWord {
    pub offset: u16,
    pub value: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicalPhaseInitialization {
    pub words: [PhysicalPhaseWord; 2],
    pub count: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalPhaseTables {
    pub entries: [PhysicalPhaseInitialization; 24],
}
impl PhysicalPhaseTables {
    pub fn get(&self, selection: u8) -> Option<&PhysicalPhaseInitialization> {
        self.entries
            .get(usize::from(selection & 15) * 4 + usize::from((selection >> 4) & 3))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoefficientPriming {
    pub routing: u8,
    pub shaper: u8,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PhaseCallback {
    #[default]
    None,
    CachedWord,
    Unison,
    UnisonTriangle,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseCallbackTables {
    pub phases: [PhaseCallback; 24],
    pub counters: [bool; 24],
}
impl PhaseCallbackTables {
    pub fn index(selection: u8) -> usize {
        usize::from(selection & 15) * 4 + usize::from((selection >> 4) & 3)
    }
}
impl CoefficientPriming {
    pub fn parallel(self) -> bool {
        self.routing & 3 == 2
    }
    pub fn pickup(self) -> bool {
        self.parallel() && self.shaper & 15 == 9
    }
    pub fn first_sender_gap(self) -> u16 {
        if self.pickup() {
            31
        } else if self.parallel() {
            33
        } else {
            28
        }
    }
}
