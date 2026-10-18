Native Rustias instrument: the same Rust synthesis engine as the browser version, compiled to native machine code. No firmware or WASM audio runtime required.

This update removes native 128-frame pre-render queues that caused CPU bursts
with multiple AU/VST3/CLAP instances and short DAW buffers. Rendering follows the
host's requested frames while retaining the existing modulation/FX controller
quantum and DSP arithmetic. PCM buffers are reused, the sampler visits occupied
voice slots, and editor telemetry runs at 20 Hz only while its window is visible.
Reported plugin latency now reflects the actual output path.

Native PCM before/after matches bit for bit across 125 scenarios and 1,047,680
stereo frames. Browser PCM also remains byte-identical across 279 scenarios.
Streaming checks cover 1/17/32/64/128/257/512-frame buffers at 44.1/48/96 kHz,
held modulation and macro edits, recording and four concurrent instances.

| System | Installer | Formats |
| --- | --- | --- |
| macOS 12+, Apple Silicon / Intel | `.pkg` | AU, VST3, CLAP |
| Windows 10/11 x64 | `Setup.exe` | VST3, CLAP |
| Linux x86_64, X11 / XWayland | `.run` or Debian/Ubuntu `.deb` | VST3, CLAP |

The instrument includes eight timbres, 128 voices, four oscillators, five Wavefolder shapes, expanded filters, native modulation sequencers, Poly motion and effect chains. Use the DAW for note sequencing.

- Quit the DAW before installation, then reopen and rescan plugins.
- Windows Setup includes the Microsoft Visual C++ runtime and the WebView2 bootstrapper; installing a missing WebView2 runtime requires internet access. The installer provides format selection and uninstall support.
- Linux: run `bash Rustias-Linux-x86_64-*.run` to install for your account. Requires Python 3, WebKitGTK 4.1 (2.40+) and X11/XWayland. For Debian/Ubuntu use `sudo apt install ./Rustias-Linux-amd64-*.deb` to install system-wide with dependencies.
- macOS bundles use ad-hoc signatures. These downloads have no Developer ID notarization or Windows Authenticode signature.
- Verify downloads with the attached `SHA256SUMS.txt`.

All builds pass offline native/WASM PCM and state checks. macOS also runs AU validation; all three systems run Steinberg's VST3 validator. Installer tests exercise actual payloads. Audio tests render to discarded memory.

---

Нативный Rustias для macOS, Windows и Linux. Звук считает общий Rust-движок, без прошивки и без WASM в аудиотракте.

Обновление убирает предварительный расчёт 128 сэмплов в нативных плагинах,
который создавал пики CPU при нескольких экземплярах и коротких буферах DAW.
Аудиобуферы переиспользуются, самплер обходит занятые голоса, телеметрия работает
с частотой 20 Гц только при видимом редакторе. Частота модуляции и DSP-арифметика
сохранены, заявленная задержка соответствует аудиотракту.

Нативный PCM до и после совпал побитово в 125 сценариях — 1 047 680 стереосэмплов.
Проверены буферы 1–512 сэмплов при 44,1/48/96 кГц, изменения MIDI/макросов внутри
блока, сохранение модуляции, запись и четыре одновременных экземпляра.

Скачайте установщик для своей системы из таблицы выше. Перед установкой закройте DAW, после — запустите её и обновите список плагинов. В Windows можно выбрать VST3 и CLAP; недостающие компоненты Microsoft устанавливаются автоматически. Для Linux нужны Python 3, WebKitGTK 4.1 версии 2.40+ и X11/XWayland.

Сохраняются пользовательские библиотеки и записи. Модуляционные секвенсоры, включая Poly motion, работают в Rust; нотную последовательность задаёт DAW.
