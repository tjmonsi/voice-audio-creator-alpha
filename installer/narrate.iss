; Per-user installer for narrate (spec-narrate-installer.md).
; Build with: ISCC.exe /DAppVersion=<version> installer\narrate.iss   (installer\build.ps1 does this).

#ifndef AppVersion
  #error AppVersion is not defined. Pass /DAppVersion=<version> to ISCC.exe, for example /DAppVersion=0.1.0.
#endif

[Setup]
; Fixed GUID: the Phase 2 installer reuses it so it upgrades this installation.
AppId={{8F3C2A71-5D4E-4B9A-9C61-2E7F0A3B5D18}
AppName=narrate
AppVersion={#AppVersion}
AppPublisher=Toni-Jan Keith Monserrat
; {autopf} is %LOCALAPPDATA%\Programs in non-administrative mode.
DefaultDirName={autopf}\narrate
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=narrate-setup-{#AppVersion}
UninstallDisplayName=narrate
UninstallDisplayIcon={app}\narrate.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupLogging=yes

[Files]
Source: "..\target\release\narrate.exe"; DestDir: "{app}"; Flags: ignoreversion

; The user PATH is edited in [Code] so it is never duplicated, never starts with ';', and only the
; installer's own entry is removed on uninstall. %LOCALAPPDATA%\narrate (the data directory) is not
; listed anywhere: uninstall keeps it.

[Code]
const EnvKey = 'Environment';

function PathHas(const Paths, Dir: String): Boolean;
begin
  Result := Pos(';' + Lowercase(Dir) + ';', ';' + Lowercase(Paths) + ';') > 0;
end;

procedure AddToUserPath(const Dir: String);
var Paths: String;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths) then Paths := '';
  if PathHas(Paths, Dir) then exit;
  if (Paths <> '') and (Paths[Length(Paths)] <> ';') then Paths := Paths + ';';
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths + Dir);
end;

procedure RemoveFromUserPath(const Dir: String);
var Paths: String; P: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths) then exit;
  Paths := ';' + Paths + ';';
  P := Pos(';' + Lowercase(Dir) + ';', Lowercase(Paths));
  while P > 0 do
  begin
    Delete(Paths, P, Length(Dir) + 1);   { removes ';<dir>' and keeps the next ';' }
    P := Pos(';' + Lowercase(Dir) + ';', Lowercase(Paths));
  end;
  if Length(Paths) <= 2 then Paths := '' else Paths := Copy(Paths, 2, Length(Paths) - 2);
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then AddToUserPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then RemoveFromUserPath(ExpandConstant('{app}'));
end;
