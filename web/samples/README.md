# Bundled drum samples

## TR-808

64 unmodified WAV recordings from [Michael Fischer’s TR-808 set](https://github.com/tidalcycles/sounds-tr808-fischer), distributed by Tidal Cycles under CC0-1.0. Recorded by Michael Fischer / Technopolis.

Source revision: `85fbecf1bec32553395625ea659e2a56dfd7c0e1`. See [LICENSE.txt](LICENSE.txt) for the complete CC0 dedication. `manifest.json` records each original filename, SHA-256, category and duration.

The selection contains 12 kicks, 12 snares, one closed hat, five open hats, clap, cowbell, rim, maracas, claves, 12 toms, nine congas and eight cymbals. Audio is copied without modification.

## TR-909

The complete **160-recording TR-909 set** by Jason Baker / Rob Roy Recordings is included in `tr-909/`. Source: [fluid-music/open-drums](https://github.com/fluid-music/open-drums/tree/475cc3314fe06f6d1af02e9790ad9707c1f2b26b/tr-909/TR909all), revision `475cc3314fe06f6d1af02e9790ad9707c1f2b26b`. All original filenames and WAV bytes are unchanged, together with the unmodified [TR909SET.TXT](tr-909/TR909SET.TXT).

This bank is **not CC0**. The original terms allow free copying/distribution of the complete set, prohibit modifying it, and prohibit distributing the samples for profit. These terms apply to the 909 bank separately from the CC0 808 bank.

The set contains 24 kicks, 52 snares, 16 each of low/mid/high toms, two rimshots, two claps, six each of closed hats/open hats/crashes/rides, and eight hat transitions. The combined Source picker exposes 224 recordings. `manifest.json` contains bank-specific licenses, source revisions, original filenames and SHA-256 checksums.

To reproduce the 909 import from the pinned upstream revision:

```sh
python3 scripts/vendor-tr909.py
```
