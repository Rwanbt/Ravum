# Lead's independent verification of `watch --run` fail-closed behaviour (P0-C).
#
# Scenario A: an edit that does not compile must NOT interrupt the running child.
# Scenario B: a later valid edit activates the new version (one child only).
# Scenario C: with --max-rebuilds 1, watch exits and leaves no orphan child.
#
# Independent of the agent's own tests: it drives the real binary and reads real
# process state. Hermetic (temp dirs under target/), no network.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path,
    [string]$Toolchain = "",
    [int]$StartupTimeoutSec = 90,
    [int]$RebuildTimeoutSec = 90
)
$ErrorActionPreference = "Stop"
if (-not $Toolchain) { $Toolchain = Join-Path $RepoRoot "target\debug\ravum-lang.exe" }
if (-not (Test-Path $Toolchain)) { throw "toolchain not found: $Toolchain" }

$work = Join-Path $RepoRoot "target\lead-watch"
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Path $work | Out-Null

$failures = New-Object System.Collections.ArrayList
function Check($name, [bool]$ok, $detail) {
    $status = if ($ok) { "PASS" } else { "FAIL" }
    Write-Host ("[{0}] {1} {2}" -f $status, $name, $detail)
    if (-not $ok) { [void]$failures.Add($name) }
}

function New-Package([string]$dir) {
    New-Item -ItemType Directory -Force -Path (Join-Path $dir "src") | Out-Null
    @'
[package]
name = "watched"
version = "0.1.0"
edition = "core-1.0"
'@ | Set-Content (Join-Path $dir "ravum.toml")
    Set-Version $dir "VERSION1"
}
function Set-Version([string]$dir, [string]$marker) {
    @"
use std::io;
fn main() {
    io::println("$marker");
    loop { }
}
"@ | Set-Content (Join-Path $dir "src\main.rv") -NoNewline
}
function Set-Invalid([string]$dir) {
    "fn main( {" | Set-Content (Join-Path $dir "src\main.rv") -NoNewline
}

function Get-Children([string]$root) {
    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root, [StringComparison]::OrdinalIgnoreCase) -and $_.Name -ne "ravum-lang.exe" }
}
function Wait-For([scriptblock]$predicate, [int]$timeoutSec, [string]$label) {
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (& $predicate) { return $true }
        Start-Sleep -Milliseconds 250
    }
    Write-Host "  (timeout waiting for $label)"
    return $false
}
function Cleanup-Root([string]$root) {
    Get-Children $root | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

# ---------------------------------------------------------------- scenario A+B
$pkg = Join-Path $work "ab"
New-Package $pkg
$outFile = Join-Path $work "ab.out"
$errFile = Join-Path $work "ab.err"
$watch = Start-Process -FilePath $Toolchain -ArgumentList @("watch", "--run", $pkg) -Wait:$false -PassThru -NoNewWindow -RedirectStandardOutput $outFile -RedirectStandardError $errFile
$childRoot = (Resolve-Path $pkg).Path
try {
    $started = Wait-For { (Get-Content $outFile -Raw -ErrorAction SilentlyContinue) -match "VERSION1" } $StartupTimeoutSec "VERSION1"
    Check "initial-version-runs" $started (Get-Content $outFile -Raw -ErrorAction SilentlyContinue)
    $initialChildren = @(Get-Children $childRoot)
    Check "initial-child-present" ($initialChildren.Count -ge 1) ("children=" + ($initialChildren | ForEach-Object { $_.ProcessId }) -join ",")
    $initialPids = $initialChildren | ForEach-Object { $_.ProcessId }

    # A: invalid edit — the old child must survive.
    Set-Invalid $pkg
    $failedReported = Wait-For { (Get-Content $outFile -Raw -ErrorAction SilentlyContinue) -match "build failed" } $RebuildTimeoutSec "build failed"
    Start-Sleep -Seconds 2
    $aliveAfterFailure = @(Get-Children $childRoot | Where-Object { $initialPids -contains $_.ProcessId })
    Check "invalid-edit-old-version-survives" (($aliveAfterFailure.Count -ge 1) -and $failedReported) ("alive=" + ($aliveAfterFailure | ForEach-Object { $_.ProcessId }) + " failedReported=" + $failedReported)

    # B: valid edit — the new version must activate, exactly one child.
    Set-Version $pkg "VERSION2"
    $activated = Wait-For { (Get-Content $outFile -Raw -ErrorAction SilentlyContinue) -match "VERSION2" } $RebuildTimeoutSec "VERSION2"
    Start-Sleep -Seconds 2
    $childrenNow = @(Get-Children $childRoot)
    Check "valid-edit-activates-new-version" ($activated -and $childrenNow.Count -ge 1) ("activated=" + $activated + " children=" + ($childrenNow | ForEach-Object { $_.ProcessId }))
    Check "exactly-one-child-after-swap" ($childrenNow.Count -eq 1) ("count=" + $childrenNow.Count)
} finally {
    Stop-Process -Id $watch.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
    Cleanup-Root $childRoot
}

# ---------------------------------------------------------------- scenario C
$pkg2 = Join-Path $work "c"
New-Package $pkg2
$outFile2 = Join-Path $work "c.out"
$errFile2 = Join-Path $work "c.err"
$watch2 = Start-Process -FilePath $Toolchain -ArgumentList @("watch", "--run", "--max-rebuilds", "1", $pkg2) -Wait:$false -PassThru -NoNewWindow -RedirectStandardOutput $outFile2 -RedirectStandardError $errFile2
$childRoot2 = (Resolve-Path $pkg2).Path
try {
    [void](Wait-For { (Get-Content $outFile2 -Raw -ErrorAction SilentlyContinue) -match "VERSION1" } $StartupTimeoutSec "VERSION1 (c)")
    Set-Version $pkg2 "VERSION2"
    [void](Wait-For { (Get-Content $outFile2 -Raw -ErrorAction SilentlyContinue) -match "VERSION2" } $RebuildTimeoutSec "VERSION2 (c)")
    $exited = $watch2.WaitForExit(30000)
    Check "max-rebuilds-exits" $exited ("exit=" + $watch2.ExitCode)
    Start-Sleep -Seconds 1
    $left = @(Get-Children $childRoot2)
    Check "no-orphan-child-after-exit" ($left.Count -eq 0) ("left=" + ($left | ForEach-Object { $_.ProcessId }))
    Check "temp-watch-dir-cleaned" (-not (Test-Path (Join-Path $pkg2 ".ravum\watch"))) "dir exists or not"
} finally {
    if (-not $watch2.HasExited) { Stop-Process -Id $watch2.Id -Force -ErrorAction SilentlyContinue }
    Cleanup-Root $childRoot2
}

Write-Host ""
if ($failures.Count -gt 0) {
    Write-Host ("WATCH CHECK: FAIL ({0} failing assertions): {1}" -f $failures.Count, ($failures -join ", "))
    exit 1
}
Write-Host "WATCH CHECK: PASS (fail-closed, activation, no orphan)"
