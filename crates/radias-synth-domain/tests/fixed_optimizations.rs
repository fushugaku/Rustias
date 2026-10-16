//! Frozen staged DSP arithmetic is the oracle, independent of the fast formulas.
use radias_synth_domain::{
    Sample,
    fixed::{high_product, multiply_q15, multiply_q31, weighted_sum},
    pan::StereoFrame,
    stereo_cache::StereoCache,
};

fn original_high(a: i16, b: i16) -> i128 {
    (i128::from(a) * i128::from(b) * 2).min(i128::from(i32::MAX))
}
fn original_q31(a: i32, b: i32) -> i64 {
    let (ah, al) = (i128::from((a >> 16) as i16), i128::from(a as u32 & 65535));
    let (bh, bl) = (i128::from((b >> 16) as i16), i128::from(b as u32 & 65535));
    ((((al * bl * 2) >> 16) + ah * bl * 2 + al * bh * 2) >> 16) as i64
        + original_high((a >> 16) as i16, (b >> 16) as i16) as i64
}
fn original_q15(a: i32, b: i16) -> i64 {
    (original_high((a >> 16) as i16, b)
        + ((i128::from(a as u32 & 65535) * i128::from(b) * 2) >> 16)) as i64
}
fn original_weighted<const N: usize>(a: [i32; N], b: [i16; N]) -> i64 {
    let low = a
        .iter()
        .zip(b)
        .map(|(a, b)| i128::from(*a as u32 & 65535) * i128::from(b) * 2)
        .sum::<i128>();
    ((low >> 16)
        + a.iter()
            .zip(b)
            .map(|(a, b)| original_high((*a >> 16) as i16, b))
            .sum::<i128>()) as i64
}
fn random(state: &mut u64) -> i32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state as i32
}

#[test]
fn products_keep_staged_rounding_and_the_whole_minimum_high_half_region() {
    let highs = [i16::MIN, -32767, -16384, -1, 0, 1, 16384, 32766, i16::MAX];
    for ah in highs {
        for bh in highs {
            assert_eq!(high_product(ah, bh), original_high(ah, bh) as i64);
            for al in [0, 1, 32767, 32768, 65534, 65535] {
                for bl in [0, 1, 32767, 32768, 65534, 65535] {
                    let a = (i32::from(ah) << 16) | al;
                    let b = (i32::from(bh) << 16) | bl;
                    assert_eq!(multiply_q31(a, b), original_q31(a, b), "{a}/{b}");
                }
            }
        }
    }
    for low in 0..=65535u32 {
        let a = (0x8000_0000 | low) as i32;
        let b = (0x8000_0000 | low.reverse_bits() >> 16) as i32;
        assert_eq!(multiply_q31(a, b), original_q31(a, b));
        // The correction also matters away from the exact MIN32*MIN32 pair.
        assert_eq!(
            multiply_q31(a, b),
            ((i64::from(a) * i64::from(b)) >> 31) - 1
        );
        for gain in [i16::MIN, -1, 0, 1, i16::MAX] {
            assert_eq!(multiply_q15(a, gain), original_q15(a, gain));
        }
    }
    let mut state = 0xf3a5_19cd_7b24_d601;
    for _ in 0..1_000_000 {
        let a = random(&mut state);
        let b = random(&mut state);
        let gain = random(&mut state) as i16;
        assert_eq!(multiply_q31(a, b), original_q31(a, b));
        assert_eq!(multiply_q15(a, gain), original_q15(a, gain));
        let samples = [
            a,
            b,
            random(&mut state),
            random(&mut state),
            random(&mut state),
        ];
        let gains = [
            gain,
            b as i16,
            a as i16,
            random(&mut state) as i16,
            random(&mut state) as i16,
        ];
        assert_eq!(
            weighted_sum(samples, gains),
            original_weighted(samples, gains)
        );
    }
}

fn original_advance(cache: &mut StereoCache, existing: StereoFrame) -> StereoFrame {
    let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let decay = |x: Sample| {
        Sample(clamp(original_q15(
            clamp(original_q15(x.0, cache.gain)),
            -32767,
        )))
    };
    cache.samples = StereoFrame {
        left: decay(cache.samples.left),
        right: decay(cache.samples.right),
    };
    StereoFrame {
        left: Sample(clamp(
            i64::from(existing.left.0) + i64::from(cache.samples.left.0),
        )),
        right: Sample(clamp(
            i64::from(existing.right.0) + i64::from(cache.samples.right.0),
        )),
    }
}

#[test]
fn silent_cache_is_absorbing_for_every_signed_gain_and_preserves_the_bus() {
    for gain in i16::MIN..=i16::MAX {
        for bus in [
            StereoFrame::default(),
            StereoFrame {
                left: Sample(i32::MIN),
                right: Sample(i32::MAX),
            },
            StereoFrame {
                left: Sample(-1),
                right: Sample(1),
            },
        ] {
            let mut cache = StereoCache {
                gain,
                samples: Default::default(),
            };
            let mut original = cache;
            assert_eq!(cache.advance(bus), original_advance(&mut original, bus));
            assert_eq!(cache, original);
            assert_eq!(cache.samples, StereoFrame::default());
        }
    }
}

#[test]
fn nonzero_tails_and_saturated_bus_sums_keep_every_frame() {
    for gain in [i16::MIN, -32767, -30000, -16384, -1, 0, 1, i16::MAX] {
        for samples in [
            StereoFrame {
                left: Sample(i32::MIN),
                right: Sample(i32::MAX),
            },
            StereoFrame {
                left: Sample(1),
                right: Sample(-1),
            },
            StereoFrame {
                left: Sample(0),
                right: Sample(0x7f123456),
            },
        ] {
            let mut cache = StereoCache { gain, samples };
            let mut original = cache;
            for frame in 0..4096 {
                let bus = StereoFrame {
                    left: Sample(if frame & 1 == 0 { i32::MAX } else { i32::MIN }),
                    right: Sample(frame * 731),
                };
                assert_eq!(cache.advance(bus), original_advance(&mut original, bus));
                assert_eq!(cache, original);
            }
        }
    }
}
