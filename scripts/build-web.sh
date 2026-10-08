#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
cargo build --release --locked --target wasm32-unknown-unknown -p radias-web \
  --config 'profile.release.panic="abort"'
mkdir -p dist
cp web/index.html web/styles.css web/app.js web/worklet.js web/favicon.svg dist/
cp "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/radias_web.wasm" dist/rustias.wasm
touch dist/.nojekyll
printf 'Web build ready: %s/dist\n' "$PWD"
