#!/usr/bin/env python3
"""Package verified native Linux plugins as DEB and a per-user .run installer."""
import hashlib
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

root=Path(__file__).resolve().parent.parent
build=root/'output/plugin/build'
packages=root/'output/plugin/packages'
packages.mkdir(parents=True,exist_ok=True)
version=re.search(r'project\(RustiasPlugin VERSION ([\d.]+)',(root/'plugin/CMakeLists.txt').read_text())[1]
clap=next(p for p in build.rglob('Rustias.clap') if p.is_file())
vst3=next(p for p in build.rglob('Rustias.vst3') if p.is_dir() and (p/'Contents/x86_64-linux').is_dir())
linked=subprocess.check_output(['ldd',str(clap)],text=True)
assert 'libgtk-' not in linked and 'libwebkit' not in linked, 'GUI libraries must remain in the isolated editor helper'
with tempfile.TemporaryDirectory(prefix='rustias-linux-') as temporary:
    work=Path(temporary);payload=work/'payload';payload.mkdir()
    shutil.copy2(clap,payload/'Rustias.clap')
    shutil.copytree(clap.parent/'Rustias.resources',payload/'Rustias.resources')
    shutil.copytree(vst3,payload/'Rustias.vst3')
    for name in ['README.md','README.ru.md','THIRD_PARTY_NOTICES.txt']: shutil.copy2(root/'plugin'/name,payload/name)
    shutil.copy2(root/'plugin/installer/install-linux.py',payload/'Install.py')
    archive=work/'payload.tar.gz'
    subprocess.run(['tar','-czf',str(archive),'-C',str(payload),'.'],check=True)
    digest=hashlib.sha256(archive.read_bytes()).hexdigest()
    header='''#!/usr/bin/env bash
set -euo pipefail
[ "$(uname -s)/$(uname -m)" = Linux/x86_64 ] || { echo 'This installer requires Linux x86_64.' >&2; exit 1; }
command -v python3 >/dev/null || { echo 'Python 3 is required.' >&2; exit 1; }
rustias_tmp=$(mktemp -d "${TMPDIR:-/tmp}/rustias.XXXXXXXX")
trap 'rm -rf "$rustias_tmp"' EXIT
rustias_line=$(awk '/^__RUSTIAS_PAYLOAD__$/{print NR+1;exit}' "$0")
tail -n +"$rustias_line" "$0" > "$rustias_tmp/payload.tar.gz"
echo 'DIGEST  payload.tar.gz' > "$rustias_tmp/SHA256SUMS"
(cd "$rustias_tmp" && sha256sum --check --status SHA256SUMS)
mkdir "$rustias_tmp/payload"
tar -xzf "$rustias_tmp/payload.tar.gz" -C "$rustias_tmp/payload"
python3 "$rustias_tmp/payload/Install.py" --source "$rustias_tmp/payload" "$@"
exit 0
__RUSTIAS_PAYLOAD__
'''.replace('DIGEST',digest)
    installer=packages/f'Rustias-Linux-x86_64-{version}.run'
    installer.write_bytes(header.encode()+archive.read_bytes());installer.chmod(0o755)
    # Run the actual self-extracting installer twice, including a stale-resource upgrade.
    test_user=work/'test-user'
    user_data=test_user/'.local/share/rustias/plugin/user-data-check.txt'
    user_data.parent.mkdir(parents=True);user_data.write_text('keep')
    subprocess.run([str(installer),'--prefix',str(test_user)],check=True)
    stale=test_user/'.clap/Rustias.resources/stale.txt';stale.write_text('old')
    subprocess.run([str(installer),'--prefix',str(test_user)],check=True)
    assert not stale.exists()
    for name,destination in [('Rustias.clap','.clap/Rustias.clap'),('Rustias.vst3','.vst3/Rustias.vst3'),('Rustias.resources','.clap/Rustias.resources')]:
        files=[payload/name] if (payload/name).is_file() else (payload/name).rglob('*')
        for file in files:
            if file.is_file():
                target=test_user/destination/(file.relative_to(payload/name) if file!=payload/name else Path())
                assert hashlib.sha256(file.read_bytes()).digest()==hashlib.sha256(target.read_bytes()).digest(),target
    subprocess.run([str(installer),'--prefix',str(test_user),'--uninstall'],check=True)
    assert not (test_user/'.clap/Rustias.clap').exists()
    assert user_data.read_text()=='keep'
    debroot=work/'deb';control=debroot/'DEBIAN';control.mkdir(parents=True)
    (debroot/'usr/lib/vst3').mkdir(parents=True);(debroot/'usr/lib/clap').mkdir(parents=True)
    shutil.copytree(payload/'Rustias.vst3',debroot/'usr/lib/vst3/Rustias.vst3')
    shutil.copytree(payload/'Rustias.resources',debroot/'usr/lib/clap/Rustias.resources')
    shutil.copy2(payload/'Rustias.clap',debroot/'usr/lib/clap/Rustias.clap')
    docs=debroot/'usr/share/doc/rustias';docs.mkdir(parents=True)
    for name in ['README.md','README.ru.md','THIRD_PARTY_NOTICES.txt']: shutil.copy2(payload/name,docs/name)
    (control/'control').write_text(f'''Package: rustias
Version: {version}
Architecture: amd64
Maintainer: Rustias <noreply@github.com>
Section: sound
Priority: optional
Depends: libwebkit2gtk-4.1-0 (>= 2.40), libgtk-3-0, libstdc++6, libc6 (>= 2.35)
Homepage: https://github.com/fushugaku/Rustias
Description: Rustias native VST3 and CLAP synthesizer
 Firmware-free Rust synthesis engine with a WebKitGTK editor.
''')
    deb=packages/f'Rustias-Linux-amd64-{version}.deb'
    subprocess.run(['dpkg-deb','--root-owner-group','--build',str(debroot),str(deb)],check=True)
    extracted=work/'deb-extracted';subprocess.run(['dpkg-deb','--extract',str(deb),str(extracted)],check=True)
    assert (extracted/'usr/lib/vst3/Rustias.vst3/Contents/x86_64-linux').is_dir()
    assert (extracted/'usr/lib/clap/Rustias.resources/web/index.html').is_file()
print('Linux .run installation, upgrade, rollback backups, file hashes, uninstall and DEB payload verified.')
