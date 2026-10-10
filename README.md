# Rustias

**English** · [Русский](README.ru.md)

Rustias is the Rust implementation of a Korg RADIAS emulation project: a direct synthesis engine, an original-firmware interpreter, a desktop instrument and a headless debugger. Nine crates share one Cargo workspace.

The project is under development. Individual algorithms and recorded scenarios are compared with the C++ reference emulator. These comparisons do not establish complete equivalence with physical RADIAS hardware.

## WebAssembly

[Play the synthesizer](https://fushugaku.github.io/Rustias/) · [New interface](https://fushugaku.github.io/Rustias/?interface=new).

The browser instrument runs **without firmware**. The shared Rust generator in `radias-synth-infrastructure::synthesizer` produces audio inside a Web Audio `AudioWorklet`. JavaScript forwards controls and plays the generated samples. The web build does not use the SH3/C55 interpreter or SYS images and does not bundle RDL banks or factory PCM ROM. Users can import their own RDL programs. Browser drum samples are separate local files or the bundled drum libraries.

All 155 implemented native parameters are arranged on one silver instrument panel, with a full-screen button and a compact desktop layout. The browser adds Patch 7/8, for 163 parameters in total. Definitions extend the same [schema](crates/radias-synth-infrastructure/src/parameters.json) that Rust uses to validate controls.

| Section | Available controls |
| --- | --- |
| OSC1 | Saw, Pulse, Triangle, Sine, Noise, Formant; Waveform, Cross, oscillator Unison and VPM; CTRL1/CTRL2. Noise/Formant use Waveform mode. |
| OSC2 / Mixer | Four waveforms; Normal, Ring, Sync and Ring+Sync; semitone/fine tuning; OSC1, OSC2 and noise levels |
| Filters | Filter1 type morph, cutoff, resonance, EG1 intensity and key tracking; Single, Serial, Parallel and Individual routing; Filter2 LP/HP/BP/Comb, link, cutoff, resonance and envelope/key controls |
| Drive / Waveshaper | Independent Off / Drive / WavShape modes; all eleven WS types (Decimator, Hard Clip, OctSaw, MultiTri, MultiSin, four SubOSC waves, Pickup, Level Boost); Depth 0–127 and PreFilt1 / PreAmp position. Switching modes retains the WS type and depth. |
| EG1 / EG2 / EG3 | Independent ADSR, eight curves, velocity level/time sensitivity and key tracking |
| Amplifier | Level, stereo pan, key tracking, level offset, source gain/Expression routing and optional MIDI volume; automatic Unison gain bank |
| LFO1 / LFO2 | Waveform, shape, frequency, phase, Free/Timbre/Voice sync, tempo sync, division and rate offset |
| Virtual patches | Eight source/destination/intensity routes, manual offsets and feedback destinations for all eight patches |
| Voice / Pitch | Up to eight timbres, 128 shared web voice slots, Mono/Poly, retrigger/priority, instrument Unison with 2–8 members, detune/spread, portamento curve/time/CC65, transpose, fine tune, bend and vibrato |
| Tuning / MIDI | Eleven scales, root, master tune and custom cents; per-timbre or Global MIDI channel, key windows, receive flags; note on/off, bend, wheel, CC11 Expression, CC64 sustain, CC65 and all notes/sound off |
| Drum Kit | Sixteen independently editable synth or sample instruments, owning timbre, kit level/pan/transpose, common −24…+24 dB Gain, trigger notes and exclusive groups; 224 bundled recordings and local upload |
| Web sequencer | One polyphonic track per timbre, up to eight, each 1–128 steps; one Play starts every track together. Per-step chords and arbitrary samples, range Copy/Paste, velocity and gate; independent track enable/length/resolution and eight blocks of sixteen steps |
| Macros | Eight program knobs, each with up to twelve targets; independent −100…+100% influence, editable names and assignments |

Click **Start audio** to enable playback. All synthesis sections stay on the same page; choose a timbre at the top to edit it. When **Drum mode** is enabled, select its owning timbre and an instrument: the controls edit that instrument and the keyboard becomes sixteen drum pads. Playing a pad also selects its instrument for editing, marked by a blue border. Synth controls affect that instrument; Kit level/pan affect the whole kit. Selecting another instrument keeps sounding voices. Monitor volume is separate from timbre level.

New sample assignments and **808 kit** / **909 kit** start with Filter1 LPF24 (Morph 0) at maximum cutoff. Morph 127 is the near-dry Thru endpoint and is labelled in the filter heading. On previously saved kits, set Morph to 0 for an audible low-pass sweep. Native and PCM allocation share a 128-voice browser limit; oldest voices are stolen when it fills. PCM oscillator, Mono/Unison, portamento and damper controls are inactive. One-shot ignores key release, so EG release controls are inactive until Gate or Loop is selected. LFO/EG modulation needs an active virtual-patch route or filter EG intensity.

Dials support vertical drag, Shift for fine adjustment, wheel, arrow/Page/Home/End keys, direct numeric entry and double-click reset. The full-screen button expands the instrument. Desktop uses a compact full-width rack; additional timbres and effects extend its scrollable content. Smaller screens use larger touch targets.

### New interface

**New interface** opens in a separate browser tab. It uses the same Rust engine, controls, program library, sample profiles and recordings as **Classic**. The shared DSP, AudioWorklet clock and desktop instrument are unchanged.

The header keeps Program, Timbre sound, Save, BPM, Play and Record available. **Files** contains Import, Export and the RDL report; **Controls** contains audio/MIDI and performance controls. Eight macros sit below the header with target summaries and their assignment pencils. **Macros** and **Keyboard** fold these areas; macro visibility is remembered separately for desktop and phone.

The synthesis rack follows signal order, with OSC / Filters / Mod / Patch / Voice / Drums shortcuts. The effects column keeps unused slots compact and offers independent parameter folding; Master is separated from the four timbre stages. **Build** adds Fit, a clickable minimap, readable port labels and highlights the selected module's cables.

Drag the separator above the sequencer to resize it, or focus it and use Up/Down/Home/End. The overview shows all 128 stored steps in eight blocks. **Steps** shows every timbre; **Piano roll** and **Samples** show the selected track. Click a grid cell to toggle an event and audition the resulting chord. The row picker searches the full sample library. Step properties open alongside the grid, allowing sound edits during sequencing; on a phone they appear below it. **Select**, endpoint taps or Shift-click select a range for Copy/Paste, including across timbres. Note divisions produce a musical bar/beat ruler. Opening a Mod Sequence gives its controls the editor area until closed.

Layout preferences stay local and separate from sound programs. Save, Save copy, timbre loading, RDL/JSON imports and recording folders retain their existing behavior in both interfaces.

New programs start with four timbres. **+** beside the timbre tabs adds another sound and sequencer lane, up to eight. Each timbre has its own MIDI channel, module graph, source profiles and four effects. All timbres share the 128-voice pool and Master FX. Older four-timbre programs retain their original sounds; unused new slots stay disabled.

With **Drum mode** on, the owning sequencer track shows sample names from the current kit instead of pitches. Its step editor offers all sixteen named drum pads, including uploaded samples; selecting pads auditions the complete selected set. Names follow **Trigger** and kit **Transpose**, and update when sample assignments change. Synth instruments use Drum 01–16 labels. Other timbres keep their note keyboard.

Each PCM drum has independent **Key track**, **Level offset**, **Source gain**, **MIDI vol RX** and **MIDI vol** settings, alongside its filters, envelopes and Amp level. **Gain source → Manual gain** enables Source gain; Expression uses the owning timbre’s CC11 instead. CC7 changes only instruments with MIDI vol RX enabled. Older browser patches migrate the previous common manual gain into their assigned PCM instruments; subsequent edits remain independent. Key track follows the sample’s pitch transpose. Gain bank is the automatic native Unison readout; PCM uses bank 0. Playing a sequencer drum pad also selects that instrument for panel editing.

Click a sequencer step to edit its chord. The **← / →** buttons move between steps 1–128 without closing the editor and switch the visible sixteen-step block automatically. Clicking a note auditions **all selected notes together** on that track’s timbre, with its current sound, velocity and gate. Play starts all active tracks at the same native audio boundary; track lengths are independent, so one can run 128 steps while another loops sixteen. The block selector chooses steps 1–16 through 113–128. Each track has its own Resolution: 1/32, 1/24, 3/64, 1/16, 1/12, 3/32, 1/8, 1/6, 3/16, 1/4, 1/3, 3/8, 1/2, 2/3, 3/4 or 1/1. As described for [RADIAS P15 Resolutn](https://cdn.korg.com/us/support/download/files/c9f9bb7725d303cbc977b6e7cc08d464.pdf#page=106), a quarter-note step lasts one BPM beat; triplets and dotted values use their corresponding fractions. Audition and gate use the selected resolution. The step currently open has a blue outline, separate from the red playback indicator. **Copy next** copies its chord, samples, velocity and gate; Alt-click clears a step. Timing runs in the AudioWorklet, with at most one 128-frame native block of onset quantization. This sequencer is a browser extension and does not change the desktop sequencer or firmware.

Use **Add sample…** in any step to search all 224 recordings and your custom uploads, independently of the sixteen kit slots and Drum mode. **Upload** in the editor adds a custom file directly to the step. A step can combine up to 128 notes and sample sources. Clicking a sample name opens its sound in the main synthesis panel; each source has independent filters, envelopes, amplifier, pitch, Drive/WS and modulation settings per timbre. **Playback** and **Preview** follow that source. Adding a sample auditions the whole edited chord.

For a range, enable **Select** and tap its first and last step, drag across cells on desktop, or Shift-click an endpoint. **Copy** retains notes, samples, empty gaps, velocity and gate. Choose a destination step on any timbre or a later block and press **Paste** (also available inside the step editor). Ctrl/Cmd+C/V and Undo work outside text fields. Paste extends the destination loop if needed and reports truncation at step 128. Copying PCM kit steps to another timbre retains their sample identities and copies source settings when the destination has no profile for that sample.

**Gain · dB** in Drum Kit controls all kit drums and direct sequence samples, from −24 to +24 dB; the initial value is **+12 dB** (about four times the amplitude). Melodic synth timbres retain their own level. Gain is included in both saved patches and the restored session.

**Program** selects complete programs; **Timbre sound** selects a sound for the currently selected 01–08 slot. **Save** stores the entire program in localStorage: its timbre count, all timbres and names, their sequencer tracks (including notes, samples, length and resolution), sixteen drum instruments, sample assignments and per-source settings, Drum Kit Gain, all module layouts/routing graphs, FX, macros, global controls and output volume. Saving a selected program updates that record. **Save program copy…** creates an independent record and selects it. Changing a timbre or pattern keeps the current program selected; `*` indicates unsaved changes.

The Save arrow also offers **Save timbre**, **Save timbre copy…** and **Export timbre**. Individual timbres have their own library and appear in **Timbre sound**, alongside the built-in sounds. Choosing one replaces only that slot’s sound, module graph/layout and sample profiles; its current sequencer pattern, MIDI channel, key range and the other timbres remain. A drum timbre also includes its sixteen-instrument kit and assigns the shared kit to the destination slot. Programs embed complete timbre data, so later edits to a library sound do not alter previously saved programs.

The current session restores automatically after reload, with playback stopped. **Export** writes the entire program as JSON; **Import** accepts program or timbre JSON. Timbre imports join the sound library and load into the selected slot. Custom audio remains in IndexedDB; JSON stores its references. Older native programs and 16-step web sequences migrate automatically.

**Import** also accepts RADIAS Librarian **.rdl** backups, partial program banks and individual program records, up to 8 MiB. Every program becomes a saved user patch in **Program**, with its four timbres, native synthesis controls, tempo, MIDI receive settings and associated sixteen-instrument drum kit when present. The picker includes search. Importing the same file again skips existing programs and preserves subsequent edits. Banks use compact localStorage records; malformed files and storage quota failures leave the existing library unchanged. Parsing runs locally in a separate WebAssembly worker; the backup is never uploaded.

RDL imports include **both polyphonic note sequencers** and **all three Mod Sequences of each of the four original timbres**. Note patterns follow their original timbre assignments; Sequencer Link joins both halves into up to 64 steps. Every eight-note chord, muted trigger, TIE and KEY velocity is retained, along with resolution, swing, gate offset, RunMode, Latch, scan zone, transpose and BaseNote. The **⋯** beside a track opens these settings. **Pattern → RDL Seq 1 / 2** also makes unassigned source patterns available on any timbre. Play runs timed patterns together; keyboard/MIDI triggers use the saved scan zone, transpose and velocity, while **RunMode → Step** advances once per played note. TIE carries common notes without retriggering them.

Mod sequences retain all sixteen offsets, SYS 2.00 knob destinations, On/Off, resolution, direction, RunMode, KeySync and Step/Slide. **KeySync → Timbre** resets on the first note after all keys have been released; **Voice** resets the shared timbre clock on every note. Independent per-voice modulation phases are not reproduced; affected programs include this notice in their RDL report. Older RDL imports with untouched empty tracks migrate when loaded; edited browser tracks remain, with the original note patterns still selectable. Programs, copies, session restore and JSON exports retain this sequencing data. The localStorage library uses lossless LZW compression so complete 256-program banks fit with their sequences and original records; older uncompressed libraries remain readable.

Click **RDL** on an imported patch to inspect its source and conversion limits. PCM/Audio In sources are unavailable without their original ROM/input path, so dependent timbres/drums stay muted until their source is replaced. Choose an available **OSC1 waveform**; for a drum, assign a bundled or uploaded sample. Unavailable modulation routes are disabled and out-of-range controls are reported. Vocoder, hardware arpeggiator and original Global tuning/scale are not reproduced. The complete original program record, linked kit and Global record remain in the saved patch and its JSON export, including librarian metadata. Editing or replacing a source keeps these original records; JSON export is not an RDL file or a hardware-compatible conversion.

In Drum Kit, **808 kit** and **909 kit** load sixteen-instrument kits. **Source** chooses any of the 224 bundled samples or the synth engine, with **Search samples** to find a recording; **Upload** adds a local file to the selected instrument. Uploads are decoded locally, downmixed to mono and resampled to 48 kHz. Maximum duration is 30 seconds and maximum file size is 20 MB; accepted formats depend on the browser’s audio decoder. Playback modes are **One-shot** (ignores key release), **Gate** (releases EG2 with the key) and **Loop** (repeats until EG2 releases). The Rust sampler processes native filters/Comb, EG1–3, Drive/WS, pitch, LFO/virtual-patch modulation, amplifier and pan; oscillator-source controls become inactive for a PCM instrument. The browser supports 128 voices total across native synthesis, kit samples and direct sequence samples. Native desktop builds retain their original 24-voice allocation.

Custom files stay in IndexedDB on this browser and device; they are never uploaded to a server. JSON exports contain their assignments, while the audio stays in IndexedDB. To use such a patch on another browser, upload its custom files there again. The 64 bundled 808 WAVs come from [Michael Fischer’s TR-808 recordings](https://github.com/tidalcycles/sounds-tr808-fischer), distributed under [CC0-1.0](web/samples/LICENSE.txt). The complete 160-file [TR-909 set by Jason Baker / Rob Roy Recordings](https://github.com/fluid-music/open-drums/tree/475cc3314fe06f6d1af02e9790ad9707c1f2b26b/tr-909/TR909all) is bundled separately, with WAVs and the [original terms](web/samples/tr-909/TR909SET.TXT) unchanged: the set may be copied and distributed for free, but may not be modified or distributed for profit. The 909 set is not CC0. [The manifest](web/samples/manifest.json) records each bank’s license and source revision, original filenames and SHA-256 hashes.

The standalone voice profile supplies mathematically generated tables for filters, Comb, envelopes, LFO/tempo, modulation, noise, tuning, pan and voice groups. Waveform interpolation correction remains zero. Effects use the same immutable catalog, control tables and Rust audio processor as the native player. No firmware file is loaded at runtime. Original ROM PCM, live Audio In, vocoder and firmware sequencers are not exposed in this browser profile; the local sampler and polyphonic sequencer are web additions.

### Modulation sequencers

The arrow beside **T1–T8** opens that timbre's **Mod Sequencer**. **+ Mod** adds a line, up to six per timbre. Choose a numeric synthesis/sample parameter or **Macro 1–8**, then set its step offsets with the knobs. Each line has independent **Steps** (1–128), **Bars**, **Resolution**, **Intensity**, **Motion**, **SeqType** and **RunMode**. Bars changes the step count at the selected resolution; changing Resolution keeps that count. The block picker displays sixteen knobs at a time. On mobile, unused steps are hidden.

Following the offset principle of [RADIAS P12](https://cdn.korg.com/us/support/download/files/c9f9bb7725d303cbc977b6e7cc08d464.pdf#page=98), values add to the base parameter: ±63 units, ±24 semitones for Pitch/OSC2 Semitone, or ±100 percentage points for Macro. Intensity scales or reverses this offset. **Step** holds each value; **Slide** joins neighboring values continuously, including the loop boundary. Forward, Reverse, Alt1/Alt2 and Loop/OneShot are available. OneShot holds its final value; later lines take priority when assigned to the same target.

**Play** restarts all note and modulation lines together; their lengths and divisions then run independently. Keyboard/MIDI notes also start a stopped modulation clock; **KeySync** controls note-triggered resets. **Stop** or **Pause audio** restores the base sound. Blue markers on affected synth and Macro dials show live modulation while their numeric values retain the saved base. Program Save/copy, JSON and session restore preserve the lines; individual timbre sounds carry their modulation and remap owned targets when loaded into another slot. Macro references address the destination program's macros. RDL imports populate these lines from all original modulation sequences.

Timing and Macro composition run in the AudioWorklet before each native block, at both 48 and 44.1 kHz device rates. This browser adapter calls the shared Rust controls; the original desktop engine is unchanged.

### Macro knobs

Each program has eight **Macro** knobs (0–100%). Right-click a synthesis, sample, FX or added-module parameter to **Assign to macro**; on a phone, press and hold. A macro can have up to twelve targets across different timbres. Each target has its own −100…+100% influence: positive raises the parameter, negative lowers it, scaled to that parameter's range. Contributions from multiple macros add around a saved base value.

The **pencil** beside each macro's assignment count opens its name and target list. Adjust influence with the slider or remove a binding with **×**. Direct parameter edits update the base without jumping; removing a binding restores the remaining contributions. Changing an FX/module type retains incompatible assignments as unavailable. Program Save, Save copy, JSON and session restore retain names, positions, targets, strengths and bases. Loading an individual timbre keeps the program's macros and rebases that slot's targets to its new sound.

### Recording

**Record** captures the final stereo output while you keep playing the keyboard, MIDI or all active sequencer tracks. Native inserts, Master FX, drum samples and monitor Volume are included. **Stop recording** saves the take without stopping notes or the sequencer; **Pause audio** finishes an active recording before pausing playback.

Open **Recordings** beside the keyboard. Takes are grouped in a folder for the current saved program, with separate numbering in each folder. Unsaved programs use **Unsaved program**. You can choose another destination, create folders, rename folders/takes, move recordings, listen, delete, and download **WAV** (stereo 16-bit PCM at the device sample rate) or **MP3** (stereo 320 kbps). A prepared download link remains available if automatic downloading is blocked by the browser.

The audio and folder metadata live in **IndexedDB** on this device, separately from program JSON and localStorage. PCM is committed in small ordered chunks during recording; committed audio remains downloadable after an interrupted session. Exports run in a separate worker, so encoding an older take does not interrupt a new recording or playback. Files are not uploaded. MP3 uses the separately licensed [lamejs / LAME encoder](web/vendor/lamejs/README.md).

### Insert and Master effects

The five silver FX panels follow the rack and remain available in Build: **Insert FX 1 → Insert FX 2 → Timbre FX 3 → Timbre FX 4**, then one shared **Master FX** after all active stereo buses. All four timbre effects process that timbre's native voices, Drum Kit PCM and direct sequencer samples. Each slot offers all 30 algorithms: compressor/limiter/gate, filter/wah, EQ/distortion/cabinet/tube/decimator, reverb/early reflections, seven delays/echoes, chorus/ensemble/flanger/phaser, tremolo/ring modulation, pitch/grain shift, vibrato, rotary and talking modulation. **FX 3/4** use the complete Master variants, including four-band EQ, stereo tube/pitch/grain processing and six Reverb types, with independent parameters and histories per timbre. Their tempo/key/controller context belongs to that timbre; the final Master remains shared.

Choose the type, edit every native property, and use **On/Off** to bypass without losing settings. Free-time and synced delay fields keep independent values; TempoSync enables the appropriate fields, and BPM updates the native clock. Live parameter edits preserve delay/filter histories and sounding voices; type changes clear incompatible history. Rotary/Talking in Insert 1 use both insert slots, as in the native rack. The catalog is generated from Rust during the web build, rather than maintained separately in JavaScript.

Value readouts follow the [RADIAS effect guide](https://cdn.korg.com/us/support/download/files/c9f9bb7725d303cbc977b6e7cc08d464.pdf#page=124): note fractions for synced delays/LFOs, ms/s, Hz, dB, ratios and named modes. Changing TempoSync shows the appropriate free or synced value. Knobs retain the original stored parameter indices; display tables are exported from Rust with the catalog.

Programs save all **33 slots**; individual timbres save their four effects and leave Master unchanged when loaded elsewhere. Each parameter supports Macro assignment and the FX fields are selectable in Mod Sequencer. Owned FX modulation targets follow a sound when loaded into another timbre. Older 9/17-slot programs, two-insert sounds and RDL imports gain bypassed FX 3/4; existing insert/Master settings and Macro/Mod bindings keep their slot identities. JSON/session restore preserves the full rack. The extra stages are enabled only in the browser build; native desktop retains two inserts per timbre and nine slots. The audible FX reconstruction is shared with desktop; its sample arithmetic/topology has **not** been qualified against original FXD03 audio.

Build and serve locally:

```sh
rustup target add wasm32-unknown-unknown
bash scripts/build-web.sh
python3 -m http.server 8080 --directory dist
```

Open `http://localhost:8080`. AudioWorklet requires HTTPS or localhost. The build produces `dist/` with HTML, JavaScript, the drum WAV libraries and `rustias.wasm`. It requires no wasm-bindgen, npm installation or application server. Native synthesis runs at 48 kHz; the Web Audio adapter resamples when the output context uses another rate.

Run `node scripts/verify-web.mjs` to check the compiled module and AudioWorklet at 48/44.1 kHz. The [Pages workflow](.github/workflows/pages.yml) tests the standalone profile, builds WebAssembly and deploys `dist/` on pushes to `main`. Set Settings → Pages → Source to **GitHub Actions**.
Node.js 22 or newer is required to export the Rust effect catalog during the build and run verification; npm packages are not needed.

Three native reference tests require local firmware/captures and are excluded from the firmware-free Pages job. To run all native library tests against an existing reference checkout: `RADIAS_REFERENCE_ROOT=/path/to/radias-emulator cargo test --locked -p radias-synth-infrastructure --lib`. Reference files remain outside the repository and web build.

### Modular browser mode

**Build** opens a patch canvas for the selected timbre. Drag a module heading, or focus it and use the arrow keys, to change its position. **Rack** returns to the complete native control panel. Each timbre retains its own layout and cables.

Click an output port and then a matching input to connect it. Blue cables carry audio; red cables carry CV. An input accepts one cable; a new connection replaces its previous cable. Click a connected input or a cable, then **Disconnect** (or Delete) to remove it. **Add module…** adds an independent oscillator, filter, Drive/WS, VCA, mixer, LFO or envelope, up to 64 modules per timbre. New modules have their own controls. CV inputs cover oscillator pitch, filter cutoff, VCA gain, LFO rate and envelope gate.

**Add module → OSC 1** adds a complete independent primary oscillator: six waveforms, Waveform/Cross/Unison/VPM modes, CTRL 1/2, semitone/cents tuning and level. It uses the shared native Rust generators and controllers, with separate state for each module and voice. Its ports accept audio **mod** for Cross, CV **ctrl1/ctrl2**, **pitch** (±24 semitones at ±1) and **lfo** for Waveform CTRL2 depth. Noise/Formant use Waveform mode. The existing **OSCILLATOR** remains the simpler four-waveform module.

Every synthesis module except the required Output has an **×** button, including the original Noise, OSC 1, filters and envelopes. Removal deletes its attached cables and activates the resulting routing. Undo restores the module and cables; removed original modules also become available in Add module. Removed modules and independent OSC 1 settings persist in program/timbre saves and JSON.

**Drive/WS Position → PreFilt1 / PreAmp** rewires a serial modular path before Filter 1 or Amplifier while leaving every module in place. A custom branched path keeps its explicit cables. Dragging ends on release, lost pointer capture, window blur or a mode change; zoom and canvas scrolling preserve the pointer offset.

**Add module → SWITCH** provides audio **in** and independent outputs **A/B**. **Route** chooses the active output; a connected **select** CV overrides it (below 0.5 → A, otherwise B). Live changes crossfade over 5 ms without restarting voices. For a filtered/direct path, connect Drive/WS → Switch; A → Filter → Mixer A, B → Mixer B; Mixer → Amplifier → Output. Set both Mixer input levels to 127 for unity gain. Reuse the same Switch for any two audio branches. Output identities, Route and cables are saved with programs/timbres.

**Routing On** evaluates the cables for every voice, including PCM samples. Changing a cable enables routing; moving a module does not change sound. **Routing Off** uses the original fixed DSP path while retaining the custom patch. **Reset modules** restores the initial layout and disables routing. Undo/Redo cover module edits. Feedback loops are rejected. Layout, module parameters and cables are included in Save, Save copy, session restore and JSON export.

### One engine source

The oscillator, filter, envelope, modulation and voice kernels live in `crates/radias-synth-*`. WASM and the native device adapter call the same kernels. The local `radias-emulator/rust` checkout links its synthesis `src/` and `examples/` directories to these canonical sources; it no longer has a separately edited engine copy. To establish this arrangement in another local checkout, run `python3 scripts/link-engine-source.py --desktop-root /path/to/radias-emulator/rust`. Existing directories are preserved in a dated backup.

Native builds retain the fixed signal graph, four timbres, six virtual patches, nine FX slots, 24 voices and original controller scheduling. The browser crate enables `web-modular`; `web-polyphony` and `web-expanded` increase voices, timbres, routes and FX buses **only on the WASM target**. Browser FX 3/4 reuse the same effect processors on individual timbre buses. These features are disabled in the original desktop workspace. Firmware, original recordings and reference fixtures remain outside the web build. Changing common source files therefore reaches both builds; pushing the changes to Rustias rebuilds GitHub Pages.

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
| [`radias-web`](crates/radias-web) | Standalone profile and browser-only PCM sampler C ABI for WebAssembly/AudioWorklet |

Domain layers do not depend on the UI or filesystem. Application layers manage domain objects. Infrastructure supplies external data and device adapters; desktop, CLI and the browser compose these layers.

## Verification

```sh
cargo test --workspace --all-features --locked
cargo check --workspace --all-targets --all-features --locked
bash scripts/build-web.sh
node scripts/verify-web.mjs
# Optional local RDL check (reads your file without modifying it):
node scripts/verify-rdl.mjs dist/rustias.wasm /absolute/path/to/backup.rdl
```

Unit tests cover program routing, WAVE formats, source-file guards, audio buffering, panel interaction and firmware-free rendering/release/allocation. The Wasm check renders every supported oscillator mode, Filter2 route and waveshaper type; it also checks live modulation, mono/unison, MIDI Expression, drum edits, PCM playback/processing, all 224 sample checksums, independent PCM amplifier settings and CC7 receive, 128-voice allocation and high-bit release, shared Drum Kit Gain, arbitrary sequence samples, range copying, 128-step timing, whole-chord audition and patch persistence. RDL checks use a synthetic 256-program/32-kit bank to verify control mapping, source retention, missing-source muting, standalone/partial records, malformed files, deduplication, legacy storage migration and atomic quota failure. The real audio-device test is ignored by default because it needs original data and an output device.

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
