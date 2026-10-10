//! Whole C55 EAB1..EAFC receive and EAFE..EB27 transmit data paths.
//! Uses physical sixteen-bit memory words; no DMA/interrupt timing is implied.
pub const FRAMES_PER_BLOCK: usize = 4;
pub const SAMPLES_PER_FRAME: usize = 16;
pub const BLOCK_WORDS: usize = FRAMES_PER_BLOCK * SAMPLES_PER_FRAME * 2;
pub const ADC_WORDS: usize = FRAMES_PER_BLOCK * 2 * 2 * 2;
pub const DOUBLE_BUFFER_WORDS: usize = BLOCK_WORDS * 2;

/// The original zero flag selects the second half; every nonzero flag the first.
fn half(flag: u16) -> usize {
    usize::from(flag == 0)
}

/// Original EAB1: read even-addressed input pairs, write odd-addressed working
/// pairs, then replace the first two samples of each frame with ADC input.
pub fn receive(
    flag: u16,
    adc: &[u16; ADC_WORDS],
    bus: &[u16; DOUBLE_BUFFER_WORDS],
    working: &mut [u16; BLOCK_WORDS],
) {
    let bus_start = half(flag) * BLOCK_WORDS;
    for (destination, source) in working
        .chunks_exact_mut(2)
        .zip(bus[bus_start..bus_start + BLOCK_WORDS].chunks_exact(2))
    {
        destination[0] = source[1];
        destination[1] = source[0];
    }
    let adc_start = half(flag) * ADC_WORDS / 2;
    for frame in 0..FRAMES_PER_BLOCK {
        for channel in 0..2 {
            let source = adc_start + frame * 4 + channel * 2;
            let destination = frame * SAMPLES_PER_FRAME * 2 + channel * 2;
            working[destination] = adc[source + 1];
            working[destination + 1] = adc[source];
        }
    }
}

/// Original EAFE: write the processed working block to its selected transmit
/// half. The other half retains its previous words.
pub fn transmit(flag: u16, working: &[u16; BLOCK_WORDS], bus: &mut [u16; DOUBLE_BUFFER_WORDS]) {
    let start = half(flag) * BLOCK_WORDS;
    for (destination, source) in bus[start..start + BLOCK_WORDS]
        .chunks_exact_mut(2)
        .zip(working.chunks_exact(2))
    {
        destination[0] = source[1];
        destination[1] = source[0];
    }
}
