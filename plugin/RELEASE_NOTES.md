Native Rustias instrument: the same Rust synthesis engine as the browser version, compiled to native machine code. No firmware or WASM audio runtime required.

Version 0.2.6 adds an independent Type dropdown to EG1, EG2 and EG3:
RADIAS (default), Q ADSR, Q ADS1DS2R, Q One Shot, Q Loop S1S2 and Q Loop All.
Extended modes expose Attack Level, Decay 2 and Sustain 2. Q modes have their
own time scale, an instantaneous zero Attack and a fast initial decay with a
smooth tail. One Shot completes even after a short key press; looping modes
stop on note-off. The normal per-sample gain smoothing remains in place.

Programs, timbre sounds and sample profiles retain the settings. Synth voices,
drum instruments, PCM and Build use the same native Rust envelope implementation;
macros and modulation sequencers, including per-voice Poly motion, can target it.
Older programs default to RADIAS.

These are firmware-free models of the Q manual's stage behavior. Rates and
curves have not been calibrated against Q hardware. RADIAS retains its original
calculation: before/after PCM matched bit for bit in 279 scenarios / 3,428,352
stereo frames. Validation includes six envelope lifecycle tests, the complete
WASM suite, 131 native/browser PCM and state scenarios, native host/editor
checks, strict Apple auval and Steinberg VST3 validation. All audio checks run
in discarded memory without playback.

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

Версия 0.2.6 добавляет отдельный dropdown Type для EG1, EG2 и EG3:
RADIAS по умолчанию, Q ADSR, Q ADS1DS2R, Q One Shot, Q Loop S1S2 и Q Loop All.
В расширенных режимах доступны Attack Level, Decay 2 и Sustain 2. У Q своя
шкала времени, мгновенная нулевая атака и быстрый начальный спад с плавным
хвостом. One Shot проходит все стадии независимо от длительности нажатия;
циклические режимы выходят в Release при отпускании ноты. Сглаживание усиления
на уровне аудиосэмплов сохранено.

Настройки сохраняются в программах, тембрах и звуках сэмплов. Синтез, Drum Kit,
PCM и Build используют одну реализацию на Rust; параметры доступны макросам и
мод-секвенсорам, включая Poly. Старые программы получают режим RADIAS.

Это самостоятельные модели режимов из руководства Q. Точные времена и кривые
пока не откалиброваны по железу. Расчёт RADIAS сохранён: PCM побитно совпал с
предыдущей сборкой в 279 сценариях — 3 428 352 стереокадра. Проверены жизненный
цикл огибающих, весь WASM, 131 сценарий совпадения нативного движка и браузера,
хост и редактор, строгий auval и валидатор VST3. Все аудиопроверки выполнены в
памяти без воспроизведения.

Скачайте установщик для своей системы из таблицы выше. Перед установкой закройте DAW, после — запустите её и обновите список плагинов. В Windows можно выбрать VST3 и CLAP; недостающие компоненты Microsoft устанавливаются автоматически. Для Linux нужны Python 3, WebKitGTK 4.1 версии 2.40+ и X11/XWayland.

Сохраняются пользовательские библиотеки и записи. Модуляционные секвенсоры, включая Poly motion, работают в Rust; нотную последовательность задаёт DAW.
