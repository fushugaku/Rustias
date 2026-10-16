$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
bash scripts/build-web.sh
if ($LASTEXITCODE -ne 0) { throw 'WASM build failed' }
npm ci --prefix plugin
if ($LASTEXITCODE -ne 0) { throw 'Plugin dependencies failed' }
node plugin/build-assets.mjs
if ($LASTEXITCODE -ne 0) { throw 'Plugin assets failed' }
cmake -S plugin -B output/plugin/build -A x64
if ($LASTEXITCODE -ne 0) { throw 'Plugin configuration failed' }
cmake --build output/plugin/build --config Release --target rustias_clap rustias_vst3 rustias-runtime-check rustias-host-check rustias-editor-check --parallel 4
if ($LASTEXITCODE -ne 0) { throw 'Plugin compilation failed' }
ctest --test-dir output/plugin/build -C Release --output-on-failure
if ($LASTEXITCODE -ne 0) { throw 'Plugin verification failed' }
& "$PSScriptRoot/package-plugin-windows.ps1"
