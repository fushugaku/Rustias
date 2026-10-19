# Rustias AU / VST3

[Русский](README.ru.md)

The full firmware-free Rustias instrument as a DAW plugin. **Synthesis, filters,
voices, sample playback and effects are compiled ahead of time from Rust into
native machine code** for Apple Silicon, Intel macOS, Windows x64 and Linux x86_64. There is
no WASM runtime in the plugin's audio engine.

The browser and plugins compile the same canonical Rust sources, with eight
timbres, 128 voices, OSC 3/4, modular routing and timbre effect chains. Modulation
sequencers and macro automation also execute in native Rust. The editor retains
the shared browser interface. The original
reference desktop profile remains unchanged.

## Install

Download installers and checksums from [GitHub Releases](https://github.com/fushugaku/Rustias/releases/latest).

**macOS 12 or later, Apple Silicon or Intel:** open `Rustias-macOS-<version>.pkg`.
Installer lets you choose AU, VST3 and CLAP and installs them for your account
in `~/Library/Audio/Plug-Ins/`. No administrator password is required. Existing
bundles are replaced atomically, including their resources. Quit your DAW before
installing, then reopen it and rescan plugins.

The ZIP remains available: unzip `Rustias-macOS-AU-VST3.zip` and run
`Install.command`, or copy `Rustias.component` to
`~/Library/Audio/Plug-Ins/Components/` and `Rustias.vst3` to
`~/Library/Audio/Plug-Ins/VST3/`. AU is an instrument for Logic / GarageBand;
VST3 is an instrument for Ableton Live, Cubase, REAPER and other VST3 hosts.
Restart the DAW and rescan its plugins. The supplied local builds use an ad-hoc
signature; the PKG is unsigned unless an Installer identity is configured.
There is no Developer ID notarization.

**Windows 10/11 x64:** run `Rustias-Windows-x64-<version>-Setup.exe` and select VST3 and/or CLAP. Setup installs into the standard Common Files plugin directories, includes the Visual C++ runtime and the WebView2 bootstrapper, and registers an uninstaller. Installing a missing WebView2 runtime requires an internet connection. User libraries and recordings remain in LocalAppData when uninstalling. The executable is unsigned.

For manual installation, unzip `Rustias-Windows-VST3.zip` and copy the entire
`Rustias.vst3` directory to `C:\Program Files\Common Files\VST3\`.
Keep its `Contents/Resources` directory intact. The editor requires Microsoft's
[WebView2 Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/).
Install the [Microsoft Visual C++ x64 Runtime](https://aka.ms/vs/17/release/vc_redist.x64.exe)
if it is not already installed with the DAW.
The instrument renders without the editor or a browser window open.

CLAP builds are included too: on macOS use `~/Library/Audio/Plug-Ins/CLAP/`;
on Windows keep `Rustias.clap` and `Rustias.resources` together in the CLAP folder.
Legacy VST2 is not included.

**Linux x86_64, X11/XWayland:** run `bash Rustias-Linux-x86_64-<version>.run` for per-user installation into `~/.vst3` and `~/.clap`. Python 3 and WebKitGTK 4.1 (2.40+) are required. Debian/Ubuntu users can instead run `sudo apt install ./Rustias-Linux-amd64-<version>.deb` for system-wide installation with dependencies. The editor runs in an isolated WebKitGTK process and embeds through X11/XWayland, avoiding conflicts with the DAW's GTK version. Other architectures are not packaged.

The `.run` installer preserves old plugin bundles under `~/.local/state/rustias/plugin-backups` and accepts `--uninstall`. Removal keeps user libraries and recordings. The `.deb` is removed through the system package manager.

## In the DAW

Create an instrument track, load Rustias and send it MIDI. The plugin's **DAW**
status means its output goes to that track; it does not open a separate audio
device. Notes use each timbre's MIDI channel and receive settings. Compose notes
in the DAW: the plugin has no internal note sequencer or pattern Play button.
The **Sound** and **Mod sequencer** tabs use the full editor workspace. Drag the
bottom-right corner to resize the plugin window; the request goes through the DAW.
The **Mod sequencer** tab provides up to six independent lanes per timbre,
with 1–128 steps, independent divisions, Step/Slide, directions, Loop/OneShot and
Key Sync. It can modulate synthesis, samples, effects, modules and macros.
DAW tempo, Play/Stop and song position synchronize its native Rust clock;
seek and loop jumps restart it at the corresponding song position. Live MIDI
also starts modulation, including while the transport is stopped.
MIDI events retain their host timestamps; controller display updates are combined
at up to 30 Hz so wheel and CC bursts do not serialize a full program per event.
Moving a synth knob sends a scalar control; Rust updates its modulation base
without rebuilding the rack or resetting its clocks. Changes to bindings,
sequencer settings and referenced FX/module programs resynchronize the rack.
Short DAW buffers render only the required native samples, with no 128-sample
calculation burst. The modulation/FX controller quantum stays at 128 samples,
so buffer size does not change the sound. PCM buffers are reused, periodic editor
telemetry is limited to 20 Hz, and closing or hiding the editor disables it.
Audio rendering, MIDI, transport and parameter updates call native Rust directly.
The DAW callback does not enter a JavaScript VM. Outgoing message and recording
array formatting runs on a separate native worker; the callback never waits for it.
The existing web controls, program/timbre save, RDL import, sample upload,
recordings and WAV/MP3 exports remain available.

DAW automation exposes **Macro 1–8** and **Output level**. Right-click a synth or
FX control to assign it to a macro; edit the amount with its pencil. Individual
synth parameters are available through these assignments.

Saving the DAW project stores the complete current program, every timbre,
modulation sequencers, macros, routing, FX and loaded sample PCM in the plugin
state. A fresh instance starts independently. User patch libraries and recordings
remain in the editor's persistent local storage; recordings are not embedded in
the DAW project. Keep a WAV/MP3 export for portable recordings.
Browser note patterns remain in imported program data for round trips, but are
never played by the plugin. Sequencer Gate is unavailable as a plugin modulation
target because MIDI note lengths come from the DAW.

## Build and verify

macOS requires Xcode command-line tools, Python 3, CMake 3.25+, Node 22+, Rust
and the `aarch64-apple-darwin`, `x86_64-apple-darwin` and
`wasm32-unknown-unknown` targets. The WASM target builds the browser assets and
RDL import worker, not the plugin DSP:

```sh
bash scripts/build-plugin.sh
```

Windows additionally requires Visual Studio 2022 C++ desktop build tools:

```powershell
./scripts/build-plugin-windows.ps1
```

On Linux, install WebKitGTK 4.1/GTK3/X11 development libraries, then configure with CMake and build the same targets. Run CTest under X11 (CI uses Xvfb) and `python3 scripts/package-plugin-linux.py` to produce `.run` and `.deb` installers.

Archives and all installers appear in `output/plugin/packages/`. The `plugins.yml`
workflow builds and verifies macOS universal, Windows x64 and Linux x86_64 on every shared engine/UI change. After pushing a version to `main`, a matching `v<version>` tag promotes that exact commit's successful CI artifacts to GitHub Releases with SHA-256 checksums through `release.yml`; it does not rebuild different binaries.
SDK versions and hashes are pinned in `plugin/CMakeLists.txt`.

To package validated Mac bundles without rebuilding audio:

```sh
python3 scripts/package-plugin-pkg.py --bundles output/plugin/packages/Rustias-macOS
```

Set `RUSTIAS_INSTALLER_IDENTITY` to an available **Developer ID Installer**
identity to sign the PKG. The builder verifies the expanded payload, signatures,
architectures, user installation domain, selectable formats and upgrade behavior.
The installer has English and Russian screens and executes no install scripts.

`ctest` compares 125 native PCM/state scenarios with the actual browser build,
then repeats each with sliced native rendering and requires bit-identical PCM.
Streaming checks compare 1/17/32/64/128/257/512-frame host buffers at
44.1/48/96 kHz, timestamped edits, modulation and recording, and run four
independent instances concurrently against their isolated PCM references.
The fixture suite includes 22 modulation scenarios executed directly through the Rust ABI with
no JavaScript runtime. It covers independent clocks, Step/Slide, all directions,
OneShot, key sync, bipolar macro bindings, FX/chains, modules and sample profiles,
then verifies the native engine at 44.1/48/96 kHz, MIDI, variable blocks,
instance isolation, complete state round trips and sample-rate changes. macOS
packaging also runs Apple's strict `auval` check. These checks render into memory
and never open audio output. The optional `rustias-plugin-preview` target is a
silent UI validation host that discards every output buffer.
The actual AU Cocoa editor is also checked for host-driven and plugin-requested
window resizing, audio power cycles with the editor open, view detachment and
reattachment, and repeated editor replacement with automation and size preserved.
The direct editor host checks hide/show without reparenting.

CI also runs Steinberg's VST3 validator against the actual packaged VST3 module.
The AU check loads its exported factory directly and verifies MIDI, stereo output
and saved automation at 44.1/48 kHz. Hosted runners without a working AU registry
use this check when `auval` cannot discover the bundle; local packaging requires
strict `auval` validation. Both macOS architectures can run the direct check.

## Implementation

Version 0.2.5 captures MIDI/display state and telemetry into preallocated owned
snapshots. JSON formatting happens on the message worker, which never reads a
live engine. A coalescing mailbox preserves the final MIDI/CC7 and full corrective
state even when Stop is followed by no further rendering. FX and chain modulation
call shared typed compiler/configure helpers; change scratch and macro/lane links
are prepared ahead of rendering. Recording reuses PCM chunks, and idle formatters
wait for notifications instead of waking on a 2 ms timer.

A frozen 0.2.4 engine remains the local before/after reference: 210 adapter scenes
plus 12 dense-control timelines match every PCM bit and state snapshot on Apple
Silicon and Intel. The browser's 279 scenes / 3,428,352 stereo frames also match.
Control tests include invalid FX parameter repair, duplicate chain IDs, final Stop
under queue pressure, independent telemetry readback and exact recorded PCM.

On Apple M5 Max at 48 kHz / 128 frames, two ABBA passes (each three rounds of
1,024 callback batches) measured these per-pass ranges:

| Workload | Mean µs, 0.2.4 → 0.2.5 | P99 µs, 0.2.4 → 0.2.5 |
| --- | ---: | ---: |
| Two parallel AU, four sources / MS-20 / controllers | 274–284 → 226–230 | 684–686 → 415–417 |
| Two native runtimes, MIDI / editor telemetry | 253–271 → 216–219 | 709–728 → 237–241 |
| Two native runtimes, FX Slide / dense macros | 298–302 → 247–249 | 776 → 278–289 |
| Eight native runtimes, MIDI / editor telemetry | 1,014–1,086 → 869–879 | 2,817–2,875 → 937–944 |

The eight-runtime test exceeded its 2,667 µs budget in 474/6,144 batches before
and 0/6,144 after. Runtime tests process instances sequentially with telemetry
enabled, without opening editor windows; the AU test includes parallel thread
rendezvous. Static two/four-instance AU mean times differed by less than 3%.
These are workload-specific offline measurements, not a scheduling guarantee.
C++ `new` calls in measured MIDI/recording callbacks fell to zero; this counter
does not count Rust allocations, setup or incoming JSON editor commands.

```sh
cmake --build output/plugin/build --target rustias-control-benchmark
output/plugin/build/rustias-control-benchmark output/plugin/resources result.json
```

Version 0.2.4 removes the remaining script VM from the native audio path. A live
Logic profile of 0.2.3 showed JavaScriptCore lock and garbage-collector waits in
both AU rendering threads. An offline stress scene based on the visible patch
uses two parallel AU instances, six voices each, four sources, two MS-20 filters
and continuous timestamped wheel/pitch events. On an Apple M5 Max, 48 kHz and
128-frame buffers, three rounds of 512 callbacks measured:

| Measurement | 0.2.3 | 0.2.4 |
| --- | ---: | ---: |
| Mean callback | 790 µs | 312 µs |
| P99 callback | 3,918 µs | 867 µs |
| Callbacks exceeding 2,667 µs | 122 / 1,536 | 0 / 1,536 |

These are ordinary offline-host measurements, including thread rendezvous;
they do not promise deadlines under every patch or OS load. The former adapter
remains an offline reference only. Tests compare every PCM bit in 210 scenes
and 1,720,320 stereo frames on each platform at 44.1/48/96 kHz: all filter models,
oscillators, all 30 FX algorithms, chains, samples, macros and buffer splits.

```sh
output/plugin/build/rustias-au-benchmark /path/to/Rustias.component 512 parallel.json parallel
```

The macOS offline AU benchmark compares one instance with two timbres, two/four
instances, and RADIAS/M/Poly filters. For example, on an Apple M5 Max at 48 kHz,
32-frame host buffers and eight held voices per instance, with editors closed:

| Workload | P99 callback time, 0.2.2 | P99 callback time, 0.2.3 |
| --- | ---: | ---: |
| Two RADIAS instances | 329 µs | 131 µs |
| Four RADIAS instances | 690 µs | 280 µs |
| Four M/Poly instances | 986 µs | 393 µs |

P99 fell by about 60% in these cases. Average callback cost fell by about 14–25%;
the idle-instance average fell from 21 to 9 µs. The 32-frame budget is 667 µs.
These are ordinary offline-host measurements; patch, host scheduling and machine
load affect the results. Reproduce with each plugin version in a separate process:

```sh
cmake --build output/plugin/build --target rustias-au-benchmark
output/plugin/build/rustias-au-benchmark /path/to/Rustias.component 1024 result.json
```

`plugin/engine` builds a Rust static library linked into the native plugin.
It depends on `crates/radias-web` and the canonical `radias-synth-*` crates; there
is no copied DSP implementation. `scripts/build-plugin-engine.py` enables the
explicit `rustias_plugin` compiler cfg in an isolated Cargo build. Ordinary native
builds, including `--all-features`, retain their original reference capacities.
Each plugin instance owns its engine and can move between DAW threads.
Expanded voice storage is allocated on the heap. Initialization and whole-program
loading use a Rust preparation thread with an 8 MiB stack; rendering calls the
native engine directly on the DAW thread.

`crates/radias-web/src/native/modulation.rs` owns per-instance native modulation:
phase advancement, interpolation, lane priority, macro bindings, parameter writes
and base-value restoration. The UI only edits its configuration and displays its
reported positions. Saving or stopping never stores temporary playback offsets.
`plugin/src/RuntimeNative.cpp` dispatches controls, MIDI and output buffers to
that native library, retaining the former worklet's resampling arithmetic.
JavaScriptCore on macOS and QuickJS-NG on Windows/Linux are used only by the
offline adapter oracle, not linked as the production audio adapter.
The full editor uses WKWebView / WebView2 / WebKitGTK (XEmbed). On Linux, GTK/WebKit and their event loop live in a separate helper process; bounded nonblocking IPC forwards UI commands and state, while synthesis stays inside the DAW plugin. The native audio bridge has no WebAssembly runtime or Wasmtime dependency. A WASM file remains in
the editor's web assets for local RDL parsing and metadata; it does not render
the plugin's sound. The UI, controls and storage stay shared with the browser.

Bundles are self-contained and work offline. The initial release is a development
build; validation does not establish compatibility with every DAW or hardware
sound parity. Dependency licenses are in `THIRD_PARTY_NOTICES.txt` and the bundle's
`licenses/` directory.

OSC 4, its five Wavefolder shapes, the expanded filter bank and per-voice Poly motion are compiled into the native Rust engine. Playback in each mod lane selects the original Timbre clock or Poly motion; their clocks, probabilities, glides and envelope modulation stay in Rust. Poly motion follows DAW tempo without changing the saved BPM. See the root README for controls and target scope.
