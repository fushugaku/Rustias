Native Rustias instrument: the same Rust synthesis engine as the browser version, compiled to native machine code. No firmware or WASM audio runtime required.

Version 0.2.5 removes JSON round trips from MIDI/display snapshots, telemetry
and native FX/chain modulation. Snapshots and recording chunks reuse prepared
buffers; macro/lane links and modulation scratch are retained. Message workers
sleep until notified. Final MIDI/CC7 and full corrective states survive queue
pressure and Stop without another audio callback.

A silent Apple M5 Max test at 48 kHz / 128 frames compared frozen 0.2.4 and
0.2.5 in ABBA order. With two parallel AU, four sources, two MS-20 filters and
continuous controllers, mean callback time fell from 274–284 to 226–230 µs;
P99 fell from 684–686 to 415–417 µs. A separate sequential eight-runtime
MIDI/telemetry test reduced budget misses from 474/6,144 to 0/6,144.
Static AU mean times differed by less than 3%. These are workload-specific
ordinary offline-host measurements, not a universal real-time guarantee.

PCM and state match frozen 0.2.4 in 210 adapter scenes plus 12 control timelines
on both Mac architectures. The browser's 279 scenes / 3,428,352 stereo frames
also match. Original synthesis arithmetic, filter quality, controller quantum
and reference desktop profile are unchanged. All checks remain silent.

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

Версия 0.2.5 убирает JSON из снимков MIDI-состояния, телеметрии и модуляции
эффектов/цепочек. Снимки и блоки записи переиспользуют готовые буферы; связи
макросов/линий и рабочая память модуляции подготавливаются заранее. Фоновые
потоки ждут уведомлений. Последние MIDI/CC7 и полное корректирующее состояние
доходят при переполнении пула и после Stop без следующего аудиоблока.

В двух параллельных AU на Apple M5 Max, 48 кГц / 128 сэмплов, с четырьмя
источниками, MS-20 и потоком контроллеров, среднее время снизилось с 274–284
до 226–230 мкс, P99 — с 684–686 до 415–417 мкс. В отдельном последовательном
тесте восьми нативных экземпляров превышения бюджета снизились с 474 из 6 144
до нуля. Для статичных AU разница среднего времени — меньше 3%. Результат
зависит от патча и планировщика ОС.

Звук и состояние совпали с замороженной 0.2.4: 210 сценариев адаптера и 12
сценариев управления на Apple Silicon и Intel. Браузер сохранил PCM во всех
279 сценариях — 3 428 352 стереокадра. Арифметика синтеза, качество фильтров,
квант контроллеров и эталонный профиль исходного движка сохранены.
Все аудиопроверки выполнены в памяти, без вывода звука.

Скачайте установщик для своей системы из таблицы выше. Перед установкой закройте DAW, после — запустите её и обновите список плагинов. В Windows можно выбрать VST3 и CLAP; недостающие компоненты Microsoft устанавливаются автоматически. Для Linux нужны Python 3, WebKitGTK 4.1 версии 2.40+ и X11/XWayland.

Сохраняются пользовательские библиотеки и записи. Модуляционные секвенсоры, включая Poly motion, работают в Rust; нотную последовательность задаёт DAW.
