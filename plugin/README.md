# Rustias AU / VST3

[Русский](README.ru.md)

The full firmware-free Rustias instrument as a DAW plugin. **Synthesis, filters,
voices, sample playback and effects are compiled ahead of time from Rust into
native machine code** for Apple Silicon, Intel macOS and Windows x64. There is
no WASM runtime in the plugin's audio engine.

The browser and plugins compile the same canonical Rust sources, with eight
timbres, 128 voices, OSC 3, modular routing and timbre effect chains. Modulation
sequencers and macro automation also execute in native Rust. The editor retains
the shared browser interface. The original
reference desktop profile remains unchanged.

## Install

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

**Windows 10/11 x64:** unzip `Rustias-Windows-VST3.zip` and copy the entire
`Rustias.vst3` directory to `C:\Program Files\Common Files\VST3\`.
Keep its `Contents/Resources` directory intact. The editor requires Microsoft's
[WebView2 Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/).
Install the [Microsoft Visual C++ x64 Runtime](https://aka.ms/vs/17/release/vc_redist.x64.exe)
if it is not already installed with the DAW.
The instrument renders without the editor or a browser window open.

CLAP builds are included too: on macOS use `~/Library/Audio/Plug-Ins/CLAP/`;
on Windows keep `Rustias.clap` and `Rustias.resources` together in the CLAP folder.
Legacy VST2 is not included.

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

Archives and the macOS `.pkg` appear in `output/plugin/packages/`. The `plugins.yml`
workflow builds macOS universal AU/VST3/CLAP, its Installer package and Windows
x64 VST3 on every shared engine/UI change.
SDK versions and hashes are pinned in `plugin/CMakeLists.txt`.

To package validated Mac bundles without rebuilding audio:

```sh
python3 scripts/package-plugin-pkg.py --bundles output/plugin/packages/Rustias-macOS
```

Set `RUSTIAS_INSTALLER_IDENTITY` to an available **Developer ID Installer**
identity to sign the PKG. The builder verifies the expanded payload, signatures,
architectures, user installation domain, selectable formats and upgrade behavior.
The installer has English and Russian screens and executes no install scripts.

`ctest` compares 100 native PCM/state scenarios with the actual browser build,
including 21 modulation scenarios executed directly through the Rust ABI with
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
The shared editor/control/output adapter runs in system JavaScriptCore on macOS
and QuickJS-NG on Windows, calling that native library.
The full editor uses WKWebView / WebView2. WebAssembly is disabled in the audio
control context and Wasmtime is no longer a dependency. A WASM file remains in
the editor's web assets for local RDL parsing and metadata; it does not render
the plugin's sound. The UI, controls and storage stay shared with the browser.

Bundles are self-contained and work offline. The initial release is a development
build; validation does not establish compatibility with every DAW or hardware
sound parity. Dependency licenses are in `THIRD_PARTY_NOTICES.txt` and the bundle's
`licenses/` directory.
