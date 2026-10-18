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
Browser modulation sequencers use their JS audio-clock adapter. Native plugins
execute their modulation clocks, Step/Slide, KeySync, macro arithmetic and temporary
offsets in `crates/radias-web/src/native/modulation.rs`, under `rustias_plugin` only.
Keep native behavior checked against browser modulation/PCM fixtures. Native
plugins have no internal note sequencer: DAW Play starts modulation only, DAW MIDI
must never be intercepted by imported browser note patterns. Preserve those
patterns as inert program data for browser interchange. Sequencer Gate targets
are unavailable in plugins; DAW note lengths are owned by the host.
Never save playback offsets into program values or Macro base positions.
Native MIDI events keep their host timestamps. Coalesce editor state snapshots to
at most 30 Hz; never serialize the complete program per CC/pitch event or feed a
MIDI display refresh back as a full modulation configuration. Preserve the final
values and per-channel CC7 updates, and flush pending display state on Stop.
Native editor knob gestures send scalar controls, never a complete modulation
configuration. `web/plugin-automation.js` resends only topology/binding changes
and includes only referenced FX/circuit programs. After restoring temporary
offsets and applying a scalar control, `rustias_modulation_accept_values` updates
the Rust bases without resetting clocks. Keep manual rebasing, macros and Stop
restoration checked at 44.1/48/96 kHz and verify real editor gestures stay scalar.

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
The JS editor/control/output adapter remains shared with web/worklet.js:
JavaScriptCore on macOS, QuickJS-NG on Windows/Linux. WebAssembly is disabled in that
context; do not reintroduce a WASM audio fallback, JS modulation clock or Wasmtime.
DAW transport supplies native modulation tempo and PPQ on start/seek/loop;
live MIDI runs it with the configured KeySync even while transport is stopped.
WKWebView/WebView2/WebKitGTK
only host the editor; its WASM resource is for RDL parsing and metadata.
Only DAW lifecycle, MIDI/transport, automation, state and editor bridges belong in
the shell. Editor initialization must adopt restored sample/profile IDs without
resetting a playing instance. DAW state includes loaded sample PCM, full session
and sequencer/modulation data. Web libraries/recordings use persistent editor
storage. Never introduce a hardware AudioContext in a plugin. All plugin checks
render into discarded memory; rustias-plugin-preview is a silent UI host.
Run CTest (including native/browser PCM/state parity) and strict Apple auval when
changing host code. CI builds macOS universal, Windows x64 and Linux x86_64 plugin installers. Linux uses GTK3/WebKitGTK 4.1 with X11/XWayland embedding; run its real editor test under Xvfb with discarded audio buffers.
Mac packaging also builds a selectable AU/VST3/CLAP `.pkg` for the current user,
with no install scripts. Disable bundle relocation and use atomic bundle upgrades;
never let Installer replace a development checkout or merge stale resources.
`scripts/verify-plugin-pkg.py` inspects the real payload and Installer choices.
AU registration belongs to `plugin/Info-AU.plist.in`; a POST_BUILD-only plist copy
is insufficient because a later CMake reconfigure can overwrite it without a relink.
AU audio deactivation and temporary Cocoa view detachment must retain the editor.
Destroy GUI resources only when the view is released or replaced; CLAP hide/show
must work without another set_parent. Keep the pinned wrapper lifecycle patch in
`plugin/patches/` and validate power cycles, view reattachment and editor recreation
through the real AU bundle without audio hardware.

Expanded synthesis appends OSC 4 IDs 179–188 and filter model/drive/morph IDs
189–194 (195 parameters total). Old OSC 3 API accepts only 171–178; never send
OSC 4/filter controls through rustias_osc3_control. OSC 4 starts muted; filter
model 0 retains RADIAS arithmetic. Build builtin OSC 4 uses free ID 62 in new
graphs and mixer input d; do not rewrite existing custom graphs or occupied IDs.
New filter families are independent Rustias models, not hardware parity claims.

Mod lane playback defaults to Timbre. Poly lanes run in each Rust voice, including
PCM, with independent cursor, loop, durations, probability, transitions and RNG.
Only per-voice synth targets are valid in Poly; macros/FX/note Gate stay Timbre.
Per-voice offsets never write shared program values. Preserve cursors on edits,
copy/remap targets with timbre sounds, and carry DAW tempo without overwriting the
saved BPM. Keep the shared browser adapter and native modulation engine from
also applying Poly offsets globally. Verify real WASM, native/browser fixtures,
old PCM, and exact original-profile tests; run UI checks with audio stopped.

Release tags must match the plugin CMake version. `release.yml` publishes only after all platform checks, actual installer smoke tests and VST3 validation pass. Keep installers and archives in Release assets, never commit generated binaries. Windows Setup installs VST3/CLAP plus signed Microsoft prerequisites; preserve LocalAppData libraries on uninstall. Linux `.run` installs per-user with upgrade backups; `.deb` installs in standard system plugin paths. Neither uninstaller removes user patches or recordings.
