$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
$package = 'output/plugin/packages/Rustias-Windows'
New-Item -ItemType Directory -Force $package | Out-Null
Copy-Item -Recurse -Force 'output/plugin/build/Release/Rustias.vst3' $package
Copy-Item -Force 'output/plugin/build/CLAP/Release/Rustias.clap' $package
Copy-Item -Recurse -Force 'output/plugin/build/CLAP/Release/Rustias.resources' $package
Copy-Item plugin/README.md,plugin/README.ru.md,plugin/THIRD_PARTY_NOTICES.txt $package
Compress-Archive -Path $package -DestinationPath 'output/plugin/packages/Rustias-Windows-VST3.zip' -Force
