//! SYS 2.00 Slave output and 24-bit serial ingress into Master (A036..A03e).
//! Twelve frames is the observed DMA/serial latency of the reference machine;
//! electrical clock/pipeline timing has not been measured on original hardware.
use crate::{Sample, fixed::saturate, pan::StereoFrame};

pub const LINK_FRAMES: usize = 12;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessorLink {
    frames: [[StereoFrame; 4]; LINK_FRAMES],
    position: usize,
}
impl Default for ProcessorLink {
    fn default() -> Self {
        Self {
            frames: [[StereoFrame::default(); 4]; LINK_FRAMES],
            position: 0,
        }
    }
}
impl ProcessorLink {
    /// Slave final shift is four, Master final shift is five. McBSP transports
    /// the top 24 bits; A03b then shifts the signed received word right by four.
    pub fn ingress(sample: Sample) -> Sample {
        Sample((sample.0 & !255) >> 4)
    }
    pub fn advance(&mut self, slave: [StereoFrame; 4]) -> [StereoFrame; 4] {
        let previous = self.frames[self.position];
        self.frames[self.position] = slave.map(|s| StereoFrame {
            left: Self::ingress(s.left),
            right: Self::ingress(s.right),
        });
        self.position = (self.position + 1) % LINK_FRAMES;
        previous
    }
}
pub fn scale_slave(raw: StereoFrame) -> StereoFrame {
    StereoFrame {
        left: Sample(saturate((raw.left.0 as i64) << 4)),
        right: Sample(saturate((raw.right.0 as i64) << 4)),
    }
}
