#!/usr/bin/env python3
"""Inspect the real Installer payload without installing or opening audio."""
import argparse
from pathlib import Path
import plistlib
import subprocess
import tempfile
import xml.etree.ElementTree as ET


def verify(package):
    with tempfile.TemporaryDirectory(prefix='rustias-pkg-check-') as directory:
        expanded = Path(directory) / 'expanded'
        subprocess.run(['pkgutil', '--expand-full', str(package), str(expanded)], check=True)
        distribution = ET.parse(expanded / 'Distribution').getroot()
        domains = distribution.find('domains')
        assert domains is not None and domains.attrib == {
            'enable_anywhere': 'false', 'enable_currentUserHome': 'true', 'enable_localSystem': 'false'}
        version = distribution.find('product').get('version')
        choices = distribution.findall('choice')
        assert {choice.get('id') for choice in choices} == {'au', 'vst3', 'clap'}
        formats = {'au': ('component', 'Components'), 'vst3': ('vst3', 'VST3'), 'clap': ('clap', 'CLAP')}
        for name, (extension, folder) in formats.items():
            component = expanded / f'{name}.pkg'
            info = ET.parse(component / 'PackageInfo').getroot()
            assert info.get('identifier') == f'io.github.fushugaku.rustias.pkg.{name}'
            assert info.get('version') == version and info.get('install-location') == '/'
            assert info.find('scripts') is None, 'Home installation must not execute privileged scripts'
            payload = component / 'Payload'
            bundle = payload / 'Library/Audio/Plug-Ins' / folder / f'Rustias.{extension}'
            with (bundle / 'Contents/Info.plist').open('rb') as f:
                metadata = plistlib.load(f)
            assert metadata['CFBundleShortVersionString'] == version
            relocations = info.findall('.//relocate/bundle')
            assert not relocations, 'Plugins must stay at their standard path, not in development checkouts'
            upgrades = info.findall('.//upgrade-bundle/bundle')
            assert any(p.get('id') == metadata['CFBundleIdentifier'] for p in upgrades), f'Atomic replacement missing: {ET.tostring(info, encoding="unicode")}'
            subprocess.run(['codesign', '--verify', '--deep', '--strict', str(bundle)], check=True)
            binary = bundle / 'Contents/MacOS' / metadata['CFBundleExecutable']
            architectures = subprocess.check_output(['lipo', '-archs', str(binary)], text=True).split()
            assert set(architectures) == {'arm64', 'x86_64'}
            assert not list(bundle.rglob('Resources/rustias.wasm')), 'No WASM audio runtime'
        for language in ['en', 'ru']:
            for name in ['Welcome.html', 'Conclusion.html']:
                assert (expanded / 'Resources' / f'{language}.lproj' / name).is_file()
    # These are read-only Installer queries, using the actual current user domain.
    domains = subprocess.check_output(['installer', '-dominfo', '-pkg', str(package)], text=True)
    assert 'CurrentUserHomeDirectory' in domains and 'LocalSystem' not in domains
    choices = subprocess.check_output(['installer', '-showChoiceChangesXML', '-pkg', str(package),
                                       '-target', 'CurrentUserHomeDirectory'], text=True)
    parsed = plistlib.loads(choices.encode())
    selected = {c['choiceIdentifier']: c['attributeSetting'] for c in parsed if c['choiceAttribute'] == 'selected'}
    assert selected == {'au': 1, 'vst3': 1, 'clap': 1}, selected
    print(f'PKG {version}: AU/VST3/CLAP payloads, universal code signatures, user domain, choices and atomic upgrades verified.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', type=Path)
    verify(parser.parse_args().package.resolve())
