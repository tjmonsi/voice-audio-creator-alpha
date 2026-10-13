<#
.SYNOPSIS
Samples GPU memory use through NVML (nvidia-smi) until stopped (NFR-005, NFR-006, GPU QA items 10 and 11).

.DESCRIPTION
  sample_vram.ps1 -Out <file>

Runs
  nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits -lms 200
and appends one line per sample to <file>:
  <UTC time, ISO 8601 with milliseconds>  <memory.used in MiB>
(tab-separated). Record the idle baseline (the first samples before the run starts), then compare
the peak with it. "Within 256 MiB of the baseline within 10 s" is judged from these lines.

Stop it with Ctrl+C in its console, or kill the whole process tree so nvidia-smi goes away too:
  taskkill /T /F /PID <pid of this PowerShell process>
(a plain Stop-Process on the PowerShell process leaves nvidia-smi running). The lines of a machine
with more than one GPU alternate per GPU; this procedure assumes the single RTX 3050.

Interrupt runs (GPU QA item 10, and the interrupted download of spec-narrate-setup.md,
Requirement 9). PowerShell Start-Process cannot start a program in a new process group, which
Ctrl+Break delivery needs, so start narrate through the test-kit helper and send the signal to the
pid it prints:

  # terminal 1: sampling for the whole interrupt run
  powershell -NoProfile -File qa\vram\sample_vram.ps1 -Out vram-interrupt.tsv

  # terminal 2: start narrate in a new process group; the helper prints narrate's pid on its
  # first stdout line, keeps stdio, waits, and exits with narrate's exit code
  narrate-test-signal spawn-group <path>\narrate.exe <script> --verbose
  #   (for the interrupted download: narrate-test-signal spawn-group <path>\narrate.exe setup --verbose)

  # terminal 3, during chunk 2 (or while model files download): Ctrl+Break to that pid
  narrate-test-signal <narrate pid>
  # worker and uv pids come from the "verbose: worker pid" / "verbose: uv pid" lines; each must
  # report gone:
  narrate-test-signal alive <pid>          # exit 1 means the process is gone

For the second run of item 10, terminate narrate itself (Stop-Process on the pid) instead of
sending Ctrl+Break, then check the worker pid the same way. narrate-test-signal.exe is built by
cargo test --workspace --all-features and sits next to narrate.exe in target\debug.

Exit codes: 0 stopped normally; 1 nvidia-smi is missing or failed to start.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Out
)

$ErrorActionPreference = 'Stop'
$target = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out)

$smi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
if (-not $smi) {
    [Console]::Error.WriteLine('nvidia-smi was not found on PATH')
    exit 1
}

$info = New-Object System.Diagnostics.ProcessStartInfo
$info.FileName = $smi.Source
$info.Arguments = '--query-gpu=memory.used --format=csv,noheader,nounits -lms 200'
$info.UseShellExecute = $false
$info.RedirectStandardOutput = $true
$info.CreateNoWindow = $true

$proc = [System.Diagnostics.Process]::Start($info)
$writer = New-Object System.IO.StreamWriter($target, $true, (New-Object System.Text.UTF8Encoding($false)))
$writer.AutoFlush = $true
try {
    while ($null -ne ($line = $proc.StandardOutput.ReadLine())) {
        $value = $line.Trim()
        if ($value -eq '') { continue }
        $writer.WriteLine([DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ') + "`t" + $value)
    }
}
finally {
    $writer.Dispose()
    if (-not $proc.HasExited) { $proc.Kill() }
}
exit 0
