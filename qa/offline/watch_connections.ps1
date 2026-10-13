<#
.SYNOPSIS
Records every non-local TCP connection of one process until it exits (FR-012, GPU QA item 7).

.DESCRIPTION
  watch_connections.ps1 -ProcessId <pid> -Out <file>

Every 500 ms until the process exits, runs Get-NetTCPConnection -OwningProcess <pid> and appends
the rows whose remote address is not 127.0.0.1, ::1, 0.0.0.0 or :: to <file> as
  <UTC time>  <local address:port>  <remote address:port>  <state>  <pid>
(tab-separated). The file is created empty at the start, so an empty file means nothing was seen.

Start it on the worker pid from the "verbose: worker pid <pid>" line as soon as that line appears,
and let it run until the worker exits.

Exit codes: 0 the process ended and no remote connection was recorded; 1 at least one was
recorded; 2 the process did not exist when the script started (nothing was watched).
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [int]$ProcessId,
    [Parameter(Mandatory = $true)]
    [string]$Out
)

$ErrorActionPreference = 'Stop'
$local = @('127.0.0.1', '::1', '0.0.0.0', '::')
$target = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out)
[System.IO.File]::WriteAllText($target, '')

if (-not (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine("process $ProcessId does not exist; nothing was watched")
    exit 2
}

$recorded = 0
$polls = 0
while ($true) {
    $rows = @(Get-NetTCPConnection -OwningProcess $ProcessId -ErrorAction SilentlyContinue)
    $polls++
    foreach ($row in $rows) {
        if ($local -contains $row.RemoteAddress) { continue }
        $line = (
            [DateTime]::UtcNow.ToString('o'),
            ($row.LocalAddress + ':' + $row.LocalPort),
            ($row.RemoteAddress + ':' + $row.RemotePort),
            [string]$row.State,
            [string]$row.OwningProcess
        ) -join "`t"
        [System.IO.File]::AppendAllText($target, $line + "`r`n")
        $recorded++
    }
    if (-not (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) { break }
    Start-Sleep -Milliseconds 500
}

Write-Output "process $ProcessId ended after $polls polls; remote connection rows recorded: $recorded"
if ($recorded -gt 0) { exit 1 }
exit 0
