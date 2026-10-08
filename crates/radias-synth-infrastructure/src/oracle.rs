//! A test adapter for input/output observations of original DSP transfers.
use radias_synth_domain::filter::{FilterCoefficients, FilterState};
use radias_synth_domain::{
    Sample,
    waveform::{ShapeParameters, Transfer, WaveformFrame},
};

pub struct FilterRecord {
    pub input: Sample,
    pub coefficients: FilterCoefficients,
    pub initial: FilterState,
    pub output: Sample,
    pub final_state: FilterState,
}

pub fn decode_filter_records(bytes: &[u8]) -> Result<Vec<FilterRecord>, &'static str> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(156) {
        return Err("Incomplete or empty filter oracle");
    }
    let mut records = Vec::with_capacity(bytes.len() / 156);
    for raw in bytes.chunks_exact(156) {
        let word = |i: usize| u32::from_le_bytes(raw[4 * i..4 * i + 4].try_into().unwrap());
        if (1..28).any(|i| word(i) > 65535) {
            return Err("Malformed filter coefficient word");
        }
        if word(32) != word(38) {
            return Err("Unexpected extra filter state mutation");
        }
        let coeff = |i: usize| word(1 + i) as i16;
        let pair = |i: usize| ((word(1 + i) << 16) | word(2 + i)) as i32;
        records.push(FilterRecord {
            input: Sample(word(0) as i32),
            coefficients: FilterCoefficients {
                input_gain: coeff(0),
                feedback: pair(3),
                integrator_gain: pair(11),
                post_gain: coeff(14),
                post_feedback: coeff(16),
                mix: [coeff(18), coeff(20), coeff(22), coeff(24), coeff(26)],
            },
            initial: FilterState {
                first: word(28) as i32,
                second: word(29) as i32,
                post: [word(30) as i32, word(31) as i32],
            },
            output: Sample(word(33) as i32),
            final_state: FilterState {
                first: word(34) as i32,
                second: word(35) as i32,
                post: [word(36) as i32, word(37) as i32],
            },
        });
    }
    Ok(records)
}

pub fn decode_waveform_records(
    bytes: &[u8],
) -> Result<(Vec<WaveformFrame>, Vec<Sample>), &'static str> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(36) {
        return Err("Incomplete or empty waveform oracle");
    }
    let mut inputs = Vec::with_capacity(bytes.len() / 36);
    let mut expected = Vec::with_capacity(bytes.len() / 36);
    for record in bytes.chunks_exact(36) {
        let word = |offset| u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap());
        let transfer = match word(0) {
            0xb2ac => Transfer::CorrectedRamp,
            0xb320 => Transfer::Pulse,
            0xb380 => Transfer::ParabolicSine,
            0xb3e8 => Transfer::FoldedTriangle,
            _ => return Err("Unknown original waveform transfer"),
        };
        // FRCT/SXMD/SATD are observed in normal execution. M40 is cleared by
        // the transfer itself, sometimes before the first observed store.
        if word(28) & 0x360 != 0x340 {
            return Err("Unqualified original waveform arithmetic mode");
        }
        if word(12) > 1 || [16, 20, 24].iter().any(|i| word(*i) > u16::MAX as u32) {
            return Err("Malformed waveform control value");
        }
        inputs.push(WaveformFrame {
            transfer,
            phase: word(4) as i32,
            edge_phase: word(8) as i32,
            parameters: ShapeParameters {
                subtract_edge: word(12) != 0,
                edge_coefficient: word(16) as i16,
                waveform_control: word(20) as i16,
                gain: word(24) as i16,
            },
        });
        expected.push(Sample(word(32) as i32));
    }
    Ok((inputs, expected))
}
