# Rustias

**English** · [Русский](README.ru.md)

Rustias is the Rust implementation of a Korg RADIAS emulation project: a direct synthesis engine, an original-firmware interpreter, a desktop instrument and a headless debugger. Nine crates share one Cargo workspace.

The project is under development. Individual algorithms and recorded scenarios are compared with the C++ reference emulator. These comparisons do not establish complete equivalence with physical RADIAS hardware.

## WebAssembly

[Play the synthesizer](https://fushugaku.github.io/Rustias/).

The browser instrument runs **without firmware**. The shared Rust generator in `radias-synth-infrastructure::synthesizer` produces audio inside a Web Audio `AudioWorklet`. JavaScript forwards controls and plays the generated samples. The web build does not use the SH3/C55 interpreter, SYS images, RDL banks, PCM ROM or audio recordings.

The complete native synthesis editor is available below the performance panel. Its parameter definitions come from the same [schema](crates/radias-synth-infrastructure/src/parameters.json) that Rust uses to validate controls.

| Section | Available controls |
| --- | --- |
| OSC1 | Saw, Pulse, Triangle, Sine, Noise, Formant; Waveform, Cross, oscillator Unison and VPM; CTRL1/CTRL2. Noise/Formant use Waveform mode. |
| OSC2 / Mixer | Four waveforms; Normal, Ring, Sync and Ring+Sync; semitone/fine tuning; OSC1, OSC2 and noise levels |
| Filters | Filter1 type morph, cutoff, resonance, EG1 intensity and key tracking; Single, Serial, Parallel and Individual routing; Filter2 LP/HP/BP/Comb, link, cutoff, resonance and envelope/key controls |
| Drive / Waveshaper | Drive, Hard Clip, Decimator, OctSaw, MultiTri, MultiSin, four SubOSC waves, Pickup and Level Boost; depth and position |
| EG1 / EG2 / EG3 | Independent ADSR, eight curves, velocity level/time sensitivity and key tracking |
| Amplifier | Level, stereo pan, key tracking, level offset, source gain/Expression routing and optional MIDI volume; automatic Unison gain bank |
| LFO1 / LFO2 | Waveform, shape, frequency, phase, Free/Timbre/Voice sync, tempo sync, division and rate offset |
| Virtual patches | Six source/destination/intensity routes, manual offsets and patch feedback destinations |
| Voice / Pitch | Four timbres, 24 voice slots, Mono/Poly, retrigger/priority, instrument Unison with 2–8 members, detune/spread, portamento curve/time/CC65, transpose, fine tune, bend and vibrato |
| Tuning / MIDI | Eleven scales, root, master tune and custom cents; per-timbre or Global MIDI channel, key windows, receive flags; note on/off, bend, wheel, CC11 Expression, CC64 sustain, CC65 and all notes/sound off |
| Drum Kit | Sixteen independently editable synthesis instruments, owning timbre, kit level/pan/transpose, trigger notes and exclusive groups; pads retain the note used for release |

Click **Start audio** to enable playback. Choose a parameter group to edit the selected timbre. When **Drum mode** is enabled, select its owning timbre and an instrument; the synthesis editor edits that instrument and the keyboard becomes sixteen drum pads. Selecting another instrument does not stop sounding voices. **Save** and **Open** store or restore the complete four-timbre/16-instrument program as local JSON.

Monitor volume is separate from timbre level. Dials support vertical drag, Shift for fine adjustment, wheel and arrow keys. Pads support multiple pointers; held notes are released when the page loses focus. The editor adapts to mobile screens.

The standalone profile supplies every native controller with mathematically generated tables, including filters, Comb, envelopes, LFO/tempo, modulation, noise, tuning, pan and voice groups. Waveform interpolation correction remains zero. These are independent data for the shared DSP algorithms; they do not copy factory ROM or reproduce its factory programs. PCM/Audio In generators, FXD03 effects, vocoder and sequencers are still unfinished in the native engine and are not presented as working controls.

Build and serve locally:

```sh
rustup target add wasm32-unknown-unknown
bash scripts/build-web.sh
python3 -m http.server 8080 --directory dist
```

Open `http://localhost:8080`. AudioWorklet requires HTTPS or localhost. The build produces `dist/` with HTML, JavaScript and `rustias.wasm`. It requires no wasm-bindgen, npm installation or application server. Native synthesis runs at 48 kHz; the Web Audio adapter resamples when the output context uses another rate.

Run `node scripts/verify-web.mjs` to check the compiled module and AudioWorklet at 48/44.1 kHz. The [Pages workflow](.github/workflows/pages.yml) tests the standalone profile, builds WebAssembly and deploys `dist/` on pushes to `main`. Set Settings → Pages → Source to **GitHub Actions**.

## Desktop and CLI builds

The workspace uses Rust edition 2024. The initial import was tested on macOS with Rust/Cargo **1.97.1**. `Cargo.lock` pins dependencies, including eframe/egui 0.36.2, CPAL 0.18.2 and midir 0.11.0.

```sh
git clone https://github.com/fushugaku/Rustias.git
cd Rustias
cargo build --release --locked -p radias-cli
cargo build --release --locked -p radias-desktop
# Build all workspace crates:
cargo build --release --locked --workspace
```

Executables are `target/release/radias-rust` and `target/release/radias-desktop`. The CLI does not enable audio/MIDI device libraries. Desktop builds need a windowing environment and the platform libraries used by CPAL, midir and eframe. Other desktop operating systems were not verified during the import.

## Engine modes

**Direct synthesis** (`radias-synth-*`) runs the recovered oscillator, filter, envelope, amplifier, pan, LFO, modulation and voice-management algorithms in Rust. Desktop uses a 24-slot voice pool with four independently controlled timbres, generating samples in the audio callback.

**Original firmware** (`radias-domain`, `radias-application`) runs in modeled SH3/C55 processors with board memory, buses and peripherals. It is used for investigation, tracing and comparison. Instruction interpretation is slower than real time. The CLI uses this interpreter; desktop selects direct synthesis when its data and audio device are available.

## External desktop data

Building and ordinary unit tests do not require firmware. The firmware interpreter and ROM-backed desktop profile need external files. The browser profile is self-contained.

| Path relative to the data directory | Purpose |
| --- | --- |
| `firmware/RADIAS_SYS_0200.bin` | SYS 2.00 image; required by the interpreter and desktop native table extraction |
| `firmware/Radias-backup.rdl` | Program bank; required by native desktop and optional in CLI through `--backup` |
| `firmware/dsp-master-host-stream.bin` | Master DSP upload stream containing native synthesis tables |
| `assets/native-va/saw.json`, `pulse.json`, `triangle.json`, `sine.json` | Prepared native voice parameters; all four files live in `assets/native-va/` |
| `assets/native-va/filter-controls.json` | Native desktop filter control map |
| `runs/alternative-pcm/alternative-pcm.bin` | Optional alternative PCM bank for the interpreter |

The data directory can be this checkout or a separate directory with the same layout. Point desktop at the full original project with `--workspace` to use existing data directly.

## Desktop

```sh
cargo run --release --locked -p radias-desktop
# External data directory:
cargo run --release --locked -p radias-desktop -- \
  --workspace /absolute/path/to/radias-data
# Narrow window, no audio output:
cargo run --release --locked -p radias-desktop -- \
  --workspace /absolute/path/to/radias-data --no-audio --size 390x780
```

The native panel provides physical controls, program selection and an on-screen keyboard. External MIDI arrives through the virtual `RADIAS Rust` input and is forwarded to the selected engine.

`--workspace` sets the data and working-file directory. `--no-audio` disables audio output and native audio-engine initialization; the interpreter still requires the SYS image. `--size WIDTHxHEIGHT` sets the initial window size.

The firmware engine keeps completed NOR writes in `runs/rust-desktop/working-flash.bin`. Offline audition produces `runs/rust-desktop/last-preview.wav` and adjacent native channel files. Flash persistence does not save processor state, RAM, pending operations or live voices. Launching with the same data directory reuses the working image.

## Headless interpreter

CLI paths are relative to the current directory. External files can be supplied explicitly:

```sh
cargo run --release --locked -p radias-cli -- \
  --firmware /absolute/path/to/radias-data/firmware/RADIAS_SYS_0200.bin \
  --backup /absolute/path/to/radias-data/firmware/Radias-backup.rdl \
  --interactive --dry-audio runs/cli/dry.wav --mix-audio runs/cli/mix
```

Interactive mode accepts one command per line and returns JSON lines:

```text
state
run 5000000
midi 90 3c 64
runframes 4800
midi 80 3c 00
runframes 4800
quit
```

`run` advances interpreter steps; `runframes` advances board audio frames at 48 kHz. MIDI bytes are hexadecimal: the example presses/releases C4 on channel 1. Allow firmware boot to finish before sending notes; one `run` command does not guarantee readiness.

| Option | Purpose |
| --- | --- |
| `--steps N` | Run a fixed instruction budget without an interactive session |
| `--backup-global RDL` | Import Global settings only |
| `--pcm-bank BIN` | Mount a bank using native Flash layout |
| `--flash-image BIN` | Load or create a separate 4 MiB working Flash image |
| `--input-wave WAV`, `--input-loop` | Feed mono/stereo 48 kHz PCM16/24/32 or float32 WAVE; optionally loop it |
| `--dry-audio WAV` | Capture the dry audio stream |
| `--mix-audio PREFIX` | Capture `PREFIX-master.wav` and `PREFIX-slave.wav` |
| `--vocoder-audio PREFIX` | Capture available vocoder streams |
| `--dump JSON` | Save diagnostic state |
| `--trace PATH`, `--fxd-trace PATH`, `--fxd-link-trace PATH` | Record diagnostic traces |

Output-path and file-alias guards protect original SYS/RDL/PCM/WAVE inputs from overwrite. Working files and captures are ignored by Git.

## Workspace structure

| Crate | Responsibility |
| --- | --- |
| [`radias-synth-domain`](crates/radias-synth-domain) | Fixed-point synthesis algorithms; `no_std`, no dependencies |
| [`radias-synth-application`](crates/radias-synth-application) | Voice rendering, polyphony, sample clocks and control events; `no_std` |
| [`radias-synth-infrastructure`](crates/radias-synth-infrastructure) | Shared device-independent generator, standalone tables, native program/table adapters and optional CPAL output |
| [`radias-domain`](crates/radias-domain) | SH3/C55, board memory/peripherals, NOR, codec and backup/program objects; no external dependencies |
| [`radias-application`](crates/radias-application) | Machine lifecycle, commands, execution budgets and PCM policy |
| [`radias-infrastructure`](crates/radias-infrastructure) | WAV/PCM/Flash files and diagnostic captures; optional `desktop-io` audio/MIDI |
| [`radias-cli`](crates/radias-cli) | Headless composition and JSON-line protocol |
| [`radias-desktop`](crates/radias-desktop) | egui panel, keyboard and native/firmware engine composition |
| [`radias-web`](crates/radias-web) | Standalone profile C ABI for WebAssembly/AudioWorklet |

Domain layers do not depend on the UI or filesystem. Application layers manage domain objects. Infrastructure supplies external data and device adapters; desktop, CLI and the browser compose these layers.

## Verification

```sh
cargo test --workspace --all-features --locked
cargo check --workspace --all-targets --all-features --locked
bash scripts/build-web.sh
node scripts/verify-web.mjs
```

Unit tests cover program routing, WAVE formats, source-file guards, audio buffering, panel interaction and firmware-free rendering/release/allocation. The Wasm check renders every supported oscillator mode, Filter2 route and waveshaper type; it also checks live modulation, mono/unison, MIDI Expression, drum edits and complete program round trips. The real audio-device test is ignored by default because it needs original data and an output device.

Rust examples in `crates/*/examples/` are retained. Many `*_parity` programs need complete reference recordings, WAV files and observed states from the full research workspace. Those comparisons require its data and C++ oracles; ordinary unit tests do not run them. Pass a data directory explicitly to examples that accept one.

Generated SH3/C55 decoders are included and compile with Cargo. Regeneration requires the C++ sources and generators from the full project.

## Current limits

- Arbitrary RDL compilation and complete parameter combinations remain unfinished.
- FXD03 effects, the final codec/DAC path and hardware timing require further work. Pre-FXD/codec captures are intermediate outputs.
- Some controller/voice policies, sequencer/vocoder behavior and physical controls remain incomplete.
- The firmware interpreter can underrun during monitoring. The speed of individual native algorithms does not establish readiness of the entire instrument for real-time performance.
- Factory PCM ROM is external. An alternative bank does not reproduce the missing factory bank.
- The browser exposes the implemented native synthesis controls through a firmware-free table profile. Factory programs and hardware sound parity require the original data and separate validation.

Software parity confirms only the observed boundary and tested scenario. Full hardware equivalence remains a project goal.
