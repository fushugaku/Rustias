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
`web-polyphony` AND either WASM or the explicit `rustias_plugin` cfg. Eight timbres, eight virtual patches, three synthesis LFOs and
thirty-three FX slots and OSC 3 require `web-expanded` AND either WASM or the explicit `rustias_plugin` cfg; reference native sizes stay
four, six, two and nine even when that feature is enabled on a native test without that cfg. Only scripts/build-plugin-engine.py enables this cfg
in an isolated Cargo invocation; ordinary --all-features must keep the reference
profile. Never enable those features or the plugin cfg in the original
desktop workspace. Browser sequencing, sample library, circuit UI and storage
belong to `web/` and `crates/radias-web`, with no dependency from the native app.
Exact shared arithmetic optimizations must retain staged rounding and saturation:
Q31 needs the high-half MIN16*MIN16 correction even with nonzero low halves.
Only exactly zero inactive stereo caches may skip decay; nonzero tails and bus
accumulation order must remain unchanged. Compare native PCM/WASM before/after.
Browser oscillator caches contain compiled controls only; phase, Sine feedback,
Sync/Ring and four-frame smoothing continue every sample. FX coefficient keys
include exact argument bits and sample rate. Browser FX scratch is type-sized:
no ring for delay-free algorithms, complete rings for delays, retained on bypass.
Native racks keep all nine preallocated rings and the strict processor API.
Active FX plans preserve paired-insert skips even when the first insert is
bypassed, and the global active predicate still counts the skipped odd insert.
OSC 3 replaces browser Noise using the shared OSC 1 controller and OSC 2 tuning;
stored IDs 171–178 and Virtual Patch destinations 43–51 append to the old schema.
Ring/Sync observes native OSC 1/2 phase/audio taps without altering their states
or output; the OSC 1→2 source is the current processed OSC 2 result.
Browser FX 3/4 use the Master catalog but the owning timbre's context and stereo
bus, before the shared Master. Original insert indices 0–15 and Master 16 remain
stable; the added timbre pairs occupy 17–32. Native racks keep two inserts.
Additional browser timbre FX live in `effects.chains[timbre]` with stable string
IDs, after the four fixed stages and before Master. No fixed chain count cap;
reuse shared EffectAudioProcessor state by ID on reorder. Keep the active-FX fast
path aware of chains, and grow the browser JSON buffer before obtaining its view.
OSC 3 uses OSC 1's selection-dependent mixer scale in the web/plugin profile; reference native Noise remains
unchanged. Sequencer Gate modulation is a temporary percentage-point offset,
including current note releases, and never overwrites the saved step Gate.
Program macros, their bipolar bindings and added-timbre count are browser data.
Modulation sequencers also belong to the browser: their audio-clock adapter applies
temporary offsets through the existing Rust C ABI and restores the base values.
Never save playback offsets into program values or Macro base positions.

The Pages workflow builds these sources on every push to `main`. Verify the real
WASM with `node scripts/verify-web.mjs` and check UI changes in a browser, including
mobile. Do not commit firmware, local reference captures, backup directories or
browser QA data.
Run audio checks offline against WASM/AudioWorklet buffers. Do not start audible
browser playback or recording during QA; the user explicitly requires quiet tests.
Browser checks cover controls/layout with audio stopped. Build assets share one
revision and fingerprinted filenames; keep module imports, worker URLs, JSON
tables and WASM on that revision. OSC 3 edits use `osc3-control` messages and
`rustias_osc3_control` fields 0–7; its stored IDs remain 171–178. Preserve OSC 2
IDs 13–16 and verify its Wave/Semi/Fine cannot retune isolated Wave/Normal OSC 3.

`android/` embeds the same `dist/` using WebViewAssetLoader. Keep synthesis and
instrument UI in the shared crates/web; Android-specific file dialogs, lifecycle
and fullscreen live in the shell. `bash scripts/build-android.sh` builds the web
assets, APK, export unit tests and Android lint. Do not start audible device tests.

`plugin/engine` statically links an AOT native Rust build of the same
`crates/radias-web` and synthesis sources into AUv2/VST3/CLAP. The explicit
`rustias_plugin` cfg opts into the web extensions without changing ordinary native
reference builds, even under --all-features. Keep synthesis in these shared crates,
not copied into the shell. NativeHost owns per-instance state; never rely on one
thread-local singleton for multiple DAW instances or for thread migration.
The JS control/sequencing/output adapter remains shared with web/worklet.js:
JavaScriptCore on macOS, QuickJS-NG on Windows. WebAssembly is disabled in that
context; do not reintroduce a WASM audio fallback or Wasmtime. WKWebView/WebView2
only host the editor; its WASM resource is for RDL parsing and metadata.
Only DAW lifecycle, MIDI/transport, automation, state and editor bridges belong in
the shell. Editor initialization must adopt restored sample/profile IDs without
resetting a playing instance. DAW state includes loaded sample PCM, full session
and sequencer/modulation data. Web libraries/recordings use persistent editor
storage. Never introduce a hardware AudioContext in a plugin. All plugin checks
render into discarded memory; rustias-plugin-preview is a silent UI host.
Run CTest (including native/browser PCM/state parity) and strict Apple auval when
changing host code. CI builds macOS universal and Windows x64 plugin archives.
