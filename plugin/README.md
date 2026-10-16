# Rustias AU / VST3

[Русский](README.ru.md)

The full firmware-free Rustias instrument as a DAW plugin. It uses the exact WASM
and worklet built for the browser, including eight timbres, 128 voices, OSC 3,
polyphonic and modulation sequencers, macros, modular routing, sample kits and
timbre effect chains. Changes to the shared Rust engine also reach these builds.
The original reference desktop engine is unchanged.

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

macOS requires Xcode command-line tools, CMake 3.25+, Node 22+, Rust and the
`wasm32-unknown-unknown` target:

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

`ctest` verifies the real shared engine at 44.1/48/96 kHz, MIDI, variable blocks,
instance isolation, complete state round trips and sample-rate changes. macOS
packaging also runs Apple's strict `auval` check. These checks render into memory
and never open audio output. The optional `rustias-plugin-preview` target is a
silent UI validation host that discards every output buffer.

## Implementation

The macOS shell runs the unchanged worklet in system JavaScriptCore and embeds
the full interface in WKWebView. Windows runs that worklet in QuickJS-NG, the same
WASM in Wasmtime, and the interface in WebView2. Both call the existing WASM ABI.
The UI, sequencing, DSP, effects and storage logic are shared with `web/` and
`crates/radias-web`; no separate synthesizer implementation is maintained.

Bundles are self-contained and work offline. The initial release is a development
build; validation does not establish compatibility with every DAW or hardware
sound parity. Dependency licenses are in `THIRD_PARTY_NOTICES.txt` and the bundle's
`licenses/` directory.
