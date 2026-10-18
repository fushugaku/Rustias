$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
$package = 'output/plugin/packages/Rustias-Windows'
if (Test-Path $package) { Remove-Item -Recurse -Force $package }
New-Item -ItemType Directory -Force $package | Out-Null
Copy-Item -Recurse -Force 'output/plugin/build/Release/Rustias.vst3' $package
Copy-Item -Force 'output/plugin/build/CLAP/Release/Rustias.clap' $package
Copy-Item -Recurse -Force 'output/plugin/build/CLAP/Release/Rustias.resources' $package
Copy-Item plugin/README.md,plugin/README.ru.md,plugin/THIRD_PARTY_NOTICES.txt $package
Compress-Archive -Path $package -DestinationPath 'output/plugin/packages/Rustias-Windows-VST3.zip' -Force

$version = [regex]::Match((Get-Content plugin/CMakeLists.txt -Raw), 'project\(RustiasPlugin VERSION ([0-9.]+)').Groups[1].Value
$prerequisites = Join-Path $package 'prerequisites'
New-Item -ItemType Directory -Force $prerequisites | Out-Null
$downloads = @{
    'vc_redist.x64.exe' = 'https://aka.ms/vs/17/release/vc_redist.x64.exe'
    'MicrosoftEdgeWebview2Setup.exe' = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703'
}
foreach ($name in $downloads.Keys) {
    $destination = Join-Path $prerequisites $name
    Invoke-WebRequest -Uri $downloads[$name] -OutFile $destination
    $signature = Get-AuthenticodeSignature $destination
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
        throw "Invalid Microsoft prerequisite signature: $name"
    }
}
$compiler = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe'
if (!(Test-Path $compiler)) {
    choco install innosetup --yes --no-progress
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup installation failed' }
}
& $compiler "/DVersion=$version" "/DPackageRoot=$((Resolve-Path $package).Path)" plugin/installer/windows.iss
if ($LASTEXITCODE -ne 0) { throw 'Windows installer build failed' }
& "$PSScriptRoot/verify-plugin-windows.ps1" -Version $version -PackageRoot $package
