<#
.SYNOPSIS
Records and compares the user-level locations that narrate setup must not touch (NFR-002).

.DESCRIPTION
  snapshot.ps1 -Out <file>
  snapshot.ps1 -Compare <before> <after>

-Out records, as a text file (tab-separated lines, UTF-8):
  * %LOCALAPPDATA%\uv, %APPDATA%\uv, %USERPROFILE%\.local\bin, %USERPROFILE%\.cache\huggingface and
    %USERPROFILE%\.cache\torch: existence and a recursive list of path, size and last-write time;
  * HKCU:\Software\Python: existence and all subkeys and values;
  * the user Path value (read without expanding variables);
  * the names of the top-level entries of %USERPROFILE%, %APPDATA%, %LOCALAPPDATA% and %TEMP%;
  * for every top-level entry of %TEMP%, the files below it whose name contains "espeak".

-Compare prints the differences between two snapshot files and exits 1 when any of the five
locations, the registry key, or the user Path value differs. New top-level entries of the four
directories are printed for QA to attribute (other programs write there too) and do not change the
exit code, except that a new %TEMP% entry that contains a file whose name contains "espeak" (the
phonemizer leftover that the TEMP/TMP redirection prevents) also exits 1.

Exit codes: 0 no difference; 1 differences; 2 usage or unreadable snapshot.
#>
[CmdletBinding()]
param(
    [string]$Out,
    [string]$Compare,
    [Parameter(Position = 0)]
    [string]$After
)

$ErrorActionPreference = 'Stop'
$MaxShown = 100

function Esc([string]$Text) {
    if ($null -eq $Text) { return '' }
    return $Text.Replace("`t", '<TAB>').Replace("`r", '<CR>').Replace("`n", '<LF>')
}

function Get-Locations {
    @(
        @{ Name = '%LOCALAPPDATA%\uv'; Path = (Join-Path $env:LOCALAPPDATA 'uv') },
        @{ Name = '%APPDATA%\uv'; Path = (Join-Path $env:APPDATA 'uv') },
        @{ Name = '%USERPROFILE%\.local\bin'; Path = (Join-Path $env:USERPROFILE '.local\bin') },
        @{ Name = '%USERPROFILE%\.cache\huggingface'; Path = (Join-Path $env:USERPROFILE '.cache\huggingface') },
        @{ Name = '%USERPROFILE%\.cache\torch'; Path = (Join-Path $env:USERPROFILE '.cache\torch') }
    )
}

function Get-Roots {
    @(
        @{ Name = 'USERPROFILE'; Path = $env:USERPROFILE },
        @{ Name = 'APPDATA'; Path = $env:APPDATA },
        @{ Name = 'LOCALAPPDATA'; Path = $env:LOCALAPPDATA },
        @{ Name = 'TEMP'; Path = $env:TEMP }
    )
}

function Resolve-OutPath([string]$Path) {
    $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
}

function New-Snapshot {
    $lines = New-Object 'System.Collections.Generic.List[string]'
    $lines.Add("format`t1")
    $lines.Add("taken`t" + [DateTime]::UtcNow.ToString('o'))

    foreach ($loc in Get-Locations) {
        $exists = Test-Path -LiteralPath $loc.Path
        $lines.Add("loc`t" + $loc.Name + "`t" + $exists + "`t" + (Esc $loc.Path))
        if (-not $exists) { continue }
        $base = (Get-Item -LiteralPath $loc.Path -Force).FullName
        $items = @(Get-ChildItem -LiteralPath $loc.Path -Recurse -Force -ErrorAction SilentlyContinue)
        foreach ($item in ($items | Sort-Object FullName)) {
            $rel = $item.FullName.Substring($base.Length).TrimStart('\')
            $size = '-'
            if (-not $item.PSIsContainer) { $size = [string]$item.Length }
            $lines.Add("entry`t" + $loc.Name + "`t" + (Esc $rel) + "`t" + $size + "`t" + $item.LastWriteTimeUtc.ToString('o'))
        }
    }

    $regPath = 'HKCU:\Software\Python'
    $regExists = Test-Path -LiteralPath $regPath
    $lines.Add("reg`t" + $regExists)
    if ($regExists) {
        $keys = @(Get-Item -LiteralPath $regPath) + @(Get-ChildItem -LiteralPath $regPath -Recurse -ErrorAction SilentlyContinue)
        $regLines = New-Object 'System.Collections.Generic.List[string]'
        foreach ($key in $keys) {
            $regLines.Add("KEY " + (Esc $key.Name))
            foreach ($name in $key.GetValueNames()) {
                $kind = $key.GetValueKind($name)
                $value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                if ($value -is [byte[]]) { $text = [BitConverter]::ToString($value) }
                elseif ($value -is [string[]]) { $text = $value -join '|' }
                else { $text = [string]$value }
                $regLines.Add("VALUE " + (Esc $key.Name) + " [" + (Esc $name) + "] (" + $kind + ") = " + (Esc $text))
            }
        }
        foreach ($l in ($regLines | Sort-Object)) { $lines.Add("regline`t" + $l) }
    }

    $envKey = Get-Item -LiteralPath 'HKCU:\Environment'
    $userPath = $envKey.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    if ($null -eq $userPath) { $lines.Add("userpath`t<absent>") } else { $lines.Add("userpath`t" + (Esc $userPath)) }

    foreach ($root in Get-Roots) {
        if (-not $root.Path -or -not (Test-Path -LiteralPath $root.Path)) { continue }
        $names = @(Get-ChildItem -LiteralPath $root.Path -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.Name } | Sort-Object)
        foreach ($n in $names) { $lines.Add("top`t" + $root.Name + "`t" + (Esc $n)) }
    }

    if ($env:TEMP -and (Test-Path -LiteralPath $env:TEMP)) {
        foreach ($entry in @(Get-ChildItem -LiteralPath $env:TEMP -Force -ErrorAction SilentlyContinue)) {
            $hits = @()
            if ($entry.PSIsContainer) {
                $base = $entry.FullName
                $hits = @(Get-ChildItem -LiteralPath $base -Recurse -Force -File -Filter '*espeak*' -ErrorAction SilentlyContinue |
                    ForEach-Object { $_.FullName.Substring($base.Length).TrimStart('\') })
            }
            elseif ($entry.Name -like '*espeak*') {
                $hits = @($entry.Name)
            }
            foreach ($hit in $hits) { $lines.Add("espeak`t" + (Esc $entry.Name) + "`t" + (Esc $hit)) }
        }
    }
    return , $lines
}

function Read-Snapshot([string]$File) {
    $snap = @{ Loc = @{}; LocPath = @{}; RegExists = $null; Reg = @{}; UserPath = $null; Top = @{}; Espeak = @{} }
    foreach ($root in Get-Roots) { $snap.Top[$root.Name] = @{} }
    foreach ($line in [System.IO.File]::ReadAllLines($File)) {
        $f = $line.Split("`t")
        switch ($f[0]) {
            'loc' { $snap.Loc[$f[1]] = @{ Exists = ($f[2] -eq 'True'); Entries = @{} }; $snap.LocPath[$f[1]] = $f[3] }
            'entry' { $snap.Loc[$f[1]].Entries[$f[2]] = $f[3] + ' ' + $f[4] }
            'reg' { $snap.RegExists = ($f[1] -eq 'True') }
            'regline' { $snap.Reg[$f[1]] = $true }
            'userpath' { $snap.UserPath = $f[1] }
            'top' {
                if (-not $snap.Top.ContainsKey($f[1])) { $snap.Top[$f[1]] = @{} }
                $snap.Top[$f[1]][$f[2]] = $true
            }
            'espeak' {
                if (-not $snap.Espeak.ContainsKey($f[1])) { $snap.Espeak[$f[1]] = New-Object 'System.Collections.Generic.List[string]' }
                $snap.Espeak[$f[1]].Add($f[2])
            }
        }
    }
    if ($null -eq $snap.UserPath) { throw "$File is not a snapshot file (no userpath line)" }
    return $snap
}

function Show-List([string]$Prefix, $Items) {
    $n = 0
    foreach ($i in $Items) {
        $n++
        if ($n -le $MaxShown) { Write-Output ($Prefix + $i) }
    }
    if ($n -gt $MaxShown) { Write-Output ($Prefix + '... and ' + ($n - $MaxShown) + ' more') }
}

function Compare-Snapshots([string]$BeforeFile, [string]$AfterFile) {
    $a = Read-Snapshot $BeforeFile
    $b = Read-Snapshot $AfterFile
    $differs = $false

    $names = @($b.Loc.Keys) + @($a.Loc.Keys | Where-Object { -not $b.Loc.ContainsKey($_) })
    foreach ($name in $names) {
        $la = $a.Loc[$name]
        $lb = $b.Loc[$name]
        if ($null -eq $la -or $null -eq $lb) {
            Write-Output "DIFF location ${name}: present in only one snapshot"
            $differs = $true
            continue
        }
        $changed = $false
        if ($la.Exists -ne $lb.Exists) {
            Write-Output "DIFF ${name}: exists before=$($la.Exists) after=$($lb.Exists)"
            $changed = $true
        }
        $added = @($lb.Entries.Keys | Where-Object { -not $la.Entries.ContainsKey($_) } | Sort-Object)
        $removed = @($la.Entries.Keys | Where-Object { -not $lb.Entries.ContainsKey($_) } | Sort-Object)
        $modified = @($lb.Entries.Keys | Where-Object { $la.Entries.ContainsKey($_) -and $la.Entries[$_] -ne $lb.Entries[$_] } | Sort-Object)
        if ($added.Count -gt 0 -or $removed.Count -gt 0 -or $modified.Count -gt 0) {
            Write-Output "DIFF ${name}: $($added.Count) added, $($removed.Count) removed, $($modified.Count) changed"
            Show-List '  added: ' $added
            Show-List '  removed: ' $removed
            Show-List '  changed: ' $modified
            $changed = $true
        }
        if ($changed) { $differs = $true } else { Write-Output "same ${name}" }
    }

    if ($a.RegExists -ne $b.RegExists) {
        Write-Output "DIFF HKCU:\Software\Python: exists before=$($a.RegExists) after=$($b.RegExists)"
        $differs = $true
    }
    $regAdded = @($b.Reg.Keys | Where-Object { -not $a.Reg.ContainsKey($_) } | Sort-Object)
    $regRemoved = @($a.Reg.Keys | Where-Object { -not $b.Reg.ContainsKey($_) } | Sort-Object)
    if ($regAdded.Count -gt 0 -or $regRemoved.Count -gt 0) {
        Write-Output "DIFF HKCU:\Software\Python: $($regAdded.Count) added, $($regRemoved.Count) removed"
        Show-List '  added: ' $regAdded
        Show-List '  removed: ' $regRemoved
        $differs = $true
    }
    elseif ($a.RegExists -eq $b.RegExists) {
        Write-Output 'same HKCU:\Software\Python'
    }

    if ($a.UserPath -cne $b.UserPath) {
        Write-Output 'DIFF user Path value'
        Write-Output "  before: $($a.UserPath)"
        Write-Output "  after:  $($b.UserPath)"
        $differs = $true
    }
    else {
        Write-Output 'same user Path value'
    }

    foreach ($root in Get-Roots) {
        $rootName = $root.Name
        $before = $a.Top[$rootName]
        $after = $b.Top[$rootName]
        $new = @($after.Keys | Where-Object { -not $before.ContainsKey($_) } | Sort-Object)
        $gone = @($before.Keys | Where-Object { -not $after.ContainsKey($_) } | Sort-Object)
        if ($new.Count -gt 0) {
            Write-Output "NEW top-level entries in %${rootName}% (QA attributes them): $($new.Count)"
            Show-List '  new: ' $new
        }
        if ($gone.Count -gt 0) {
            Write-Output "GONE top-level entries in %${rootName}%: $($gone.Count)"
            Show-List '  gone: ' $gone
        }
        if ($rootName -eq 'TEMP') {
            foreach ($entry in $new) {
                if ($b.Espeak.ContainsKey($entry)) {
                    foreach ($file in $b.Espeak[$entry]) {
                        Write-Output "DIFF new %TEMP% entry '$entry' contains an espeak file: $file"
                    }
                    $differs = $true
                }
            }
        }
    }

    $script:Differs = $differs
    if ($differs) { Write-Output 'RESULT: differences found' } else { Write-Output 'RESULT: no differences' }
}

if ($Out -and $Compare) {
    [Console]::Error.WriteLine('use either -Out <file> or -Compare <before> <after>, not both')
    exit 2
}
if ($Out) {
    $target = Resolve-OutPath $Out
    $lines = New-Snapshot
    [System.IO.File]::WriteAllLines($target, [string[]]$lines.ToArray(), (New-Object System.Text.UTF8Encoding($false)))
    Write-Output "snapshot written to $target"
    exit 0
}
if ($Compare -and $After) {
    $script:Differs = $false
    try {
        Compare-Snapshots (Resolve-OutPath $Compare) (Resolve-OutPath $After)
    }
    catch {
        [Console]::Error.WriteLine($_.Exception.Message)
        exit 2
    }
    if ($script:Differs) { exit 1 }
    exit 0
}
[Console]::Error.WriteLine('usage: snapshot.ps1 -Out <file> | snapshot.ps1 -Compare <before> <after>')
exit 2
