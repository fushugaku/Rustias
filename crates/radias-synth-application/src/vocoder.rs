//! Continuous vocoder use case; device buffers and firmware data stay in adapters.
use radias_synth_domain::{
    pan::StereoFrame,
    vocoder::{InterpolationTables, Vocoder, VocoderError, VocoderFrame},
};
pub struct VocoderRenderer {
    pub processor: Vocoder,
    pub tables: InterpolationTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VocoderBlockError {
    LengthMismatch,
    Sample { frame: usize, error: VocoderError },
}
impl VocoderRenderer {
    /// Reconstruct the original even-addressed vocoder view inside the copy
    /// loop's odd-addressed block. Scheduling remains owned by the caller.
    pub fn render_exchange_frame(
        &mut self,
        exchange: &mut crate::dsp_audio_exchange::DspAudioExchange,
        index: usize,
        interpolate: bool,
    ) -> Result<StereoFrame, VocoderBlockError> {
        let mut frame = exchange
            .vocoder_frame(index)
            .map_err(|_| VocoderBlockError::Sample {
                frame: index,
                error: VocoderError::FrameRoute,
            })?;
        let output = self
            .processor
            .process(&mut frame, interpolate, &self.tables)
            .map_err(|error| VocoderBlockError::Sample {
                frame: index,
                error,
            })?;
        exchange
            .replace_vocoder_frame(index, &frame)
            .map_err(|_| VocoderBlockError::Sample {
                frame: index,
                error: VocoderError::FrameRoute,
            })?;
        Ok(output)
    }

    pub fn publish_sources(
        &mut self,
        program: radias_synth_domain::vocoder_control::VocoderProgram<'_>,
        sources: radias_synth_domain::vocoder_sources::VocoderSources,
        controls: &radias_synth_domain::vocoder_control::VocoderControlTables<'_>,
    ) {
        self.publish_frequency_source(program, sources.read(program.bytes[40]), controls);
    }
    pub fn advance_formant(
        &mut self,
        playback: &mut radias_synth_domain::formant_motion::FormantPlayback,
        record: radias_synth_domain::formant_motion::MotionRecord<'_>,
    ) {
        self.processor.publish_formant(playback.advance(record));
    }
    pub fn publish_frequency_source(
        &mut self,
        program: radias_synth_domain::vocoder_control::VocoderProgram<'_>,
        source: i16,
        controls: &radias_synth_domain::vocoder_control::VocoderControlTables<'_>,
    ) {
        self.processor.parameters[0xbf] = program.frequency_offset(source, controls) as u16;
    }
    /// Compile fresh stored patch data through the native control algorithms.
    /// The selected modulation callback's current value remains an explicit
    /// input port; Formant Motion scheduling belongs to its playback use case.
    pub fn from_program(
        program: radias_synth_domain::vocoder_control::VocoderProgram<'_>,
        inputs: radias_synth_domain::vocoder_control::VocoderControlInputs,
        controls: &radias_synth_domain::vocoder_control::VocoderControlTables<'_>,
        tables: InterpolationTables,
    ) -> Result<Self, radias_synth_domain::vocoder_control::VocoderControlError> {
        let mut processor = Vocoder {
            parameters: program.compile_targets(inputs, controls)?,
            state: [0; 300],
        };
        if program.bytes[44] & 127 == 127 {
            for band in 0..16 {
                let at = 46 + 2 * band;
                processor.state[0x94 + band] =
                    u16::from_le_bytes(program.bytes[at..at + 2].try_into().unwrap());
            }
        }
        processor.initialize();
        Ok(Self { processor, tables })
    }
    pub fn render_buses(
        &mut self,
        buses: [StereoFrame; 4],
        input: StereoFrame,
        interpolate: bool,
    ) -> Result<[StereoFrame; 4], VocoderError> {
        self.render_sources(buses, [input, StereoFrame::default()], interpolate)
    }
    pub fn render_sources(
        &mut self,
        buses: [StereoFrame; 4],
        inputs: [StereoFrame; 2],
        interpolate: bool,
    ) -> Result<[StereoFrame; 4], VocoderError> {
        let mut frame = VocoderFrame::from_sources(buses, inputs);
        self.processor
            .process(&mut frame, interpolate, &self.tables)?;
        Ok(frame.buses())
    }
    /// Boundary changes never reset histories or parameter smoothing. Validate
    /// extents before advancing; an invalid sample stops at its actual frame.
    pub fn render_block(
        &mut self,
        frames: &mut [VocoderFrame],
        interpolate: &[bool],
        output: &mut [StereoFrame],
    ) -> Result<(), VocoderBlockError> {
        if frames.len() != interpolate.len() || frames.len() != output.len() {
            return Err(VocoderBlockError::LengthMismatch);
        }
        for (index, ((frame, update), destination)) in
            frames.iter_mut().zip(interpolate).zip(output).enumerate()
        {
            *destination = self
                .processor
                .process(frame, *update, &self.tables)
                .map_err(|error| VocoderBlockError::Sample {
                    frame: index,
                    error,
                })?;
        }
        Ok(())
    }
}
