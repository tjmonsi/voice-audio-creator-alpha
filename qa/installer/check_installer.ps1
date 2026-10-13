<#
.SYNOPSIS
Runs the installer check sequence of spec-narrate-test-strategy.md (Requirement: Installer checks) and prints PASS or FAIL per check.

.DESCRIPTION
  powershell -NoProfile -ExecutionPolicy Bypass -File qa\installer\check_installer.ps1 [-Version 0.1.0] [-RepoRoot <path>]

Run it from a non-elevated shell of the Owner's standard user. It changes the real per-user install
(%LOCALAPPDATA%\Programs\narrate) and the real user PATH, and it ends with narrate installed when
every check passes. It never touches %LOCALAPPDATA%\narrate (the data directory).

Checks, in order:
  1. installer\build.ps1 builds dist\narrate-setup-<version>.exe.
  2. A silent install into a scratch /DIR that has a deny-write ACE exits non-zero and leaves no
     PATH entry (the ACE is removed afterwards).
  3. A silent install exits 0, puts narrate.exe under %LOCALAPPDATA%\Programs\narrate and writes
     nothing under C:\Program Files.
  4. The user PATH holds the install directory exactly once, and a process whose PATH is rebuilt
     from the registry prints "narrate 0.1.0" (the line "narrate <version>" for the given version).
  5. A silent reinstall exits 0 and the PATH entry is still there exactly once.
  6. A silent uninstall (unins000.exe) removes the install directory and the PATH entry, leaves the
     other PATH entries unchanged and keeps %LOCALAPPDATA%\narrate.
  7. A silent install again, and "narrate status" from a rebuilt-PATH process exits 0.

The installer's own exit codes are printed in the detail of each line (devops records the abort
code of check 2). Installer logs go to %TEMP%\narrate-installer-check\.

Exit codes: 0 every check passed; 1 at least one check failed.
#>
[CmdletBinding()]
param(
    [string]$Version = '0.1.0',
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
)

$ErrorActionPreference = 'Stop'
$script:Failures = 0
$script:Checks = 0

$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\narrate'
$DataDir = Join-Path $env:LOCALAPPDATA 'narrate'
$Installer = Join-Path $RepoRoot ('dist\narrate-setup-' + $Version + '.exe')
$LogDir = Join-Path $env:TEMP 'narrate-installer-check'
$AppIdKey = '{8F3C2A71-5D4E-4B9A-9C61-2E7F0A3B5D18}_is1'

function Write-Check([string]$Name, [bool]$Ok, [string]$Detail) {
    $script:Checks++
    $suffix = ''
    if ($Detail) { $suffix = ' (' + $Detail + ')' }
    if ($Ok) {
        Write-Output ('PASS: ' + $Name + $suffix)
    }
    else {
        $script:Failures++
        Write-Output ('FAIL: ' + $Name + $suffix)
    }
}

function Get-UserPathEntries {
    $key = Get-Item -LiteralPath 'HKCU:\Environment'
    $raw = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    return @($raw.Split(';') | Where-Object { $_.Trim() -ne '' })
}

function Convert-ToKey([string]$Entry) {
    $expanded = [Environment]::ExpandEnvironmentVariables($Entry.Trim())
    try { $expanded = [System.IO.Path]::GetFullPath($expanded) } catch { }
    return $expanded.TrimEnd('\').ToLowerInvariant()
}

function Get-EntryCount([string]$Dir) {
    $want = Convert-ToKey $Dir
    return @(Get-UserPathEntries | Where-Object { (Convert-ToKey $_) -eq $want }).Count
}

function Get-OtherEntries([string]$Dir) {
    $want = Convert-ToKey $Dir
    return @(Get-UserPathEntries | Where-Object { (Convert-ToKey $_) -ne $want } | ForEach-Object { Convert-ToKey $_ })
}

function Invoke-Installer([string]$Tag, [string]$ExtraArgs) {
    if (-not (Test-Path -LiteralPath $LogDir)) { New-Item -ItemType Directory -Path $LogDir | Out-Null }
    $log = Join-Path $LogDir ($Tag + '.log')
    $argLine = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /LOG="' + $log + '"'
    if ($ExtraArgs) { $argLine = $argLine + ' ' + $ExtraArgs }
    $proc = Start-Process -FilePath $Installer -ArgumentList $argLine -Wait -PassThru
    return $proc.ExitCode
}

# Runs a command line in a new process whose PATH is rebuilt from the registry (Machine + ';' + User).
function Invoke-WithRebuiltPath([string]$CommandLine) {
    $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $user = [Environment]::GetEnvironmentVariable('Path', 'User')
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $info.Arguments = '/d /c ' + $CommandLine
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.CreateNoWindow = $true
    $info.EnvironmentVariables['Path'] = $machine + ';' + $user
    foreach ($name in @($info.EnvironmentVariables.Keys)) {
        if ($name -like 'NARRATE_*') { $info.EnvironmentVariables.Remove($name) }
    }
    $proc = [System.Diagnostics.Process]::Start($info)
    $stdout = $proc.StandardOutput.ReadToEnd()
    $stderr = $proc.StandardError.ReadToEnd()
    $proc.WaitForExit()
    return @{ Code = $proc.ExitCode; Out = $stdout; Err = $stderr }
}

function Get-FirstLine([string]$Text) {
    $first = ($Text -split "`r?`n" | Where-Object { $_.Trim() -ne '' } | Select-Object -First 1)
    if ($null -eq $first) { return '' }
    return $first.Trim()
}

Write-Output ('installer check: version ' + $Version + ', repository ' + $RepoRoot)

# 1. installer\build.ps1
$buildScript = Join-Path $RepoRoot 'installer\build.ps1'
$buildCode = -1
if (Test-Path -LiteralPath $buildScript) {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $buildScript | Out-Host
    $buildCode = $LASTEXITCODE
}
$built = Test-Path -LiteralPath $Installer
Write-Check 'build.ps1 builds the installer' (($buildCode -eq 0) -and $built) ('exit ' + $buildCode + ', ' + $Installer + ' exists: ' + $built)
if (-not $built) {
    foreach ($name in @('deny-ACL install is refused', 'silent install', 'PATH entry once and narrate --version', 'silent reinstall', 'silent uninstall', 'silent install again and narrate status')) {
        Write-Check $name $false 'not run: the installer was not built'
    }
    Write-Output ('SUMMARY: ' + $script:Checks + ' checks, ' + $script:Failures + ' failed')
    exit 1
}

# 2. deny-ACL install into a scratch /DIR
$scratch = Join-Path $env:TEMP ('narrate-installer-deny-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$denyCode = $null
$denyEntry = $true
try {
    & icacls.exe $scratch /deny ($env:USERNAME + ':(OI)(CI)(W)') | Out-Null
    $denyCode = Invoke-Installer 'install-deny-acl' ('/DIR="' + $scratch + '"')
    $denyEntry = (Get-EntryCount $scratch) -gt 0
}
finally {
    & icacls.exe $scratch /remove:d $env:USERNAME | Out-Null
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Check 'deny-ACL install is refused' (($null -ne $denyCode) -and ($denyCode -ne 0) -and (-not $denyEntry)) ('exit code ' + $denyCode + ', PATH entry for the scratch directory: ' + $denyEntry)

# 3. silent install
$othersBefore = Get-OtherEntries $InstallDir
$code3 = Invoke-Installer 'install-1' ''
$exe = Join-Path $InstallDir 'narrate.exe'
$programFiles = @($env:ProgramFiles, ${env:ProgramFiles(x86)}) | Where-Object { $_ } | ForEach-Object { Join-Path $_ 'narrate' }
$underProgramFiles = @($programFiles | Where-Object { Test-Path -LiteralPath $_ }).Count -gt 0
$machineKey = (Test-Path -LiteralPath ('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\' + $AppIdKey)) -or (Test-Path -LiteralPath ('HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\' + $AppIdKey))
Write-Check 'silent install' (($code3 -eq 0) -and (Test-Path -LiteralPath $exe) -and (-not $underProgramFiles) -and (-not $machineKey)) ('exit ' + $code3 + ', narrate.exe present: ' + (Test-Path -LiteralPath $exe) + ', under Program Files: ' + $underProgramFiles + ', HKLM uninstall key: ' + $machineKey)

# 4. PATH entry exactly once, rebuilt PATH runs narrate --version
$count4 = Get-EntryCount $InstallDir
$run4 = Invoke-WithRebuiltPath 'narrate --version'
$line4 = Get-FirstLine $run4.Out
Write-Check 'PATH entry once and narrate --version' (($count4 -eq 1) -and ($run4.Code -eq 0) -and ($line4 -eq ('narrate ' + $Version))) ('PATH entries ' + $count4 + ', exit ' + $run4.Code + ', output "' + $line4 + '"')

# 5. silent reinstall
$code5 = Invoke-Installer 'install-2' ''
$count5 = Get-EntryCount $InstallDir
Write-Check 'silent reinstall' (($code5 -eq 0) -and ($count5 -eq 1)) ('exit ' + $code5 + ', PATH entries ' + $count5)

# 6. silent uninstall
$dataBefore = Test-Path -LiteralPath $DataDir
$setupBefore = Test-Path -LiteralPath (Join-Path $DataDir 'setup.json')
$unins = Join-Path $InstallDir 'unins000.exe'
$haveUnins = Test-Path -LiteralPath $unins
$code6 = -1
if ($haveUnins) {
    $proc = Start-Process -FilePath $unins -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
    $code6 = $proc.ExitCode
    # The uninstaller hands over to a temporary copy of itself, so wait for the directory to go.
    $deadline = (Get-Date).AddSeconds(60)
    while ((Test-Path -LiteralPath $InstallDir) -and ((Get-Date) -lt $deadline)) { Start-Sleep -Milliseconds 500 }
}
$dirGone = -not (Test-Path -LiteralPath $InstallDir)
$count6 = Get-EntryCount $InstallDir
$othersAfter = Get-OtherEntries $InstallDir
$othersSame = (($othersBefore -join ';') -eq ($othersAfter -join ';'))
$dataKept = $true
if ($dataBefore) { $dataKept = Test-Path -LiteralPath $DataDir }
if ($setupBefore) { $dataKept = $dataKept -and (Test-Path -LiteralPath (Join-Path $DataDir 'setup.json')) }
$dataNote = 'data directory kept'
if (-not $dataBefore) { $dataNote = 'data directory did not exist before, so nothing to keep' }
Write-Check 'silent uninstall' ($haveUnins -and $dirGone -and ($count6 -eq 0) -and $othersSame -and $dataKept) ('unins000.exe present: ' + $haveUnins + ', exit ' + $code6 + ', install directory gone: ' + $dirGone + ', PATH entries for it: ' + $count6 + ', other PATH entries unchanged: ' + $othersSame + ', ' + $dataNote + ': ' + $dataKept)

# 7. silent install again, narrate status from a rebuilt PATH
$code7 = Invoke-Installer 'install-3' ''
$run7 = Invoke-WithRebuiltPath 'narrate status'
$count7 = Get-EntryCount $InstallDir
Write-Check 'silent install again and narrate status' (($code7 -eq 0) -and ($count7 -eq 1) -and ($run7.Code -eq 0)) ('install exit ' + $code7 + ', PATH entries ' + $count7 + ', narrate status exit ' + $run7.Code + ', last stdout line "' + (($run7.Out -split "`r?`n" | Where-Object { $_.Trim() -ne '' } | Select-Object -Last 1)) + '"')

Write-Output ('SUMMARY: ' + $script:Checks + ' checks, ' + $script:Failures + ' failed')
if ($script:Failures -gt 0) { exit 1 }
exit 0
