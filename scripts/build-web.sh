#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
# Keep cargo, rustc and the installed Wasm standard library in one toolchain.
rustias_cargo_bin=cargo
if command -v rustup >/dev/null 2>&1; then
  rustias_cargo_bin="$(rustup which cargo)"
  export RUSTC="${RUSTC:-$(rustup which rustc)}"
fi
"$rustias_cargo_bin" build --release --locked --target wasm32-unknown-unknown -p radias-web \
  --config 'profile.release.panic="abort"'
mkdir -p dist
cp web/index.html web/styles.css web/app.js web/worklet.js web/favicon.svg dist/
cp crates/radias-synth-infrastructure/src/parameters.json dist/parameters.json
cp "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/radias_web.wasm" dist/rustias.wasm
touch dist/.nojekyll
printf 'Web build ready: %s/dist\n' "$PWD"
