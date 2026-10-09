#!/usr/bin/env python3
"""Copy the complete, unmodified Rob Roy TR-909 set at a pinned revision."""
from collections import Counter
from io import BytesIO
from pathlib import Path
import hashlib
import json
import urllib.request
import wave
import zipfile

ROOT = Path(__file__).resolve().parents[1]
REVISION = "475cc3314fe06f6d1af02e9790ad9707c1f2b26b"
SOURCE = "https://github.com/fluid-music/open-drums"
PREFIX = f"open-drums-{REVISION}/tr-909/TR909all/"
URL = f"https://codeload.github.com/fluid-music/open-drums/zip/{REVISION}"
CATEGORIES = {
    "BT": ("kick", "Kick"), "ST": ("snare", "Snare"),
    "LT": ("low-tom", "Low tom"), "MT": ("mid-tom", "Mid tom"),
    "HT": ("high-tom", "High tom"), "RIM": ("rim", "Rim"),
    "HANDCLP": ("clap", "Clap"), "HHC": ("closed-hat", "Closed hat"),
    "HHO": ("open-hat", "Open hat"), "CSH": ("cymbal", "Crash"),
    "RIDE": ("ride", "Ride"), "OPCL": ("hat-transition", "Hat open→closed"),
    "CLOP": ("hat-transition", "Hat closed→open"),
}

def category(filename):
    return next(value for prefix, value in CATEGORIES.items() if filename.startswith(prefix))

def main():
    with urllib.request.urlopen(URL) as response:
        archive = zipfile.ZipFile(BytesIO(response.read()))
    files = sorted(info for info in archive.namelist() if info.startswith(PREFIX) and not info.endswith("/"))
    wavs = [name for name in files if name.endswith(".WAV")]
    assert len(wavs) == 160, "Keep the entire original set; its license forbids deleting samples."
    assert len(files) == 161 and any(name.endswith("TR909SET.TXT") for name in files)
    destination = ROOT / "web/samples/tr-909"
    destination.mkdir(parents=True, exist_ok=True)
    for name in files:
        filename = name.removeprefix(PREFIX)
        assert Path(filename).name == filename
        (destination / filename).write_bytes(archive.read(name))
    counters = Counter()
    samples = []
    for name in wavs:
        filename = name.removeprefix(PREFIX)
        kind, label = category(filename)
        counters[label] += 1
        data = archive.read(name)
        with wave.open(BytesIO(data)) as audio:
            samples.append({
                "id": f"909:{Path(filename).stem.lower()}",
                "name": f"909 {label} {counters[label]:02}", "category": kind,
                "file": f"tr-909/{filename}", "sourceFile": f"tr-909/TR909all/{filename}",
                "duration": round(audio.getnframes() / audio.getframerate(), 6),
                "sampleRate": audio.getframerate(), "channels": audio.getnchannels(),
                "sha256": hashlib.sha256(data).hexdigest(),
            })
    path = ROOT / "web/samples/manifest.json"
    previous = json.loads(path.read_text())
    existing = [sample for sample in previous["samples"] if not sample["id"].startswith("909:")]
    banks = [bank for bank in previous.get("banks", []) if bank["id"] != "909"] or [{
        "id": "808", "name": "TR-808", "count": 64, "license": previous["license"],
        "author": previous["author"], "source": previous["source"],
        "sourceCommit": previous["sourceCommit"], "licenseFile": "LICENSE.txt",
    }]
    banks.append({
        "id": "909", "name": "TR-909", "count": 160,
        "license": "LicenseRef-Rob-Roy-Free-Redistribution",
        "author": "Jason Baker / Rob Roy Recordings", "source": SOURCE,
        "sourceCommit": REVISION, "licenseFile": "tr-909/TR909SET.TXT",
        "licenseSha256": hashlib.sha256((destination / "TR909SET.TXT").read_bytes()).hexdigest(),
    })
    path.write_text(json.dumps({"version": 2, "banks": banks, "samples": existing + samples}, indent=2, ensure_ascii=False) + "\n")
    print(f"Bundled {len(samples)} unmodified TR-909 WAVs; {len(existing) + len(samples)} samples total.")

if __name__ == "__main__":
    main()
