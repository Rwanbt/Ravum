#!/usr/bin/env pwsh
# lang-bench — wall time and peak memory of `check` / `build` on generated programs.
#
# Purpose: produce measurable numbers for the size classes the compiler actually
# sees, instead of repeating "the compiler is fast". The generated sources are
# synthetic but deterministic, so two runs on the same machine are comparable.
#
# What is measured, per size class:
#   * check wall time (median of N runs, cold process each time),
#   * check peak working set,
#   * build wall time (rustc included) and peak working set.
#
# What is NOT measured: statistical confidence, warm/incremental caches,
# other machines, other platforms. `ravum-lang build` shells out to `rustc`, so
# the build column measures the whole chain, not Ravum alone.
#
# Usage: pwsh -File scripts/lang-bench.ps1 [-Runs 3] [-Sizes 100,500,2000]

[CmdletBinding()]
param(
    [int]$Runs = 3,
    [string[]]$Sizes = @('100', '500', '2000'),
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'

# A comma list can reach this script either as a real array (`100,500`) or as a
# single string (`100,500`) depending on the caller. `[int[]]` conversion of the
# string silently concatenates the digits (`100,500` -> 100500), which used to
# generate a runaway program instead of three size classes. Parse explicitly and
# bound the sizes so a malformed invocation fails fast.
$parsedSizes = [System.Collections.Generic.List[int]]::new()
foreach ($raw in $Sizes) {
    foreach ($piece in ($raw -split ',')) {
        $trimmed = $piece.Trim()
        if ($trimmed -eq '') { continue }
        $value = 0
        if (-not [int]::TryParse($trimmed, [ref]$value) -or $value -lt 1 -or $value -gt 20000) {
            throw "invalid size '${piece}': expected an integer in 1..20000; separate multiple sizes with commas"
        }
        $parsedSizes.Add($value)
    }
}
if ($parsedSizes.Count -eq 0) { throw 'no size class provided' }
$Sizes = $parsedSizes.ToArray()
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$binary = Join-Path $root 'target\debug\ravum-lang.exe'
if (-not (Test-Path $binary)) {
    throw "ravum-lang not found at ${binary}: run cargo build -p ravum-lang-cli first"
}

$work = Join-Path $root 'target\bench'
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Force -Path $work | Out-Null

function New-Program {
    param([string]$Path, [int]$Functions)
    $lines = [System.Collections.Generic.List[string]]::new()
    $lines.Add('use std::io;')
    $lines.Add('')
    $lines.Add('fn main() {')
    $lines.Add('    let mut total: i32 = 0;')
    for ($i = 0; $i -lt $Functions; $i++) {
        $lines.Add("    let v${i}: i32 = f${i}(${i});")
        $lines.Add("    total = total + v${i};")
    }
    $lines.Add('    io::println(&total.to_string());')
    $lines.Add('}')
    for ($i = 0; $i -lt $Functions; $i++) {
        $lines.Add('')
        $lines.Add("fn f${i}(x: i32) -> i32 {")
        $lines.Add('    let a: i32 = x + 1;')
        $lines.Add('    let b: i32 = a * 2;')
        $lines.Add('    if b > 10 {')
        $lines.Add('        return b - 10;')
        $lines.Add('    }')
        $lines.Add('    return a + b;')
        $lines.Add('}')
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
    [System.IO.File]::WriteAllLines($Path, $lines, [System.Text.UTF8Encoding]::new($false))
    $manifest = Split-Path -Parent (Split-Path -Parent $Path)
    [System.IO.File]::WriteAllText(
        (Join-Path $manifest 'ravum.toml'),
        "[package]`nname = ""bench$(Split-Path -Leaf $manifest)""`nversion = ""0.1.0""`nedition = ""core-1.0""`n")
}

function Invoke-Measured {
    param([string]$Label, [string[]]$Arguments)
    $times = @()
    $peak = 0
    for ($run = 0; $run -lt $Runs; $run++) {
        $stdout = [System.IO.Path]::GetTempFileName()
        $stderr = [System.IO.Path]::GetTempFileName()
        $quoted = $Arguments | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }
        $watch = [System.Diagnostics.Stopwatch]::StartNew()
        $process = Start-Process -FilePath $binary -ArgumentList $quoted -NoNewWindow -PassThru `
            -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        # Sampling: `PeakWorkingSet64` is zero once the process has exited, so the
        # working set is sampled while it runs. The reported value is therefore a
        # lower bound on the true peak.
        while (-not $process.HasExited) {
            try {
                $process.Refresh()
                if ($process.WorkingSet64 -gt $peak) { $peak = $process.WorkingSet64 }
            }
            catch {
                # The process can vanish between HasExited and Refresh.
            }
            Start-Sleep -Milliseconds 5
        }
        $watch.Stop()
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) {
            $details = Get-Content $stderr -Raw -ErrorAction SilentlyContinue
            Remove-Item $stdout, $stderr -ErrorAction SilentlyContinue
            throw "${Label} failed with exit code $($process.ExitCode): $details"
        }
        Remove-Item $stdout, $stderr -ErrorAction SilentlyContinue
        $times += $watch.Elapsed.TotalMilliseconds
    }
    $sorted = $times | Sort-Object
    $median = $sorted[[int][math]::Floor($sorted.Count / 2)]
    [pscustomobject]@{
        Step         = $Label
        MedianMs     = [math]::Round($median, 1)
        MinMs        = [math]::Round($sorted[0], 1)
        MaxMs        = [math]::Round($sorted[-1], 1)
        PeakMemoryM  = [math]::Round($peak / 1MB, 1)
    }
}
$results = @()
foreach ($size in $Sizes) {
    $directory = Join-Path $work "n$size"
    $program = Join-Path $directory 'src\main.rv'
    New-Program -Path $program -Functions $size
    $lines = ([System.IO.File]::ReadAllLines($program)).Count
    Write-Host "lang-bench: $directory ($lines lines)"

    $results += Invoke-Measured -Label "check n=$size" -Arguments @('check', $directory)
    $results += Invoke-Measured -Label "build n=$size" -Arguments @('build', $directory)
}

$results | Format-Table -AutoSize
$results | ConvertTo-Json | Set-Content (Join-Path $work 'results.json') -Encoding utf8
Write-Host "lang-bench: wrote $(Join-Path $work 'results.json')"

if (-not $Keep) {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
