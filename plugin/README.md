# Rustias AU / VST3

[Русский](README.ru.md)

The full firmware-free Rustias instrument as a DAW plugin. **Synthesis, filters,
voices, sample playback and effects are compiled ahead of time from Rust into
native machine code** for Apple Silicon, Intel macOS and Windows x64. There is
no WASM runtime in the plugin's audio engine.

The browser and plugins compile the same canonical Rust sources, with eight
timbres, 128 voices, OSC 3, modular routing and timbre effect chains. Sequencers,
macros and the interface retain the shared JavaScript control layer. The original
reference desktop profile remains unchanged.

## Install

**macOS 12 or later, Apple Silicon or Intel:** unzip `Rustias-macOS-AU-VST3.zip`
and run `Install.command`, or copy `Rustias.component` to
`~/Library/Audio/Plug-Ins/Components/` and `Rustias.vst3` to
`~/Library/Audio/Plug-Ins/VST3/`. AU is an instrument for Logic / GarageBand;
VST3 is an instrument for Ableton Live, Cubase, REAPER and other VST3 hosts.
Restart the DAW and rescan its plugins. The supplied local builds use an ad-hoc
signature; there is no Developer ID notarization.

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
device. Notes use each timbre's MIDI channel and receive settings. DAW tempo and
Play/Stop drive all enabled sequencer tracks. The internal Play button also works.
The existing web controls, program/timbre save, RDL import, sample upload,
recordings and WAV/MP3 exports remain available.

DAW automation exposes **Macro 1–8** and **Output level**. Right-click a synth or
FX control to assign it to a macro; edit the amount with its pencil. Individual
synth parameters are available through these assignments.

Saving the DAW project stores the complete current program, every timbre,
sequencers, modulation, macros, routing, FX and loaded sample PCM in the plugin
state. A fresh instance starts independently. User patch libraries and recordings
remain in the editor's persistent local storage; recordings are not embedded in
the DAW project. Keep a WAV/MP3 export for portable recordings.

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

Archives appear in `output/plugin/packages/`. The `plugins.yml` workflow builds
macOS universal AU/VST3 and Windows x64 VST3 on every shared engine/UI change.
SDK versions and hashes are pinned in `plugin/CMakeLists.txt`.

`ctest` compares 79 native PCM/state scenarios with the actual browser build,
then verifies the native engine at 44.1/48/96 kHz, MIDI, variable blocks,
instance isolation, complete state round trips and sample-rate changes. macOS
packaging also runs Apple's strict `auval` check. These checks render into memory
and never open audio output. The optional `rustias-plugin-preview` target is a
silent UI validation host that discards every output buffer.

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

The shared sequencing, modulation, macros and output adapter run in system
JavaScriptCore on macOS and QuickJS-NG on Windows, calling that native library.
The full editor uses WKWebView / WebView2. WebAssembly is disabled in the audio
control context and Wasmtime is no longer a dependency. A WASM file remains in
the editor's web assets for local RDL parsing and metadata; it does not render
the plugin's sound. The UI, controls and storage stay shared with the browser.

Bundles are self-contained and work offline. The initial release is a development
build; validation does not establish compatibility with every DAW or hardware
sound parity. Dependency licenses are in `THIRD_PARTY_NOTICES.txt` and the bundle's
`licenses/` directory.
