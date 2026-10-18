# Expanded filter optimization

The browser and native plugins use the same expanded filter kernels in
`crates/radias-synth-infrastructure/src/circuit/filters.rs`. The original desktop
RADIAS profile and model 0 retain their recovered arithmetic.

The expanded models use two oversampling passes and, for nonlinear models,
additional saturation and feedback calculations. They remain more expensive than
the original fixed-point filter. This change removes repeated work while keeping
the original f64 operations, saturation, state updates and oversampling rate:

- Cache parameter coefficients separately from signal history. Drive/Morph edits
  do not rebuild cutoff/resonance coefficients; Resonance edits do not repeat tan.
- Reuse exact cutoff/prewarp results across voices in a 256-entry, thread-local
  cache. It owns no instance state, allocates no heap storage and takes no locks.
  Collisions recompute the exact expression; thread migration changes only hits.
- Specialize each topology so pole/stage loops contain no model selection.
- Return the exact saturation endpoints +/-1 directly when the input to the
  rational soft clip reaches +/-3.

Both fixed filter slots, additional Build filters and PCM voices use these kernels.

## Measured full-render cost

Measured on 2026-10-18, Apple M5 Max, macOS arm64, Node v24.19.0, ordinary release
WASM, engine rate 48 kHz. These are **complete engine render** times per block of
128 stereo frames, not isolated filter timings. Seven rounds alternate build
order; each round warms 128 blocks and times another 128. Startup and program
configuration are excluded. No audio device is opened.

The baseline is the unmodified `18e7d4f` source. Saw voices are distributed across
eight timbres, with no added FX. The filter workloads set Cutoff=70,
Resonance=70, Drive=64. Dual uses M/Poly followed by Multi Filter in series,
with the second Cutoff=85. Poly workloads add four Slide lanes to timbre 1.

| Workload | Voices | Before, ms/block | After, ms/block | Render time saved |
| --- | ---: | ---: | ---: | ---: |
| RADIAS control | 128 | 3.29084 | 3.29273 | -0.06% |
| Ladder LP24 | 32 | 1.15587 | 1.10886 | 4.07% |
| Pro LP24 | 32 | 1.38516 | 1.32717 | 4.19% |
| Multi Filter | 32 | 1.08820 | 1.06359 | 2.26% |
| M/Poly LP24 | 32 | 1.41023 | 1.31443 | 6.79% |
| M/Poly LP24 | 128 | 5.42834 | 5.20115 | 4.19% |
| Linear LP24 | 32 | 1.07257 | 1.03528 | 3.48% |
| Multi Filter + Poly | 48 | 1.62204 | 1.57818 | 2.70% |
| M/Poly + Poly | 48 | 2.07779 | 1.95684 | 5.82% |
| Dual M/Poly + Multi | 128 | 6.09944 | 5.73122 | 6.04% |

The RADIAS difference is measurement noise. Results depend on patch, voice count
and runtime; these numbers do not measure a DAW or guarantee 128 real-time voices.
The 48 kHz budget is 2.667 ms/block, which the measured 128-voice expanded workloads
still exceed in Node.

## Sound verification and reproduction

The two production WASM builds produced **identical PCM bytes** for all 279
scenarios, covering 3,428,352 stereo frames. Coverage includes the 17 added models
in both slots, live Cutoff/Resonance/Drive/Morph/model changes, all filter routes,
PCM, Build with audio-rate Cutoff CV, Poly modulation, the older synthesis/FX
scenarios and release tails. The combined PCM SHA-256 is
`52fd22c9d93c047e45acb4d2ade3a98baeb774240a5ec2432bcd58a00b1fc2f8`.

Retain the earlier build before rebuilding. Run from the repository root:

```sh
cp dist/rustias.wasm /tmp/rustias-before.wasm
# Apply the source change.
bash scripts/build-web.sh
node scripts/compare-render-performance.mjs /tmp/rustias-before.wasm dist/rustias.wasm \
  --rounds=7 --blocks=128 \
  --bench=saw128,model-1-32,model-4-32,model-6-32,model-9-32,model-11-32,model-9-128,model-poly-6-48,model-poly-9-48,model-dual-128
```

The script checks PCM before timing and writes machine metadata, binary hashes,
every timing sample and medians as JSON. `--verify-only` skips timing. Do not run
other builds or benchmarks concurrently with the timed portion.

`cargo test --locked -p radias-synth-infrastructure --test expanded_filters`
also checks exact cached/uncached output across independent voice histories,
model changes, random coefficient keys, collisions, clamp edges and release
tails, plus the rational soft clip at and between saturation endpoints. It builds
the canonical kernel without changing the desktop engine's reference profile.
