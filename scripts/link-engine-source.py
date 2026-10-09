#!/usr/bin/env python3
"""Link a local desktop workspace to the canonical synthesis source, preserving backups."""
import argparse
from datetime import datetime, timezone
from pathlib import Path
import shutil

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--desktop-root', type=Path, required=True, help='The radias-emulator/rust workspace')
args = parser.parse_args()
canonical = Path(__file__).resolve().parent.parent
desktop = args.desktop_root.resolve()
crates = ['radias-synth-domain', 'radias-synth-application', 'radias-synth-infrastructure']
if desktop == canonical or not (desktop / 'Cargo.toml').is_file():
    parser.error('Choose the separate desktop Rust workspace.')
for name in crates:
    if not (desktop / 'crates' / name / 'Cargo.toml').is_file():
        parser.error(f'Missing desktop crate: {name}')
backup = desktop.parent / ('engine-source-backup-' + datetime.now(timezone.utc).strftime('%Y%m%d-%H%M%S'))
for name in crates:
    crate = desktop / 'crates' / name
    for part in ['src', 'examples']:
        source = canonical / 'crates' / name / part
        target = crate / part
        if not source.is_dir() or target.is_symlink() and target.resolve() == source:
            continue
        if target.exists() or target.is_symlink():
            saved = backup / name / part
            saved.parent.mkdir(parents=True, exist_ok=True)
            target.rename(saved)
        target.symlink_to(source, target_is_directory=True)
    # Metadata remains workspace-local. Keep manifests identical, with optional
    # browser features declared but never selected by the native workspace.
    manifest = crate / 'Cargo.toml'
    source_manifest = canonical / 'crates' / name / 'Cargo.toml'
    if manifest.read_bytes() != source_manifest.read_bytes():
        saved = backup / name / 'Cargo.toml'
        saved.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(manifest, saved)
        shutil.copy2(source_manifest, manifest)
print(f'Both builds use synthesis sources in {canonical / "crates"}.')
if backup.exists():
    print(f'Previous sources preserved in {backup}.')
