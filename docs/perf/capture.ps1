<#
.SYNOPSIS
  Capture Meteor's resource baseline into docs/perf/<label>.json.

.DESCRIPTION
  Every optimization phase in the roadmap has a numeric exit criterion, and this
  is what produces the numbers. Run it once before touching anything (the
  baseline) and again after each phase; compare with `-Compare <file>`.

  Uses only what ships with Windows: performance counters, CIM and file mtimes.

  What it measures, with Meteor RUNNING and hidden to the tray, no game open:
    * CPU%       — \Process(meteor)\% Processor Time, averaged over the window
    * Wakeups    — thread context switches/sec across meteor's threads
    * Memory     — Private Bytes of meteor.exe and of the WebView2 process tree
    * Disk       — files under %APPDATA%\com.alfonso.meteor whose mtime moved,
                   which is the honest measure of "writes while doing nothing"
    * Backends   — whether nvml.dll is loaded and the cputemp sidecar is running
                   (neither should be until a game runs, from phase 3 on)

.EXAMPLE
  # 1. Start Meteor, close the window so it sits in the tray, then:
  powershell -File docs\perf\capture.ps1 -Label baseline -Minutes 10

.EXAMPLE
  powershell -File docs\perf\capture.ps1 -Label phase3 -Compare docs\perf\baseline.json
#>
[CmdletBinding()]
param(
    [string]$Label = 'baseline',
    [double]$Minutes = 10,
    [string]$Compare,
    [string]$OutDir
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# `$PSScriptRoot` is empty in some invocation shapes (dot-sourcing, `-Command`
# with a relative path); fall back to the script's own location so `New-Item`
# never receives an empty path at the very end of a multi-minute capture.
if (-not $OutDir) {
    $OutDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
}

function Get-MeteorProcess {
    $p = Get-Process -Name meteor -ErrorAction SilentlyContinue
    if (-not $p) { throw 'meteor.exe is not running. Start Meteor and close its window to the tray first.' }
    if ($p -is [array]) { $p = $p[0] }
    return $p
}

function Get-WebViewTree($parentId) {
    # WebView2 spawns a browser process plus renderer/GPU children; sum the tree.
    $all = Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'"
    $direct = @($all | Where-Object { $_.ParentProcessId -eq $parentId })
    $ids = [System.Collections.Generic.HashSet[int]]::new()
    foreach ($d in $direct) { [void]$ids.Add([int]$d.ProcessId) }
    # One level of grandchildren is enough for the WebView2 tree shape.
    foreach ($a in $all) { if ($ids.Contains([int]$a.ParentProcessId)) { [void]$ids.Add([int]$a.ProcessId) } }
    return @($all | Where-Object { $ids.Contains([int]$_.ProcessId) })
}

# Performance-counter paths are LOCALIZED: on a Spanish Windows the object is
# "Proceso" and the counter "% de tiempo de procesador", so the English literal
# `\Process(meteor)\% Processor Time` returns nothing (and under StrictMode the
# `.CounterSamples` access throws). Perflib assigns every object/counter a
# language-neutral index; resolve the names through the current-language table.
$script:PerfNames = $null
function Resolve-CounterName([int]$index) {
    if (-not $script:PerfNames) {
        $raw = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Perflib\CurrentLanguage').Counter
        $script:PerfNames = @{}
        for ($i = 0; $i -lt $raw.Count - 1; $i += 2) { $script:PerfNames[[int]$raw[$i]] = $raw[$i + 1] }
    }
    $name = $script:PerfNames[$index]
    if (-not $name) { throw "Perflib index $index has no name in the current language table." }
    return $name
}
$objProcess  = Resolve-CounterName 230   # Process
$ctrCpu      = Resolve-CounterName 6     # % Processor Time
$objThread   = Resolve-CounterName 232   # Thread
$ctrCtx      = Resolve-CounterName 146   # Context Switches/sec

$dataDir = Join-Path $env:APPDATA 'com.alfonso.meteor'
$proc = Get-MeteorProcess
Write-Host "Sampling meteor.exe (pid $($proc.Id)) for $Minutes min. Leave it idle in the tray."

$before = @{}
if (Test-Path $dataDir) {
    Get-ChildItem $dataDir -Recurse -File | ForEach-Object { $before[$_.FullName] = $_.LastWriteTimeUtc }
}

$samples = [int][math]::Max(2, ($Minutes * 60) / 5)
$cpu = (Get-Counter "\$objProcess(meteor)\$ctrCpu" -SampleInterval 5 -MaxSamples $samples).CounterSamples
$cpuValues = @($cpu | ForEach-Object { $_.CookedValue })
$cores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors

# Context switches per second across meteor's threads = how often it wakes up.
# The `meteor/*` instance set changes between samples (short-lived tokio
# workers), and Get-Counter throws on any invalid sample unless told not to;
# keep only samples with Status 0 and average over the samples we did get.
$ctxRaw = Get-Counter "\$objThread(meteor/*)\$ctrCtx" -SampleInterval 5 -MaxSamples 3 -ErrorAction SilentlyContinue
$ctxTotal = 0
if ($ctxRaw) {
    $ctx = @($ctxRaw | ForEach-Object { $_.CounterSamples } | Where-Object { $_.Status -eq 0 })
    $rounds = @($ctxRaw).Count
    if ($ctx -and $rounds -gt 0) { $ctxTotal = [math]::Round((($ctx | Measure-Object CookedValue -Sum).Sum) / $rounds, 1) }
}

$after = @{}
if (Test-Path $dataDir) {
    Get-ChildItem $dataDir -Recurse -File | ForEach-Object { $after[$_.FullName] = $_.LastWriteTimeUtc }
}
$touched = @()
foreach ($k in $after.Keys) {
    if (-not $before.ContainsKey($k) -or $before[$k] -ne $after[$k]) {
        $touched += (Split-Path $k -Leaf)
    }
}

$proc.Refresh()
$webview = Get-WebViewTree $proc.Id
$webviewMb = 0
if ($webview.Count -gt 0) {
    $webviewMb = [math]::Round((($webview | Measure-Object -Property WorkingSetSize -Sum).Sum) / 1MB, 1)
}
$modules = @($proc.Modules | ForEach-Object { $_.ModuleName })

$result = [ordered]@{
    label            = $Label
    captured_utc     = (Get-Date).ToUniversalTime().ToString('o')
    commit           = (& git rev-parse --short HEAD 2>$null)
    minutes          = $Minutes
    machine          = [ordered]@{
        os     = (Get-CimInstance Win32_OperatingSystem).Caption
        cpu    = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
        cores  = $cores
        ram_gb = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB, 1)
    }
    idle             = [ordered]@{
        cpu_percent_mean    = if ($cpuValues.Count) { [math]::Round((($cpuValues | Measure-Object -Average).Average) / $cores, 3) } else { $null }
        cpu_percent_max     = if ($cpuValues.Count) { [math]::Round((($cpuValues | Measure-Object -Maximum).Maximum) / $cores, 3) } else { $null }
        context_switches_s  = $ctxTotal
        threads             = $proc.Threads.Count
        meteor_private_mb   = [math]::Round($proc.PrivateMemorySize64 / 1MB, 1)
        meteor_workingset_mb = [math]::Round($proc.WorkingSet64 / 1MB, 1)
        webview_processes   = $webview.Count
        webview_workingset_mb = $webviewMb
        files_written       = @($touched | Sort-Object -Unique)
        files_written_count = @($touched).Count
        nvml_loaded         = [bool]($modules -contains 'nvml.dll')
        sidecar_running     = [bool](Get-Process cputemp -ErrorAction SilentlyContinue)
    }
    caches           = [ordered]@{
        covers_mb    = if (Test-Path "$dataDir\covers") { [math]::Round((Get-ChildItem "$dataDir\covers" -File | Measure-Object Length -Sum).Sum / 1MB, 1) } else { 0 }
        app_icons_mb = if (Test-Path "$dataDir\app_icons") { [math]::Round((Get-ChildItem "$dataDir\app_icons" -File | Measure-Object Length -Sum).Sum / 1MB, 1) } else { 0 }
    }
    artifacts        = [ordered]@{
        meteor_exe_mb  = if (Test-Path 'src-tauri/target/release/meteor.exe') { [math]::Round((Get-Item 'src-tauri/target/release/meteor.exe').Length / 1MB, 1) } else { $null }
        cputemp_exe_mb = if (Test-Path 'src-tauri/binaries/cputemp.exe') { [math]::Round((Get-Item 'src-tauri/binaries/cputemp.exe').Length / 1MB, 1) } else { $null }
        installer_mb   = if (Test-Path 'src-tauri/target/release/bundle/nsis') { [math]::Round(((Get-ChildItem 'src-tauri/target/release/bundle/nsis' -Filter *.exe | Measure-Object Length -Sum).Sum) / 1MB, 1) } else { $null }
    }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$outFile = Join-Path $OutDir "$Label.json"
$result | ConvertTo-Json -Depth 6 | Set-Content -Path $outFile -Encoding utf8
Write-Host "`nWrote $outFile"
$result.idle | Format-List

if ($Compare) {
    $base = Get-Content $Compare -Raw | ConvertFrom-Json
    Write-Host "`n=== vs $Compare ==="
    foreach ($k in 'cpu_percent_mean', 'context_switches_s', 'meteor_private_mb', 'webview_workingset_mb', 'files_written_count') {
        $old = $base.idle.$k
        $new = $result.idle.$k
        $delta = if ($old -and $old -ne 0) { "{0:P0}" -f (($new - $old) / $old) } else { 'n/a' }
        Write-Host ("{0,-24} {1,10} -> {2,10}  ({3})" -f $k, $old, $new, $delta)
    }
}
