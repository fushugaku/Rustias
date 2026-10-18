// Exercise the expanded kernels without opting the desktop engine into the
// web/plugin profile. Both paths below execute the canonical source file.
#[path = "../src/circuit/filters.rs"]
mod filters;

#[test]
fn coefficient_cache_keeps_voice_histories_independent() {
    let mut cached: [filters::Model; 18] = std::array::from_fn(|_| Default::default());
    let mut direct: [filters::Model; 18] = std::array::from_fn(|_| Default::default());
    let mut random = 97_u32;
    for frame in 0..8192 {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        // Shared keys, distinct keys, cache collisions, both clamps and zero.
        let coefficient = match frame % 7 {
            0 => 0,
            1 => i32::MAX,
            2 => i32::MIN,
            _ => random as i32,
        };
        for voice in 0..18 {
            let input = if frame < 4096 {
                (random as i32 as f64 / 2147483648.0) * (voice + 1) as f64
            } else {
                0.0 // Preserve nonzero release tails too.
            };
            let mode = ((voice + frame / 113) % 18) as u8;
            let resonance = ((frame / 17 + voice) % 130) as f64 / 127.0;
            let drive = ((frame / 29 + voice) % 130) as f64 / 127.0;
            let morph = ((frame / 23 + voice) % 130) as f64 / 127.0;
            let actual =
                cached[voice].sample_coefficient(input, mode, coefficient, resonance, drive, morph);
            let expected = direct[voice].sample(
                input,
                mode,
                filters::frequency(coefficient),
                resonance,
                drive,
                morph,
            );
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "voice {voice}, frame {frame}"
            );
        }
    }
}

#[test]
fn saturating_soft_clip_is_exact_at_and_between_endpoints() {
    for x in (0..=8192).map(|i| i as f64 / 1024.0 - 4.0).chain([
        -0.0,
        f64::NEG_INFINITY,
        f64::INFINITY,
        3.0_f64.next_up(),
        3.0_f64.next_down(),
        (-3.0_f64).next_up(),
        (-3.0_f64).next_down(),
    ]) {
        let clamped = x.clamp(-3.0, 3.0);
        let expected = clamped * (27.0 + clamped * clamped) / (27.0 + 9.0 * clamped * clamped);
        assert_eq!(filters::soft(x).to_bits(), expected.to_bits(), "input {x}");
    }
    assert!(filters::soft(f64::NAN).is_nan());
}
