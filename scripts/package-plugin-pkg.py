#!/usr/bin/env python3
"""Build a native macOS Installer product from the already validated plugins."""
import argparse
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
FORMATS = (
    ('au', 'component', 'Components', 'Audio Unit (AU)', 'Logic Pro, GarageBand'),
    ('vst3', 'vst3', 'VST3', 'VST3', 'Ableton Live, Cubase, REAPER'),
    ('clap', 'clap', 'CLAP', 'CLAP', 'Bitwig Studio, REAPER'),
)
IDENTIFIER = 'io.github.fushugaku.rustias.pkg'


def run(*args):
    subprocess.run([str(arg) for arg in args], check=True)


def configure_bundles(bundles):
    # Prevent Installer's relocation search from finding a development checkout
    # or a backed-up plugin. Upgrade atomically replaces the entire old bundle.
    for bundle in bundles:
        bundle.update(BundleIsRelocatable=False, BundleIsVersionChecked=True,
                      BundleHasStrictIdentifier=True, BundleOverwriteAction='upgrade')
        configure_bundles(bundle.get('ChildBundles', []))


def build(source, output, identity):
    versions = set()
    for _, extension, _, _, _ in FORMATS:
        bundle = source / f'Rustias.{extension}'
        with (bundle / 'Contents/Info.plist').open('rb') as f:
            info = plistlib.load(f)
        versions.add(info['CFBundleShortVersionString'])
        run('codesign', '--verify', '--deep', '--strict', bundle)
        binary = bundle / 'Contents/MacOS' / info['CFBundleExecutable']
        arch = subprocess.check_output(['lipo', '-archs', str(binary)], text=True).split()
        if set(arch) != {'arm64', 'x86_64'}:
            raise SystemExit(f'{bundle}: a universal arm64/x86_64 build is required')
    if len(versions) != 1:
        raise SystemExit('AU, VST3 and CLAP versions must match')
    version = versions.pop()
    output = output or ROOT / 'output/plugin/packages' / f'Rustias-macOS-{version}.pkg'
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='rustias-pkg-') as temporary:
        work = Path(temporary)
        distribution = ET.Element('installer-gui-script', minSpecVersion='2')
        ET.SubElement(distribution, 'title').text = f'Rustias {version}'
        ET.SubElement(distribution, 'product', id=IDENTIFIER, version=version)
        ET.SubElement(distribution, 'options', customize='allow',
                      **{'require-scripts': 'false', 'hostArchitectures': 'arm64,x86_64'})
        # Match the previous per-user Install.command and avoid shadowing it with
        # a second system-wide copy. No scripts, sudo or fixed username required.
        ET.SubElement(distribution, 'domains', enable_anywhere='false',
                      enable_currentUserHome='true', enable_localSystem='false')
        check = ET.SubElement(distribution, 'volume-check')
        allowed = ET.SubElement(check, 'allowed-os-versions')
        ET.SubElement(allowed, 'os-version', min='12.0')
        ET.SubElement(distribution, 'welcome', file='Welcome.html', **{'mime-type': 'text/html'})
        ET.SubElement(distribution, 'conclusion', file='Conclusion.html', **{'mime-type': 'text/html'})
        choices = ET.SubElement(distribution, 'choices-outline')
        for name, extension, folder, title, description in FORMATS:
            destination = Path('Library/Audio/Plug-Ins') / folder / f'Rustias.{extension}'
            payload = work / name / 'root'
            (payload / destination.parent).mkdir(parents=True)
            run('ditto', source / destination.name, payload / destination)
            components = work / name / 'components.plist'
            run('pkgbuild', '--analyze', '--root', payload, components)
            with components.open('rb') as f:
                definitions = plistlib.load(f)
            configure_bundles(definitions)
            with components.open('wb') as f:
                plistlib.dump(definitions, f)
            package = f'{name}.pkg'
            package_id = f'{IDENTIFIER}.{name}'
            run('pkgbuild', '--root', payload, '--component-plist', components,
                '--identifier', package_id, '--version', version,
                '--install-location', '/', '--ownership', 'recommended', work / package)
            ET.SubElement(choices, 'line', choice=name)
            choice = ET.SubElement(distribution, 'choice', id=name, title=title,
                                   description=description, start_selected='true')
            ET.SubElement(choice, 'pkg-ref', id=package_id)
            ET.SubElement(distribution, 'pkg-ref', id=package_id,
                          version=version, onConclusion='none').text = package
        xml = work / 'Distribution.xml'
        ET.indent(distribution)
        ET.ElementTree(distribution).write(xml, encoding='utf-8', xml_declaration=True)
        # Write to a new file and replace only after productbuild succeeds.
        built = work / output.name
        signing = ['--sign', identity] if identity else []
        run('productbuild', '--distribution', xml, '--package-path', work,
            '--resources', ROOT / 'plugin/installer', '--identifier', IDENTIFIER,
            '--version', version, *signing, built)
        run('python3', ROOT / 'scripts/verify-plugin-pkg.py', built)
        run('ditto', built, output)
    print(f'Installer: {output}')
    print('Installer signature:', identity or 'unsigned (no Developer ID Installer identity configured)')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundles', type=Path, required=True,
                        help='Directory containing the signed Rustias.component/.vst3/.clap bundles')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--sign', default=os.environ.get('RUSTIAS_INSTALLER_IDENTITY', ''))
    args = parser.parse_args()
    build(args.bundles.resolve(), args.output.resolve() if args.output else None, args.sign)
