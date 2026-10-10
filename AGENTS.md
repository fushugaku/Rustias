# Shared synthesis source

The canonical synthesis crates are this repository's `crates/radias-synth-domain`,
`crates/radias-synth-application`, and `crates/radias-synth-infrastructure`.
The local `../radias-emulator/rust` workspace links their `src/` and `examples/`
directories here. Edits through either path change these same files; commit shared
engine changes in Rustias so the browser build receives them. Use this repository
as the Semble search root for the synthesis implementation.

Keep desktop parity intact: the native fixed graph, 24 voices, physical controller
scheduling, and original reference workflows must remain unchanged. Modular
routing is guarded by `web-modular`; expanded allocation is guarded by
`web-polyphony` AND the WASM target. Eight timbres, eight virtual patches and
seventeen FX slots require `web-expanded` AND the WASM target; native sizes stay
four, six and nine even when that feature is enabled on a native test. Never enable those features in the original
desktop workspace. Browser sequencing, sample library, circuit UI and storage
belong to `web/` and `crates/radias-web`, with no dependency from the native app.
Program macros, their bipolar bindings and added-timbre count are browser data.

The Pages workflow builds these sources on every push to `main`. Verify the real
WASM with `node scripts/verify-web.mjs` and check UI changes in a browser, including
mobile. Do not commit firmware, local reference captures, backup directories or
browser QA data.
