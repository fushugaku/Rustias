//! Exchanges the original fixed four-frame working block with native processors.
use radias_synth_domain::{
    Sample,
    dsp_audio_exchange::{
        self, ADC_WORDS, BLOCK_WORDS, DOUBLE_BUFFER_WORDS, FRAMES_PER_BLOCK, SAMPLES_PER_FRAME,
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameIndexError;

#[derive(Clone)]
pub struct DspAudioExchange {
    working: [u16; BLOCK_WORDS],
    trailing_sample: Sample,
}

impl Default for DspAudioExchange {
    fn default() -> Self {
        Self {
            working: [0; BLOCK_WORDS],
            trailing_sample: Sample(0),
        }
    }
}

impl DspAudioExchange {
    pub fn receive(&mut self, flag: u16, adc: &[u16; ADC_WORDS], bus: &[u16; DOUBLE_BUFFER_WORDS]) {
        dsp_audio_exchange::receive(flag, adc, bus, &mut self.working);
        // Word 04E3 and its XOR-1 partner immediately follow the working
        // block. This is always the first ADC pair, regardless of half flag.
        self.trailing_sample = Sample(((u32::from(adc[1]) << 16) | u32::from(adc[0])) as i32);
    }

    /// Original working DWORDs are addressed at odd words 0463+32*frame.
    pub fn frame(&self, index: usize) -> Result<[Sample; SAMPLES_PER_FRAME], FrameIndexError> {
        if index >= FRAMES_PER_BLOCK {
            return Err(FrameIndexError);
        }
        Ok(core::array::from_fn(|sample| {
            let offset = (index * SAMPLES_PER_FRAME + sample) * 2;
            Sample(
                ((u32::from(self.working[offset + 1]) << 16) | u32::from(self.working[offset]))
                    as i32,
            )
        }))
    }

    pub fn replace_frame(
        &mut self,
        index: usize,
        samples: [Sample; SAMPLES_PER_FRAME],
    ) -> Result<(), FrameIndexError> {
        if index >= FRAMES_PER_BLOCK {
            return Err(FrameIndexError);
        }
        for (sample, value) in samples.iter().enumerate() {
            let offset = (index * SAMPLES_PER_FRAME + sample) * 2;
            self.working[offset] = value.0 as u16;
            self.working[offset + 1] = (value.0 as u32 >> 16) as u16;
        }
        Ok(())
    }

    pub fn transmit(&self, flag: u16, bus: &mut [u16; DOUBLE_BUFFER_WORDS]) {
        dsp_audio_exchange::transmit(flag, &self.working, bus);
    }

    pub fn working_words(&self) -> &[u16; BLOCK_WORDS] {
        &self.working
    }

    pub fn vocoder_frame(
        &self,
        index: usize,
    ) -> Result<radias_synth_domain::vocoder::VocoderFrame, FrameIndexError> {
        let current = self.frame(index)?;
        let following = if index + 1 < FRAMES_PER_BLOCK {
            self.frame(index + 1)?[0]
        } else {
            self.trailing_sample
        };
        Ok(radias_synth_domain::vocoder::VocoderFrame {
            // The original sample job stores 0462+32*frame in word4001.
            // Its even-addressed DWORD view reverses the odd copy-loop view.
            samples: core::array::from_fn(|sample| {
                let value = if sample < SAMPLES_PER_FRAME {
                    current[sample].0
                } else {
                    following.0
                };
                value.rotate_left(16)
            }),
        })
    }

    /// Retain routed stores that cross into the next frame or adjacent ADC
    /// pair. The caller owns publication of that adjacent sample to its port.
    pub fn replace_vocoder_frame(
        &mut self,
        index: usize,
        frame: &radias_synth_domain::vocoder::VocoderFrame,
    ) -> Result<(), FrameIndexError> {
        self.replace_frame(
            index,
            core::array::from_fn(|sample| Sample(frame.samples[sample].rotate_left(16))),
        )?;
        let following = Sample(frame.samples[SAMPLES_PER_FRAME].rotate_left(16));
        if index + 1 < FRAMES_PER_BLOCK {
            let at = (index + 1) * SAMPLES_PER_FRAME * 2;
            self.working[at] = following.0 as u16;
            self.working[at + 1] = (following.0 as u32 >> 16) as u16;
        } else {
            self.trailing_sample = following;
        }
        Ok(())
    }

    pub fn trailing_sample(&self) -> Sample {
        self.trailing_sample
    }
}
