//! SYS01fcf0 chooses a word sender from the oscillator descriptor. The low
//! nibble selects the waveform; bits4/5 select Waveform/Cross/Unison/VPM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryPitchSendTable {
    pub senders: [u8; 24],
}
impl PrimaryPitchSendTable {
    pub fn sender(self, selection: u8) -> Option<u8> {
        let index = usize::from(selection & 15) * 4 + usize::from((selection >> 4) & 3);
        self.senders.get(index).copied()
    }
}
