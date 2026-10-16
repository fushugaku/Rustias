#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
bash scripts/build-web.sh
npm ci --prefix plugin
node plugin/build-assets.mjs
cmake -S plugin -B output/plugin/build -DCMAKE_BUILD_TYPE=Release '-DCMAKE_OSX_ARCHITECTURES=arm64;x86_64' "$@"
cmake --build output/plugin/build --target rustias_clap rustias_vst3 rustias_au rustias-au-check rustias-runtime-check rustias-host-check rustias-editor-check --parallel 4
ctest --test-dir output/plugin/build --output-on-failure
bash scripts/package-plugin-mac.sh
