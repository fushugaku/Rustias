#!/usr/bin/env python3
"""Per-user VST3/CLAP installer. User patches and recordings are never removed."""
import argparse
import datetime
from pathlib import Path
import shutil
import subprocess
import uuid

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--source', type=Path, default=Path(__file__).resolve().parent)
parser.add_argument('--prefix', type=Path, default=Path.home(), help='User home containing .vst3 and .clap')
parser.add_argument('--uninstall', action='store_true')
args = parser.parse_args()
source, prefix = args.source.resolve(), args.prefix.expanduser().resolve()
targets = [('Rustias.vst3', prefix/'.vst3/Rustias.vst3'),
           ('Rustias.clap', prefix/'.clap/Rustias.clap'),
           ('Rustias.resources', prefix/'.clap/Rustias.resources')]
if args.uninstall:
    for _, destination in targets:
        if destination.is_dir() and not destination.is_symlink(): shutil.rmtree(destination)
        elif destination.exists() or destination.is_symlink(): destination.unlink()
    print('Rustias plugins removed. User libraries and recordings retained.')
else:
    for name, _ in targets:
        if not (source/name).exists(): raise SystemExit(f'Missing plugin payload: {name}')
    dependencies=subprocess.run(['ldd',str(source/'Rustias.clap')],capture_output=True,text=True)
    if dependencies.returncode or 'not found' in dependencies.stdout:
        raise SystemExit('Install WebKitGTK 4.1 (2.40+) and GTK3, or use the Debian/Ubuntu package.\n'+dependencies.stdout+dependencies.stderr)
    backup = prefix/'.local/state/rustias/plugin-backups'/(datetime.datetime.now().strftime('%Y%m%d-%H%M%S')+'-'+uuid.uuid4().hex[:8])
    staged, installed, moved = [], [], []
    try:
        for name, destination in targets:
            destination.parent.mkdir(parents=True, exist_ok=True)
            stage=destination.parent/('.rustias-stage-'+uuid.uuid4().hex)
            if (source/name).is_dir(): shutil.copytree(source/name, stage)
            else: shutil.copy2(source/name, stage)
            staged.append((stage,destination,name))
        for stage,destination,name in staged:
            if destination.exists() or destination.is_symlink():
                backup.mkdir(parents=True, exist_ok=True)
                destination.rename(backup/name);moved.append((backup/name,destination))
            stage.rename(destination);installed.append(destination)
    except BaseException:
        for destination in reversed(installed):
            if destination.is_dir(): shutil.rmtree(destination)
            else: destination.unlink()
        for previous,destination in reversed(moved): previous.rename(destination)
        raise
    finally:
        for stage,_,_ in staged:
            if stage.is_dir(): shutil.rmtree(stage)
            elif stage.exists(): stage.unlink()
    print('Rustias VST3 and CLAP installed for:',prefix)
    if moved: print('Previous versions:',backup)
    print('Restart the DAW and rescan plugins. Requires WebKitGTK 4.1 and X11/XWayland.')
