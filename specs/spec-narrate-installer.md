# spec-narrate-installer

## Metadata

| Field | Value |
|---|---|
| **Feature** | Per-user Inno Setup installer for `narrate` and its build script |
| **Parent feature** | spec-narrate-architecture.md |
| **Sub-features** | None |
| **Requirement refs** | BR-002, UR-005, UR-010, FR-016, NFR-003, NFR-009 |
| **Feature type** | Infrastructure |
| **Status** | Draft |

Status values: Draft, Review, Approved, Implemented.

## Description

`installer/narrate.iss` builds `dist\narrate-setup-<version>.exe`, an Inno Setup 6 installer that copies `narrate.exe` to `%LOCALAPPDATA%\Programs\narrate` without elevation, adds that directory to the user PATH exactly once, and on uninstall removes the binary and only its own PATH entry. `installer/build.ps1` builds the release binary, refuses a binary that has test hooks, finds `ISCC.exe`, and compiles the script. The AppId is fixed now so the Phase 2 installer (which adds `narrate-onnx`) upgrades the same installation.

## Triggered by

| Trigger type | Source | Condition |
|---|---|---|
| Deployment | devops (plan step 5) runs `powershell -NoProfile -ExecutionPolicy Bypass -File installer\build.ps1` | After QA approves Phase 1 |
| User action | Owner double-clicks `narrate-setup-0.1.0.exe` | Install or reinstall |
| User action | Windows Settings uninstall, or `unins000.exe` | Uninstall |

## Required data

| Data | Source | Type | Notes |
|---|---|---|---|
| `narrate.exe` | `target\release\narrate.exe` from `cargo build --release -p narrate` | File | Must print exactly `narrate <version>` for `--version` |
| `AppVersion` | Parsed from that `--version` line | String | Passed to ISCC as `/DAppVersion=<version>` |
| Inno Setup 6.7.3 | `winget install --id JRSoftware.InnoSetup --exact --version 6.7.3 --scope user --silent --accept-package-agreements --accept-source-agreements` (research installer section 1) | Tool | Installer sha256 `9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732` per the winget manifest |

## Behavior

### Requirement: Setup section directives

`installer/narrate.iss` SHALL contain these `[Setup]` values and SHALL fail to compile when `AppVersion` is not defined (`#ifndef AppVersion` with `#error`).

| Directive | Value | Source |
|---|---|---|
| `AppId` | `{{8F3C2A71-5D4E-4B9A-9C61-2E7F0A3B5D18}` (fixed; Phase 2 reuses it unchanged) | research installer section 2 |
| `AppName` | `narrate` | |
| `AppVersion` | `{#AppVersion}` | |
| `AppPublisher` | `Toni-Jan Keith Monserrat` | |
| `DefaultDirName` | `{autopf}\narrate` (maps to `%LOCALAPPDATA%\Programs\narrate` in non-administrative mode) | research installer section 2 |
| `PrivilegesRequired` | `lowest` | research installer section 2 |
| `PrivilegesRequiredOverridesAllowed` | not set | research installer section 2 |
| `ArchitecturesAllowed` | `x64compatible` | research installer section 2 |
| `ArchitecturesInstallIn64BitMode` | `x64compatible` | research installer section 2 |
| `ChangesEnvironment` | `yes` | research installer section 2 |
| `DisableProgramGroupPage` | `yes` | research installer section 2 |
| `OutputDir` | `..\dist` (relative to the script) | |
| `OutputBaseFilename` | `narrate-setup-{#AppVersion}` | |
| `UninstallDisplayName` | `narrate` | |
| `UninstallDisplayIcon` | `{app}\narrate.exe` | |
| `Compression` / `SolidCompression` | `lzma2` / `yes` | |
| `WizardStyle` | `modern` | |
| `SetupLogging` | `yes` | |

`[Files]`: one entry, `Source: "..\target\release\narrate.exe"; DestDir: "{app}"; Flags: ignoreversion`. No `[Registry]` entry touches `Path`, and no `uninsdeletevalue` flag is used on `Path` (research installer section 3). No `[UninstallDelete]` entry names the data directory.

#### Scenario: Script compiles

- **GIVEN** Inno Setup 6.7.3 and a release `narrate.exe`
- **WHEN** `installer\build.ps1` runs
- **THEN** `dist\narrate-setup-0.1.0.exe` SHALL exist and the script SHALL exit 0

#### Scenario: Version missing

- **GIVEN** the script compiled without `/DAppVersion`
- **WHEN** ISCC runs
- **THEN** compilation SHALL fail with an error naming `AppVersion`

### Requirement: User PATH entry managed in code

The installer SHALL add `{app}` to `HKCU\Environment\Path` in `CurStepChanged(ssPostInstall)` only when it is not already present (case-insensitive, `;`-delimited comparison), preserving the value type as REG_EXPAND_SZ and adding no leading `;` when the value is empty or absent, and the uninstaller SHALL remove every occurrence of `{app}` from that value in `CurUninstallStepChanged(usPostUninstall)` while leaving all other entries in order; the `[Code]` of research installer section 3 (functions `PathHas`, `AddToUserPath`, `RemoveFromUserPath`) implements this and SHALL be used as written.

#### Scenario: Install adds one entry

- **GIVEN** a user PATH without the install directory
- **WHEN** `narrate-setup-0.1.0.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /LOG="<log>"` runs from a non-elevated shell
- **THEN** the exit code SHALL be 0 and `[Environment]::GetEnvironmentVariable('Path','User')` split on `;` SHALL contain `%LOCALAPPDATA%\Programs\narrate` (expanded) exactly once
- **AND** all entries that were there before SHALL still be there in the same order

#### Scenario: Reinstall keeps one entry

- **GIVEN** `narrate` is installed
- **WHEN** the same silent install runs again
- **THEN** the user PATH SHALL contain the install directory exactly once

#### Scenario: Uninstall removes only its entry

- **GIVEN** `narrate` is installed and the user PATH holds other entries
- **WHEN** `"%LOCALAPPDATA%\Programs\narrate\unins000.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART` runs
- **THEN** the install directory SHALL no longer exist and the user PATH SHALL contain every other entry unchanged and no entry for the install directory

### Requirement: No elevation and per-user location

The installer SHALL run in non-administrative mode for every user, SHALL install under `%LOCALAPPDATA%\Programs\narrate`, and SHALL write only the install directory, its HKCU uninstall key, and `HKCU\Environment\Path` (NFR-003, UR-010).

#### Scenario: Install without admin (FR-016)

- **GIVEN** a standard Windows user session without elevation
- **WHEN** the silent install runs
- **THEN** it SHALL finish with exit code 0 without a UAC prompt, `%LOCALAPPDATA%\Programs\narrate\narrate.exe` SHALL exist, and nothing SHALL be written under `C:\Program Files`
- **AND** a new process whose PATH is rebuilt from the registry (`[Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')`) SHALL run `narrate --version` and print `narrate 0.1.0`

#### Scenario: Install location not writable (FR-016)

- **GIVEN** a target directory with a deny-write ACL for the current user (`icacls <dir> /deny "%USERNAME%":(OI)(CI)(W)`), used through `/DIR="<dir>"` so the real install is not touched
- **WHEN** the silent install runs
- **THEN** the installer SHALL exit non-zero (Inno Setup's abort exit code; the exact value is not in the research notes and devops records it)
- **AND** the user PATH SHALL contain no entry for `<dir>`

### Requirement: Uninstall keeps the data directory

The uninstaller SHALL NOT delete `%LOCALAPPDATA%\narrate` (several GB of Python environment, models, and clips shared with development builds and Phase 2), and the README SHALL tell the Owner how to remove it by hand (`Remove-Item -Recurse -Force "$env:LOCALAPPDATA\narrate"`).

#### Scenario: Data kept

- **GIVEN** `narrate` is installed and `%LOCALAPPDATA%\narrate\setup.json` exists
- **WHEN** the silent uninstall runs
- **THEN** `%LOCALAPPDATA%\narrate\setup.json` SHALL still exist

### Requirement: Build script

`installer/build.ps1` SHALL (1) run `cargo build --release -p narrate`; (2) run `target\release\narrate.exe --version` and stop with exit 1 unless the output matches `^narrate (\d+\.\d+\.\d+)$`, which also rejects test-hook builds; (3) find `ISCC.exe` in this order: `$env:ISCC` if it names an existing file; `InstallLocation` of `HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1`; the same key under `HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\`; `$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe`; `${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe` (research installer section 1); (4) stop with exit 1 and print the winget install command when none is found; (5) run `ISCC.exe /DAppVersion=<version> installer\narrate.iss` and exit with ISCC's code; (6) print the path of the built installer.

#### Scenario: ISCC missing

- **GIVEN** no Inno Setup 6 installation and `$env:ISCC` unset
- **WHEN** `installer\build.ps1` runs
- **THEN** it SHALL exit 1 and print `winget install --id JRSoftware.InnoSetup --exact --version 6.7.3 --scope user`

#### Scenario: Test-hook binary refused

- **GIVEN** `target\release\narrate.exe` was built with `--features test-hooks`
- **WHEN** step (2) runs
- **THEN** the script SHALL exit 1 without calling ISCC (step (1) rebuilds without features, so this only happens if the build is skipped or replaced)

## Algorithmic steps

1. devops installs Inno Setup 6.7.3 with the winget command above (user scope).
2. `installer\build.ps1` builds and packages.
3. devops runs the verification sequence in spec-narrate-test-strategy.md (installer checks), ending with `narrate` installed.

## Output / Outcome

| Output | Type | Description |
|---|---|---|
| `dist\narrate-setup-0.1.0.exe` | Artifact | Per-user installer (git-ignored `dist/`) |
| `%LOCALAPPDATA%\Programs\narrate\narrate.exe`, `unins000.exe`, `unins000.dat` | Installed files | |
| `HKCU\Environment\Path` | Registry value | One entry for the install directory |
| HKCU uninstall key `{8F3C2A71-5D4E-4B9A-9C61-2E7F0A3B5D18}_is1` | Registry key | Expected under HKCU per HKA semantics (research installer, Unverified list) |

## Error handling

| Error condition | Handling strategy | Outcome |
|---|---|---|
| File copy fails (ACL, disk) | Inno aborts before `ssPostInstall` | Non-zero exit; PATH untouched |
| Path value absent | `AddToUserPath` writes just `{app}` | No leading `;` |
| Path already contains `{app}` | `PathHas` short-circuits | No duplicate |
| Shells opened before install | Out of scope of the installer | Owner opens a new terminal (research installer section 3, Limits) |
| ISCC not found | Build script stops | Exit 1 with install command |

## Events emitted (if applicable)

Not applicable.

## Dependencies

| Dependency | Type | Required | Notes |
|---|---|---|---|
| Inno Setup 6.7.3 (`JRSoftware.InnoSetup`) | Build-time | yes | x64compatible needs 6.3.0 or later (research installer section 2) |
| Rust 1.99 toolchain | Build-time | yes | Release build |
| `ChangesEnvironment=yes` broadcast | Runtime | yes | Uninstall broadcast is unverified (research installer section 3) |

## Sub-feature breakdown (if applicable)

Not applicable.

## Submodule scope (if applicable)

Not applicable.

## Traceability

| Requirement | Spec section | Verified by |
|---|---|---|
| BR-002, UR-005 | Requirements: Setup section directives; No elevation and per-user location | devops installer checks (step 5) |
| UR-010 | Requirement: No elevation and per-user location | devops registry and file check |
| FR-016 | All Requirements | devops installer checks: install, reinstall, deny ACL, uninstall |
| NFR-003 | Requirement: No elevation and per-user location | Silent install from a non-elevated shell |
| NFR-009 | `ArchitecturesAllowed=x64compatible` | Install on the Windows 11 x64 target |
