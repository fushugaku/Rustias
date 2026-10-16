#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p "$HOME/Library/Audio/Plug-Ins/Components" "$HOME/Library/Audio/Plug-Ins/VST3" "$HOME/Library/Audio/Plug-Ins/CLAP"
ditto Rustias.component "$HOME/Library/Audio/Plug-Ins/Components/Rustias.component"
ditto Rustias.vst3 "$HOME/Library/Audio/Plug-Ins/VST3/Rustias.vst3"
ditto Rustias.clap "$HOME/Library/Audio/Plug-Ins/CLAP/Rustias.clap"
printf 'Installed Rustias AU, VST3 and CLAP for this user. Restart your DAW and rescan plug-ins.\n'
