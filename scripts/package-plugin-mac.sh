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
  # Fresh runner images retain an AU registrar cache predating this installation.
  # Restart only the ephemeral CI account's registrar; never touch a user's host.
  killall -u "$(id -un)" AudioComponentRegistrar 2>/dev/null || true
  auval -a > output/plugin/au-list.log 2>&1
fi
if ! auval -v aumu Rsts Fshg -strict > output/plugin/auval.log 2>&1; then
  /usr/bin/plutil -p "$rustias_plugin_build/Rustias.component/Contents/Info.plist"
  if [[ "${CI:-}" == true ]]; then cat output/plugin/au-list.log; fi
  cat output/plugin/auval.log
  exit 1
fi
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$rustias_package" output/plugin/packages/Rustias-macOS-AU-VST3.zip
