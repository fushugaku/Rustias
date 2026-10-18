Native Rustias instrument: the same Rust synthesis engine as the browser version, compiled to native machine code. No firmware or WASM audio runtime required.

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

Скачайте установщик для своей системы из таблицы выше. Перед установкой закройте DAW, после — запустите её и обновите список плагинов. В Windows можно выбрать VST3 и CLAP; недостающие компоненты Microsoft устанавливаются автоматически. Для Linux нужны Python 3, WebKitGTK 4.1 версии 2.40+ и X11/XWayland.

Сохраняются пользовательские библиотеки и записи. Модуляционные секвенсоры, включая Poly motion, работают в Rust; нотную последовательность задаёт DAW.
