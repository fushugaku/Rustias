param([Parameter(Mandatory)][string]$Version, [Parameter(Mandatory)][string]$PackageRoot)
$ErrorActionPreference = 'Stop'
if ($env:CI -ne 'true') { throw 'Installer smoke tests may only modify an isolated CI runner.' }
$installer = (Resolve-Path "output/plugin/packages/Rustias-Windows-x64-$Version-Setup.exe").Path
$vst3 = Join-Path $env:CommonProgramW6432 'VST3/Rustias.vst3'
$clap = Join-Path $env:CommonProgramW6432 'CLAP/Rustias.clap'
$resources = Join-Path $env:CommonProgramW6432 'CLAP/Rustias.resources'
$userData = Join-Path $env:LOCALAPPDATA 'Rustias/Plugin/installer-preserve-check.txt'
New-Item -ItemType Directory -Force (Split-Path $userData) | Out-Null
Set-Content $userData 'keep'
New-Item -ItemType Directory -Force $vst3,$resources | Out-Null
Set-Content (Join-Path $vst3 'removed-by-upgrade.txt') 'stale'
Set-Content (Join-Path $resources 'removed-by-upgrade.txt') 'stale'
$process = Start-Process -FilePath $installer -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /COMPONENTS="vst3,clap" /LOG=output/plugin/windows-install.log' -Wait -PassThru
if ($process.ExitCode -notin @(0,3010)) { throw "Installer failed: $($process.ExitCode)" }
foreach ($target in @($vst3,$resources)) {
    if (Test-Path (Join-Path $target 'removed-by-upgrade.txt')) { throw 'Installer retained stale resources' }
}
foreach ($pair in @(@("$PackageRoot/Rustias.vst3",$vst3),@("$PackageRoot/Rustias.resources",$resources))) {
    $source = (Resolve-Path $pair[0]).Path
    foreach ($file in Get-ChildItem -Recurse -File $source) {
        $relative = $file.FullName.Substring($source.Length + 1)
        $destination = Join-Path $pair[1] $relative
        if (!(Test-Path $destination) -or (Get-FileHash $file.FullName).Hash -ne (Get-FileHash $destination).Hash) { throw "Installed file differs: $relative" }
    }
}
if ((Get-FileHash "$PackageRoot/Rustias.clap").Hash -ne (Get-FileHash $clap).Hash) { throw 'Installed CLAP differs' }
$uninstaller = Join-Path $env:ProgramW6432 'Rustias/unins000.exe'
$process = Start-Process $uninstaller -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
if ($process.ExitCode -ne 0 -or (Test-Path $clap) -or (Test-Path "$vst3/Contents/x86_64-win/Rustias.vst3")) { throw 'Uninstall failed' }
if ((Get-Content $userData -Raw).Trim() -ne 'keep') { throw 'Uninstall removed user data' }
$process = Start-Process -FilePath $installer -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /COMPONENTS="vst3"' -Wait -PassThru
if ($process.ExitCode -notin @(0,3010) -or !(Test-Path "$vst3/Contents/Resources/web/index.html") -or (Test-Path $clap)) { throw 'VST3-only selection failed' }
$process = Start-Process $uninstaller -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
if ($process.ExitCode -ne 0) { throw 'Second uninstall failed' }
Write-Host 'Windows EXE: real silent installation, selected formats, complete payload hashes, clean upgrade and uninstall verified.'
