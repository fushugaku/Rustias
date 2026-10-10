//! Native DSP parameter-data adapter for the timbre-output application port.
//! It executes data use cases, without a DSP CPU or firmware interpreter.
use radias_synth_application::{
    dsp_receiver::{ParameterMemory, ReceiveOutcome, receive_memory_command},
    timbre_output::TimbreOutputPort,
};
use radias_synth_domain::{dsp_control::DspEndpoint, timbre_output::TimbreOutputPlan};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DspParameterMemory {
    words: Vec<u16>,
}
impl DspParameterMemory {
    pub fn from_words(words: Vec<u16>) -> Result<Self, &'static str> {
        if words.len() != 65536 {
            return Err("DSP parameter data requires 65536 word addresses");
        }
        Ok(Self { words })
    }
    pub fn words(&self) -> &[u16] {
        &self.words
    }
    pub fn words_mut(&mut self) -> &mut [u16] {
        &mut self.words
    }
}
impl ParameterMemory for DspParameterMemory {
    fn read_word(&self, address: u16) -> u16 {
        self.words[usize::from(address)]
    }
    fn write_word(&mut self, address: u16, value: u16) {
        self.words[usize::from(address)] = value;
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeTimbreOutputPort {
    memories: [DspParameterMemory; 2],
    packets: Vec<(DspEndpoint, Vec<u16>)>,
}
impl NativeTimbreOutputPort {
    pub fn new(memories: [DspParameterMemory; 2]) -> Self {
        Self {
            memories,
            packets: Vec::new(),
        }
    }
    pub fn memories(&self) -> &[DspParameterMemory; 2] {
        &self.memories
    }
    pub fn packets(&self) -> &[(DspEndpoint, Vec<u16>)] {
        &self.packets
    }
    pub fn clear_packets(&mut self) {
        self.packets.clear();
    }
}
impl TimbreOutputPort for NativeTimbreOutputPort {
    type Error = &'static str;
    fn accept_timbre_output(&mut self, plan: &TimbreOutputPlan) -> Result<(), Self::Error> {
        let mut memories = self.memories.clone();
        let mut packets = Vec::new();
        for command in plan.commands() {
            let chip = usize::from(command.endpoint == DspEndpoint::Slave);
            let words = command.packet.words();
            for (i, &value) in words.iter().enumerate() {
                memories[chip].words_mut()[0x100 + i] = value;
            }
            if receive_memory_command(&mut memories[chip], 0x100) != ReceiveOutcome::Ready {
                return Err("Native timbre output parameter receiver rejected command");
            }
            packets.push((command.endpoint, words.to_vec()));
        }
        self.memories = memories;
        self.packets.extend(packets);
        Ok(())
    }
}
