# Rustias

[English](README.md) · **Русский**

Rustias — Rust-часть проекта эмуляции Korg RADIAS: нативный синтезатор, интерпретатор оригинальной прошивки, desktop-интерфейс и консольный отладчик. Девять крейтов собраны в один Cargo workspace.

Проект находится в разработке. Отдельные алгоритмы и записанные сценарии сверяются с исходным C++-эмулятором; эти проверки не доказывают полное совпадение с аппаратным RADIAS.

## Два режима работы

- **Нативный синтез** (`radias-synth-*`) исполняет восстановленные алгоритмы напрямую в Rust: осцилляторы, фильтры, огибающие, усилитель, панораму, LFO, модуляцию и управление голосами. Desktop использует пул из 24 голосов и четыре независимо управляемых тембра. Звуковые сэмплы создаются в callback аудиоустройства.
- **Оригинальная прошивка** (`radias-domain`, `radias-application`) исполняется в моделях SH3 и C55 с памятью, периферией и шинами платы. Этот режим нужен для исследования, трассировки и сравнения. Интерпретатор работает медленнее реального времени.

Desktop использует нативный режим, когда доступны необходимые файлы и аудиоустройство. CLI работает с интерпретатором прошивки.

## WebAssembly

[Открыть синтезатор](https://fushugaku.github.io/Rustias/).

Браузерная версия работает без прошивки. Общий Rust-генератор из `radias-synth-infrastructure::synthesizer` создаёт звук внутри Web Audio `AudioWorklet`; JavaScript передаёт команды и выводит сэмплы. Веб-сборка не подключает SH3/C55-интерпретатор, SYS, RDL, PCM или записи звука.

Под панелью исполнения находится полный редактор нативного синтеза. Интерфейс и Rust проверяют параметры по общей [схеме](crates/radias-synth-infrastructure/src/parameters.json).

| Раздел | Доступные параметры |
| --- | --- |
| OSC1 | Saw, Pulse, Triangle, Sine, Noise, Formant; Waveform, Cross, осцилляторный Unison и VPM; CTRL1/CTRL2. Noise/Formant работают в режиме Waveform. |
| OSC2 / Mixer | Четыре формы; Normal, Ring, Sync и Ring+Sync; semitone/fine tune; уровни OSC1, OSC2 и шума |
| Filters | Морфинг типа Filter1, cutoff, resonance, глубина EG1 и key tracking; Single, Serial, Parallel и Individual; Filter2 LP/HP/BP/Comb, link и собственные параметры cutoff, resonance, EG1 и клавишного трекинга |
| Drive / Waveshaper | Drive, Hard Clip, Decimator, OctSaw, MultiTri, MultiSin, четыре SubOSC, Pickup и Level Boost; глубина и положение |
| EG1 / EG2 / EG3 | Независимые ADSR, восемь кривых, чувствительность уровня/времени к velocity и key tracking |
| Amplifier | Уровень, панорама, key tracking, смещение уровня, source gain/Expression и MIDI volume; автоматический gain bank для Unison |
| LFO1 / LFO2 | Форма, shape, частота, фаза, Free/Timbre/Voice sync, tempo sync, деление и смещение скорости |
| Virtual patches | Шесть маршрутов source/destination/intensity, ручные смещения и обратные связи между патчами |
| Voice / Pitch | Четыре тембра, 24 голосовых слота, Mono/Poly, retrigger/priority, инструментальный Unison из 2–8 голосов, detune/spread, portamento с time/curve/CC65, transpose, fine tune, bend и vibrato |
| Tuning / MIDI | Одиннадцать строев, тоника, master tune и custom cents; собственный или Global MIDI-канал тембра, клавишные диапазоны и receive flags; note on/off, bend, wheel, CC11 Expression, CC64 sustain, CC65 и all notes/sound off |
| Drum Kit | 16 независимо редактируемых инструментов синтеза, тембр-владелец, общий уровень/pan/transpose, trigger note и exclusive group; отпускание pad сохраняет исходную ноту |

Нажмите **Start audio** для воспроизведения. Вкладки редактора меняют параметры выбранного тембра. При включённом **Drum mode** выберите тембр-владелец и инструмент: редактор синтеза переключится на этот инструмент, а клавиатура — на 16 drum pads. Выбор другого инструмента сохраняет звучащие голоса. **Save** и **Open** сохраняют и открывают всю программу — четыре тембра и 16 инструментов — в локальном JSON-файле.

Громкость прослушивания отделена от уровня тембра. Регуляторы поддерживают вертикальное перетаскивание, точную настройку с Shift, колесо и стрелки. Pads работают с несколькими касаниями; потеря фокуса отпускает удерживаемые ноты. Редактор адаптирован для мобильного экрана.

Автономный профиль передаёт каждому нативному контроллеру таблицы, рассчитанные по математическим формулам: фильтры, Comb, огибающие, LFO/tempo, модуляция, шум, строй, pan и группы голосов. Интерполяционная коррекция waveform остаётся нулевой. Это самостоятельные данные для общих DSP-алгоритмов; они не копируют заводской ROM и не воспроизводят заводские программы. PCM/Audio In, эффекты FXD03, vocoder и секвенсоры ещё не завершены в нативном движке и не представлены как работающие регуляторы.

Собрать и открыть локально:

```sh
rustup target add wasm32-unknown-unknown
bash scripts/build-web.sh
python3 -m http.server 8080 --directory dist
```

Откройте `http://localhost:8080`. Для AudioWorklet нужен HTTPS или localhost. Скрипт создаёт `dist/` с HTML, JavaScript и `rustias.wasm`; wasm-bindgen, npm и серверная часть не требуются. Нативный поток имеет частоту 48 кГц; Web Audio-адаптер при необходимости пересчитывает его в частоту устройства.

Проверить готовый WASM и AudioWorklet на 48/44,1 кГц можно через `node scripts/verify-web.mjs`. Workflow [pages.yml](.github/workflows/pages.yml) тестирует автономный профиль, собирает WASM и публикует `dist/` в GitHub Pages при push в `main`. Источник публикации в Settings → Pages должен быть **GitHub Actions**.

## Сборка desktop и CLI

Workspace использует Rust edition 2024. Перенос проверен с Rust/Cargo **1.97.1** на macOS. `Cargo.lock` фиксирует зависимости; основные desktop-библиотеки — eframe/egui 0.36.2, CPAL 0.18.2 и midir 0.11.0.

```sh
git clone https://github.com/fushugaku/Rustias.git
cd Rustias

# Консольный эмулятор
cargo build --release --locked -p radias-cli

# Нативное приложение
cargo build --release --locked -p radias-desktop
```

Бинарные файлы появятся в `target/release/radias-rust` и `target/release/radias-desktop`. Для сборки всего workspace:

```sh
cargo build --release --locked --workspace
```

Сборка CLI не подключает аудио- и MIDI-библиотеки. Desktop требует оконного окружения и системных библиотек, используемых CPAL, midir и eframe. Другие операционные системы при переносе не проверялись.

## Данные для запуска

Сборка и обычные unit-тесты обходятся без оригинальных образов. Для desktop-режимов и интерпретатора нужны внешние файлы; браузерный автономный профиль обходится без них. В этом репозитории находятся Rust-исходники, Cargo-манифесты и встроенное описание панели; прошивка, банки, подготовленные программы и исследовательские записи остаются отдельно.

| Файл относительно каталога данных | Для чего нужен |
| --- | --- |
| `firmware/RADIAS_SYS_0200.bin` | Образ SYS 2.00; обязателен для интерпретатора и извлечения таблиц нативного синтеза |
| `firmware/Radias-backup.rdl` | Банк программ; обязателен для нативного desktop-режима, в CLI подключается через `--backup` |
| `firmware/dsp-master-host-stream.bin` | Поток загрузки Master DSP с таблицами для нативного синтеза |
| `assets/native-va/saw.json`, `pulse.json`, `triangle.json`, `sine.json` | Подготовленные параметры нативных голосов; все четыре файла находятся в `assets/native-va/` |
| `assets/native-va/filter-controls.json` | Карта управляющих параметров фильтра для нативного режима |
| `runs/alternative-pcm/alternative-pcm.bin` | Необязательный альтернативный PCM-банк для интерпретатора |

Каталог данных может быть корнем этого репозитория или отдельной папкой с той же структурой. Для продолжения работы с полным исходным проектом передайте его каталог через `--workspace`; копировать данные не требуется.

## Desktop

```sh
# Данные расположены в корне Rustias
cargo run --release --locked -p radias-desktop

# Данные расположены в отдельном каталоге
cargo run --release --locked -p radias-desktop -- \
  --workspace /absolute/path/to/radias-data

# Проверка интерфейса в узком окне без аудиовыхода
cargo run --release --locked -p radias-desktop -- \
  --workspace /absolute/path/to/radias-data --no-audio --size 390x780
```

Панель содержит регуляторы и переключатели RADIAS, выбор программ и экранную клавиатуру. MIDI поступает через виртуальный вход `RADIAS Rust`. Внешние MIDI-байты направляются в выбранный движок.

`--workspace` задаёт каталог исходных данных и рабочих файлов. `--no-audio` отключает аудиовыход и загрузку нативного аудиодвижка; интерпретатору по-прежнему нужен SYS-образ. `--size WIDTHxHEIGHT` задаёт начальный размер окна.

В режиме прошивки рабочий Flash сохраняется в `runs/rust-desktop/working-flash.bin`, а прослушивание — в `runs/rust-desktop/last-preview.wav` и соседние файлы каналов. Flash сохраняет завершённые записи NOR, но не состояние процессоров, RAM и звучащих голосов. Повторный запуск с тем же каталогом данных использует этот рабочий образ.

## Консольный эмулятор

Пути CLI разрешаются относительно текущего каталога; расположение файлов можно указать явно:

```sh
cargo run --release --locked -p radias-cli -- \
  --firmware /absolute/path/to/radias-data/firmware/RADIAS_SYS_0200.bin \
  --backup /absolute/path/to/radias-data/firmware/Radias-backup.rdl \
  --interactive \
  --dry-audio runs/cli/dry.wav \
  --mix-audio runs/cli/mix
```

Интерактивный режим принимает одну команду на строку и отвечает строками JSON. Пример сеанса:

```text
state
run 5000000
midi 90 3c 64
runframes 4800
midi 80 3c 00
runframes 4800
quit
```

`run` задаёт число шагов интерпретатора, `runframes` — число кадров платы с частотой 48 кГц. Байты `midi` записываются шестнадцатерично; в примере это нажатие и отпускание C4 на первом канале. До отправки нот прошивка должна завершить загрузку; один вызов `run` не гарантирует готовность.

| Опция | Назначение |
| --- | --- |
| `--steps N` | Выполнить заданное число шагов без интерактивного сеанса |
| `--backup-global RDL` | Импортировать только Global-настройки из банка |
| `--pcm-bank BIN` | Подключить банк в native Flash-разметке |
| `--flash-image BIN` | Создать или загрузить отдельный рабочий Flash-образ размером 4 МиБ |
| `--input-wave WAV` | Подать mono/stereo WAVE 48 кГц: PCM16/24/32 или float32 |
| `--input-loop` | Повторять входной WAVE |
| `--dry-audio WAV` | Сохранить сухой звуковой поток |
| `--mix-audio PREFIX` | Сохранить native Master/Slave в `PREFIX-master.wav` и `PREFIX-slave.wav` |
| `--vocoder-audio PREFIX` | Сохранить доступные vocoder-потоки |
| `--dump JSON` | Записать диагностическое состояние |
| `--trace PATH`, `--fxd-trace PATH`, `--fxd-link-trace PATH` | Записать диагностические трассы |

Исходные SYS/RDL/PCM/WAVE защищены от перезаписи через выходные пути и их файловые алиасы. Рабочие данные и записи игнорируются Git.

## Устройство workspace

| Крейт | Ответственность |
| --- | --- |
| [`radias-synth-domain`](crates/radias-synth-domain) | Fixed-point алгоритмы прямого синтеза; `no_std`, без зависимостей |
| [`radias-synth-application`](crates/radias-synth-application) | Рендер голосов, полифония, часы сэмплов, события управления; `no_std` |
| [`radias-synth-infrastructure`](crates/radias-synth-infrastructure) | Общий генератор, автономные таблицы и схема параметров, адаптеры прошивки/RDL и CPAL-выход |
| [`radias-domain`](crates/radias-domain) | SH3/C55, память и периферия платы, NOR, codec, программы и backup; без внешних зависимостей |
| [`radias-application`](crates/radias-application) | Жизненный цикл машины, команды, бюджеты исполнения и политика PCM |
| [`radias-infrastructure`](crates/radias-infrastructure) | WAV, PCM, Flash-файлы, диагностические записи; аудио и MIDI через feature `desktop-io` |
| [`radias-cli`](crates/radias-cli) | Консольное приложение и строковый JSON-протокол |
| [`radias-desktop`](crates/radias-desktop) | egui-интерфейс, панель, клавиатура и соединение с движками |
| [`radias-web`](crates/radias-web) | C ABI автономного профиля для WebAssembly/AudioWorklet |

Доменные слои не зависят от интерфейса и файловой системы. Application-слои управляют доменными объектами; infrastructure подключает внешние данные и устройства. CLI и desktop собирают эти слои в приложения.

## Проверки

Из корня репозитория:

```sh
cargo test --workspace --all-features --locked
cargo check --workspace --all-targets --all-features --locked
bash scripts/build-web.sh
node scripts/verify-web.mjs
```

Unit-тесты проверяют маршрутизацию программ, форматы WAVE, защиту исходных файлов, аудиобуферы, интерфейс и автономный синтез. WASM-проверка воспроизводит все поддерживаемые режимы OSC1, маршруты Filter2 и типы waveshaper; проверяет модуляцию, mono/unison, MIDI Expression, Drum Kit и сохранение всей программы. Тест реального аудиовыхода помечен `ignored`: ему нужны исходные данные и устройство вывода.

Rust-примеры в `crates/*/examples/` сохранены вместе с кодом. Многие `*_parity`-программы читают эталонные записи, полные WAV и состояния из внешнего исследовательского workspace. Такие сравнения требуют соответствующих данных и C++-оракулов исходного проекта; они не входят в обычный запуск unit-тестов. В примерах, которые принимают каталог данных первым аргументом, передавайте его явно.

Сгенерированные декодеры SH3/C55 включены в исходники и собираются обычным Cargo. Для их регенерации нужны C++-источники и генераторы из полного проекта; этот репозиторий содержит готовый Rust-результат.

## Текущие ограничения

- Полное исполнение произвольных RDL-программ и всех сочетаний параметров ещё не завершено.
- Эффекты FXD03, конечный codec/DAC-тракт и аппаратные тайминги требуют дальнейшей проверки. Записи до FXD/codec не следует считать окончательным выходом физического RADIAS.
- Некоторые контроллерные сценарии, политики голосов, секвенсор, vocoder и аппаратные органы управления остаются неполными.
- Режим прошивки может давать underrun при прослушивании; он предназначен для диагностики. Скорость отдельных нативных алгоритмов не подтверждает готовность всего инструмента к работе в реальном времени.
- Заводской PCM-ROM в репозиторий не входит. Альтернативный PCM-банк не воспроизводит отсутствующий заводской банк.

Совпадение с программным эталоном подтверждает только проверенный сценарий и наблюдаемую границу. Полная аппаратная эквивалентность остаётся целью проекта.
