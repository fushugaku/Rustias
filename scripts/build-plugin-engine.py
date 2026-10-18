#!/usr/bin/env python3
"""Build the shared DSP as an AOT Rust static library, isolated from reference builds."""
import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--output', required=True)
parser.add_argument('--architectures', default='')
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
env = os.environ.copy()
env.pop('MAKEFLAGS', None)
env.pop('MFLAGS', None)
env.pop('CARGO_MAKEFLAGS', None)
env.pop('CARGO_ENCODED_RUSTFLAGS', None)
env['RUSTFLAGS'] = (env.get('RUSTFLAGS', '') + ' --cfg rustias_plugin').strip()
cargo = 'cargo'
if shutil.which('rustup'):
    cargo = subprocess.check_output(['rustup', 'which', 'cargo'], text=True).strip()
    env.setdefault('RUSTC', subprocess.check_output(['rustup', 'which', 'rustc'], text=True).strip())
targets = []
if platform.system() == 'Darwin':
    env['MACOSX_DEPLOYMENT_TARGET'] = '12.0'
    for arch in (args.architectures.split(';') if args.architectures else [platform.machine()]):
        targets.append({'arm64': 'aarch64-apple-darwin', 'x86_64': 'x86_64-apple-darwin'}[arch])
elif platform.system() == 'Windows':
    targets = ['x86_64-pc-windows-msvc']
elif platform.system() == 'Linux':
    targets = [{'x86_64': 'x86_64-unknown-linux-gnu',
                'aarch64': 'aarch64-unknown-linux-gnu'}[platform.machine()]]
else:
    raise SystemExit('Plugin builds support macOS, Windows and Linux')
target_dir = root / 'output/plugin/native-cargo'
libraries = []
for target in targets:
    subprocess.run([cargo, 'build', '--release', '--locked', '--manifest-path',
                    str(root / 'plugin/engine/Cargo.toml'), '--target', target,
                    '--target-dir', str(target_dir)], cwd=root, env=env, check=True)
    name = 'rustias_plugin_engine.lib' if 'windows' in target else 'librustias_plugin_engine.a'
    libraries.append(target_dir / target / 'release' / name)
output = Path(args.output).resolve()
output.parent.mkdir(parents=True, exist_ok=True)
if len(libraries) > 1:
    subprocess.run(['lipo', '-create', *map(str, libraries), '-output', str(output)], check=True)
else:
    shutil.copyfile(libraries[0], output)
print('Native Rust DSP:', ', '.join(targets), '->', output)
