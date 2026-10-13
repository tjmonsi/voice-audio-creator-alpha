# Installer research: Inno Setup per-user install

Checked 2026-10-12. Help-page quotes come from https://jrsoftware.org/ishelp/ , which documents the current Inno Setup release (it also lists newer options such as `/NOSTYLE` and RedirectionGuard); the directives below exist in 6.x unless marked.

## 1. Getting Inno Setup 6

| Item | Value | Source |
|---|---|---|
| winget id | `JRSoftware.InnoSetup`, latest 6.7.3 | https://github.com/microsoft/winget-pkgs/tree/master/manifests/j/JRSoftware/InnoSetup/6.7.3 |
| Installer | `https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe`, sha256 `9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732`, architecture x86 | same manifest |
| Scopes | `machine` (installer switch `/ALLUSERS`) and `user` (`/CURRENTUSER`) | same manifest |
| Inno Setup 7 | separate id `JRSoftware.InnoSetup.7`, 7.1.0, x64 and x86 installers | `.../InnoSetup/7/7.1.0` |
| License | "Inno Setup License", https://jrsoftware.org/files/is/license.txt | locale manifest |

Install without admin: `winget install --id JRSoftware.InnoSetup --exact --version 6.7.3 --scope user --silent --accept-package-agreements --accept-source-agreements` (standard winget flags; not run here).

ISCC location: Inno Setup's own script uses `DefaultDirName={code:GetDefaultDirName|Inno Setup 6}` with `GetDefaultDirName` returning `{autopf}\<param>` and `PrivilegesRequiredOverridesAllowed=dialog` (`setup.iss` line 25, 27; `isportable.iss` lines 14-21 at tag `is-6_7_3`). So:
- user scope: `%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe`
- machine scope (32-bit setup): `C:\Program Files (x86)\Inno Setup 6\ISCC.exe`
- Lookup that works for either scope: registry uninstall key `Inno Setup 6_is1` (AppId `Inno Setup 6`, `setup.iss` line 15), value `InstallLocation`, under HKCU or HKLM\SOFTWARE\WOW6432Node. Inno Setup is not installed on this machine (tested: neither path exists).

## 2. Per-user install directives

| Directive | Value | Doc quote |
|---|---|---|
| `PrivilegesRequired` | `lowest` | "Setup will not request to be run with administrative privileges even if it was started by a member of the Administrators group and will always run in non administrative install mode." |
| `DefaultDirName` | `{autopf}\narrate` | "auto" constants "automatically map to their "common" form unless the installation is running in non administrative install mode, in which case they map to their "user" form." `{userpf}`: "The path to the current user's Program Files folder" (`%LOCALAPPDATA%\Programs`; on pre-Windows 7 it maps to `{localappdata}\Programs`). |
| `AppId` | fixed GUID, identical in phase 1 and phase 2 installers | AppId "is checked by subsequent installations to determine whether it may append to a particular existing uninstall log" and "determines the actual name of the Uninstall registry key, to which Inno Setup tacks on "_is1"". |
| `ArchitecturesAllowed` / `ArchitecturesInstallIn64BitMode` | `x64compatible` | doc example uses exactly this pair for x64-only apps; added in 6.3.0 (2024-06-09): "In ArchitecturesAllowed and ArchitecturesInstallIn64BitMode, change any use of x64 to x64compatible." (https://jrsoftware.org/files/is6-whatsnew.htm) |
| `ChangesEnvironment` | `yes` | "at the end of the installation Setup will notify other running applications (notably Windows Explorer) that they should reload their environment variables from the registry." Without it, new values are not seen "by applications launched from Explorer until the user logs off or restarts the computer." |
| `PrivilegesRequiredOverridesAllowed` | leave blank | `/ALLUSERS` and `/CURRENTUSER` only work when this allows `commandline`; not needed for a lowest-privilege installer |
| `DisableProgramGroupPage` | `yes` | no Start Menu folder needed for a CLI |

Registry root: `HKCU` is HKEY_CURRENT_USER; `HKA` "Equals HKLM in administrative install mode, HKCU otherwise". `HKCU\Environment` is not affected by 32/64-bit registry views.

## 3. Adding `{app}` to the user PATH safely

Facts:
- `ValueType: expandsz` writes REG_EXPAND_SZ; `{olddata}` "is replaced with the previous data of the registry value ... If the value does not exist or the existing value isn't a string type, the {olddata} constant is silently removed." A pure `[Registry]` entry `ValueData: "{olddata};{app}"` therefore produces a leading `;` when the user has no Path value, and cannot remove the entry on uninstall.
- Flag `uninsdeletevalue` would "Delete the value when the program is uninstalled", which would wipe the user's whole Path. Do not use it on `Path`.
- Pascal functions (help topics `isxfunc_regquerystringvalue`, `isxfunc_regwriteexpandstringvalue`): `RegQueryStringValue(const RootKey: HKEY; const SubKeyName, ValueName: String; var ResultStr: String): Boolean` reads REG_SZ or REG_EXPAND_SZ; `RegWriteExpandStringValue(const RootKey: HKEY; const SubKeyName, ValueName, Data: String): Boolean`.
- Events: `procedure CurStepChanged(CurStep: TSetupStep)` (`ssPostInstall` "just after the actual installation finishes"); `procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep)` with values `usAppMutexCheck, usUninstall, usPostUninstall, usDone`.

Suggested `[Code]` (not compiled here; ISCC is not installed):

```pascal
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
```

Limits: comparison is case-insensitive but does not treat `C:\x\` and `C:\x` as equal. Shells that are already open keep their old PATH; verify `narrate` in a new terminal. Whether the uninstaller broadcasts the environment change when `ChangesEnvironment=yes` is unverified.

## 4. Uninstall cleanup

- Files installed through `[Files]` and the PATH entry (code above) are removed by the uninstaller.
- The data dir `%LOCALAPPDATA%\narrate` (Python env, models, voice clips; several GB) is created by `narrate setup`, not by the installer, so it stays unless the script deletes it. Option: `[UninstallDelete] Type: filesandordirs; Name: "{localappdata}\narrate"`; `filesandordirs` "matches directory names also, and any directories matching the name are deleted including all files and subdirectories in them". The spec should decide whether uninstall removes it (or offers a `narrate` command to clean it).

## 5. Command lines

| Action | Command |
|---|---|
| Build | `"<ISCC dir>\ISCC.exe" installer\narrate.iss` (output dir via `OutputDir` or `/O`) |
| Silent install | `narrate-setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /LOG="<file>"` |
| Silent uninstall | `"%LOCALAPPDATA%\Programs\narrate\unins000.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART` |

Doc quotes: `/VERYSILENT`: "When a setup is very silent this installation progress window is not displayed"; `/SUPPRESSMSGBOXES`: "Only has an effect when combined with /SILENT or /VERYSILENT"; `/CURRENTUSER`: "Instructs Setup to install in non administrative install mode. Only has an effect when the [Setup] section directive PrivilegesRequiredOverridesAllowed allows the commandline override."; `/NORESTART`: "Prevents Setup from restarting the system following a successful installation"; `/DIR="x:\dirname"` overrides the destination; `/LOG="filename"` writes a log to a fixed path. Uninstaller accepts `/SILENT`, `/VERYSILENT`, `/SUPPRESSMSGBOXES`, `/LOG`, `/NORESTART` (help topic `uninstcmdline`).

## Unverified

- The `[Code]` block above has not been compiled.
- Exact uninstall registry location for a lowest-privilege install (expected under HKCU per HKA semantics).
