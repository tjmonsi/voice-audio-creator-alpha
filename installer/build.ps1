# Builds the release binary and packages dist\narrate-setup-<version>.exe with Inno Setup 6
# (spec-narrate-installer.md, Requirement: Build script).
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File installer\build.ps1
#
# Set $env:ISCC to the path of ISCC.exe to skip the search.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$installCommand = 'winget install --id JRSoftware.InnoSetup --exact --version 6.7.3 --scope user --silent --accept-package-agreements --accept-source-agreements'
$repoRoot = Split-Path -Parent $PSScriptRoot

function Find-Iscc {
    if ($env:ISCC -and (Test-Path -LiteralPath $env:ISCC -PathType Leaf)) {
        return $env:ISCC
    }
    $uninstallKeys = @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1'
    )
    foreach ($key in $uninstallKeys) {
        $properties = Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
        if ($properties -and $properties.PSObject.Properties['InstallLocation'] -and $properties.InstallLocation) {
            $candidate = Join-Path $properties.InstallLocation 'ISCC.exe'
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
        }
    }
    $fallbacks = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe')
    )
    foreach ($candidate in $fallbacks) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    return $null
}

Push-Location $repoRoot
try {
    # 1. Release build (no test hooks).
    cargo build --release -p narrate
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    # 2. The version line must be plain "narrate X.Y.Z"; a test-hooks build prints more and is refused.
    $exe = Join-Path $repoRoot 'target\release\narrate.exe'
    $versionLine = ((& $exe --version) | Out-String).Trim()
    if ($versionLine -notmatch '^narrate (\d+\.\d+\.\d+)$') {
        [Console]::Error.WriteLine("build.ps1: unexpected version output from $exe : '$versionLine' (a test-hooks build is not packaged)")
        exit 1
    }
    $version = $Matches[1]

    # 3 and 4. Find ISCC.exe or say how to install it.
    $iscc = Find-Iscc
    if (-not $iscc) {
        [Console]::Error.WriteLine('build.ps1: ISCC.exe (Inno Setup 6) was not found. Install it without admin rights with:')
        [Console]::Error.WriteLine("  $installCommand")
        exit 1
    }

    # 5. Compile and pass ISCC's exit code through.
    & $iscc "/DAppVersion=$version" (Join-Path $PSScriptRoot 'narrate.iss')
    $isccExit = $LASTEXITCODE
    if ($isccExit -ne 0) { exit $isccExit }

    # 6. Report the artifact.
    Write-Host (Join-Path $repoRoot "dist\narrate-setup-$version.exe")
    exit 0
}
finally {
    Pop-Location
}
