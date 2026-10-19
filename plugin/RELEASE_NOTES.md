Native Rustias instrument: the same Rust synthesis engine as the browser version, compiled to native machine code. No firmware or WASM audio runtime required.

Version 0.2.4 removes the script VM from the DAW audio path. A live Logic profile
of 0.2.3 showed JavaScriptCore lock and garbage-collector waits in both AU render
threads. Audio, MIDI, transport and parameter updates now call the canonical Rust
engine directly. Outgoing message / recording-array formatting uses a separate
native worker; the audio callback never waits for it. Browser UI and DSP stay shared.

A silent offline stress test based on the observed patch used two parallel AU
instances, four sources, two MS-20 filters, six voices per instance and continuous
wheel/pitch events. At 48 kHz / 128 frames on Apple M5 Max, P99 fell from 3.918 ms
to 0.867 ms; callbacks exceeding the 2.667 ms budget fell from 122/1536 to 0/1536.
These measurements depend on patch and OS scheduling; they are not a universal
real-time guarantee.

The old worklet remains an offline oracle only. Native adapter PCM matches every
bit in 210 scenes and 1,720,320 stereo frames at 44.1/48/96 kHz, including all
filter models, oscillators, 30 effect algorithms, chains, samples and macros.
The existing native/browser PCM, state, streaming, recording and editor checks
remain in place. The original reference engine profile is unchanged.

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

Версия 0.2.4 убирает интерпретатор из аудиотракта DAW. В профиле Logic с 0.2.3
видны ожидания блокировок и сборщика мусора JavaScriptCore в обоих потоках AU.
Теперь аудио, MIDI, транспорт и параметры обращаются к общему Rust-движку
напрямую. Форматирование сообщений и массивов записи вынесено в отдельный
нативный поток, которого аудиопоток не ждёт. Общие DSP и интерфейс сохранены.

В тихом стресс-тесте с двумя параллельными AU, четырьмя источниками, двумя
фильтрами MS-20 и потоком Wheel/Pitch, при 48 кГц / 128 сэмплах P99 снизился
с 3,918 до 0,867 мс. Превышения бюджета 2,667 мс — со 122 из 1536 до 0 из 1536.
Результат зависит от патча и нагрузки ОС.

PCM совпал побитово с прежним адаптером в 210 сценариевх — 1 720 320
стереосэмплов на 44,1/48/96 кГц. Проверены все модели фильтров, осцилляторы,
30 эффектов, цепочки, сэмплы и макросы. Прежний адаптер используется только
в офлайн-проверках; эталонный профиль оригинального движка не менялся.

Скачайте установщик для своей системы из таблицы выше. Перед установкой закройте DAW, после — запустите её и обновите список плагинов. В Windows можно выбрать VST3 и CLAP; недостающие компоненты Microsoft устанавливаются автоматически. Для Linux нужны Python 3, WebKitGTK 4.1 версии 2.40+ и X11/XWayland.

Сохраняются пользовательские библиотеки и записи. Модуляционные секвенсоры, включая Poly motion, работают в Rust; нотную последовательность задаёт DAW.
