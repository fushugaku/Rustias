#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
rustias_plugin_build="${RUSTIAS_PLUGIN_BUILD:-output/plugin/build}"
rustias_package="output/plugin/packages/Rustias-macOS"
mkdir -p "$rustias_package"
for rustias_format in clap component vst3; do
  rustias_bundle="$rustias_plugin_build/Rustias.$rustias_format"
  if [[ "$rustias_format" != clap ]]; then
    codesign --force --sign - --timestamp=none "$rustias_bundle/Contents/PlugIns/Rustias.clap"
  fi
  codesign --force --sign - --timestamp=none "$rustias_bundle"
  codesign --verify --deep --strict "$rustias_bundle"
  # This is generated package staging; discard files removed by newer builds.
  rm -rf "$rustias_package/Rustias.$rustias_format"
  ditto "$rustias_bundle" "$rustias_package/Rustias.$rustias_format"
done
cp plugin/README.md plugin/README.ru.md plugin/THIRD_PARTY_NOTICES.txt "$rustias_package/"
cp plugin/Install.command "$rustias_package/"
chmod +x "$rustias_package/Install.command"
# AU validation renders offline buffers; no hardware output is opened.
mkdir -p "$HOME/Library/Audio/Plug-Ins/Components"
ditto "$rustias_plugin_build/Rustias.component" "$HOME/Library/Audio/Plug-Ins/Components/Rustias.component"
touch "$HOME/Library/Audio/Plug-Ins/Components/Rustias.component"
if [[ "${CI:-}" == true ]]; then
  # Hosted runners can run the registrar outside the login user's library domain.
  sudo mkdir -p /Library/Audio/Plug-Ins/Components
  sudo ditto "$rustias_plugin_build/Rustias.component" /Library/Audio/Plug-Ins/Components/Rustias.component
  # Fresh runner images retain an AU registrar cache predating this installation.
  # Restart only the ephemeral CI account's registrar; never touch a user's host.
  killall -u "$(id -un)" AudioComponentRegistrar 2>/dev/null || true
  auval -a > output/plugin/au-list.log 2>&1
fi
if ! auval -v aumu Rsts Fshg -strict > output/plugin/auval.log 2>&1; then
  if [[ "${CI:-}" == true && "$(<output/plugin/auval.log)" == *"didn't find the component"* ]]; then
    # No login session is required for the documented in-process factory API.
    # This checks the signed AU itself, including MIDI, stereo audio and state.
    echo 'AU registry did not discover this bundle; validating its exported factory directly.'
    "$rustias_plugin_build/rustias-au-check" "$PWD/$rustias_plugin_build/Rustias.component"
  else
    cat output/plugin/auval.log
    exit 1
  fi
fi
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$rustias_package" output/plugin/packages/Rustias-macOS-AU-VST3.zip
python3 scripts/package-plugin-pkg.py --bundles "$rustias_package"
