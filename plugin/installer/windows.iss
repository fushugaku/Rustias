#ifndef Version
  #error Version is required
#endif
#ifndef PackageRoot
  #error PackageRoot is required
#endif

[Setup]
AppId={{B4FCA923-16AD-46A9-9BA3-04AAC589D621}
AppName=Rustias
AppVersion={#Version}
AppPublisher=Rustias
AppPublisherURL=https://github.com/fushugaku/Rustias
AppSupportURL=https://github.com/fushugaku/Rustias/issues
DefaultDirName={autopf}\Rustias
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#PackageRoot}\..
OutputBaseFilename=Rustias-Windows-x64-{#Version}-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName=Rustias {#Version}
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Types]
Name: "full"; Description: "VST3 + CLAP"
Name: "custom"; Description: "Custom"; Flags: iscustom

[Components]
Name: "vst3"; Description: "VST3"; Types: full custom
Name: "clap"; Description: "CLAP"; Types: full custom

[InstallDelete]
Type: filesandordirs; Name: "{commoncf64}\VST3\Rustias.vst3"; Components: vst3
Type: filesandordirs; Name: "{commoncf64}\CLAP\Rustias.resources"; Components: clap

[Files]
Source: "{#PackageRoot}\Rustias.vst3\*"; DestDir: "{commoncf64}\VST3\Rustias.vst3"; Components: vst3; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#PackageRoot}\Rustias.clap"; DestDir: "{commoncf64}\CLAP"; Components: clap; Flags: ignoreversion restartreplace
Source: "{#PackageRoot}\Rustias.resources\*"; DestDir: "{commoncf64}\CLAP\Rustias.resources"; Components: clap; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#PackageRoot}\README*"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PackageRoot}\THIRD_PARTY_NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PackageRoot}\prerequisites\*"; Flags: dontcopy

[Code]
function HasWebView: Boolean;
var Version: String;
begin
  Result := RegQueryStringValue(HKLM32, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0');
  if not Result then
    Result := RegQueryStringValue(HKCU, 'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0');
end;

function InstallDependency(Name, Arguments: String): String;
var Code: Integer;
begin
  Result := '';
  ExtractTemporaryFile(Name);
  if not Exec(ExpandConstant('{tmp}\') + Name, Arguments, '', SW_HIDE, ewWaitUntilTerminated, Code) then
    Result := 'Cannot start ' + Name
  else if (Code <> 0) and (Code <> 3010) and (Code <> 1638) then
    Result := Name + ' failed (code ' + IntToStr(Code) + ').';
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var Installed: Cardinal;
begin
  Result := '';
  if not RegQueryDWordValue(HKLM64, 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64', 'Installed', Installed) or (Installed <> 1) then begin
    Result := InstallDependency('vc_redist.x64.exe', '/install /quiet /norestart');
    if Result <> '' then exit;
  end;
  if not HasWebView then Result := InstallDependency('MicrosoftEdgeWebview2Setup.exe', '/silent /install');
end;
