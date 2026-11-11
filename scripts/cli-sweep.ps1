#!/usr/bin/env pwsh
# cli-sweep — end-to-end checks for the CLI commands the example smoke does not run.
#
# `scripts/lang-smoke.ps1` exercises init/check/build/run/test/fmt. That leaves
# `lint`, `doc`, `cache`, `doctor` and `watch` without an end-to-end guard, so a
# regression in those paths would only be caught by unit tests — or not at all.
#
# Each case states the command, the expected exit code and a substring the output
# must contain. Everything runs on a throwaway package under `target/cli-sweep`.
#
# Usage: pwsh -File scripts/cli-sweep.ps1 [-Keep]

[CmdletBinding()]
param([switch]$Keep)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$exe = if ($IsWindows -or $env:OS -eq 'Windows_NT') { 'ravum-lang.exe' } else { 'ravum-lang' }
$binary = Join-Path $root (Join-Path 'target' (Join-Path 'debug' $exe))
if (-not (Test-Path $binary)) {
    throw "ravum-lang not found at ${binary}: run cargo build -p ravum-lang-cli first"
}

$work = Join-Path $root (Join-Path 'target' 'cli-sweep')
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Force -Path $work | Out-Null

$clean = Join-Path $work 'clean'
$unused = Join-Path $work 'unused-import'
$messy = Join-Path $work 'messy'

function New-Package {
    param([string]$Directory, [string]$Name, [string]$Source)
    New-Item -ItemType Directory -Force -Path (Join-Path $Directory 'src') | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $Directory 'ravum.toml'),
        "[package]`nname = ""$Name""`nversion = ""0.1.0""`nedition = ""core-1.0""`n")
    [System.IO.File]::WriteAllText((Join-Path (Join-Path $Directory 'src') 'main.rv'), $Source)
}

New-Package -Directory $clean -Name 'sweepclean' -Source @'
use std::io;

/// Adds two numbers.
pub fn add(a: i32, b: i32) -> i32 {
    return a + b;
}

fn main() {
    io::println(&add(2, 3).to_string());
}
'@

New-Package -Directory $unused -Name 'sweepunused' -Source @'
use std::io;
use std::math;

fn main() {
    io::println("x");
}
'@

New-Package -Directory $messy -Name 'sweepmessy' -Source @'
fn main()    {
        let x: i32=1;
}
'@

$results = @()
function Invoke-Case {
    param([string]$Id, [string[]]$Arguments, [int]$ExitCode, [string]$Contains, [string]$WorkingDirectory)
    $previous = Get-Location
    if ($WorkingDirectory) { Set-Location $WorkingDirectory }
    try {
        $output = & $binary @Arguments 2>&1
        $code = $LASTEXITCODE
        $text = ($output | Out-String)
        $problems = @()
        if ($code -ne $ExitCode) { $problems += "exit $code (expected $ExitCode)" }
        if ($Contains -and $text -notlike "*$Contains*") { $problems += "missing `"$Contains`"" }
        $script:results += [pscustomobject]@{
            Case   = $Id
            Status = if ($problems.Count -eq 0) { 'PASS' } else { 'FAIL' }
            Detail = ($problems -join '; ')
        }
    }
    finally {
        Set-Location $previous
    }
}

Invoke-Case -Id 'lint-clean' -Arguments @('lint', $clean) -ExitCode 0 -Contains '0 denied'
Invoke-Case -Id 'lint-warns' -Arguments @('lint', $unused) -ExitCode 0 -Contains '1 warning'
Invoke-Case -Id 'lint-deny' -Arguments @('lint', '--deny', 'W-RES-UNUSED-IMPORT', $unused) -ExitCode 1 -Contains 'denied: 1'
Invoke-Case -Id 'doc-writes-index' -Arguments @('doc', $clean) -ExitCode 0 -Contains 'index.md'
# Capture the written documentation *before* `cache clean` deletes it.
$docIndex = Join-Path $clean '.ravum/doc/index.md'
$docContents = if (Test-Path $docIndex) { Get-Content $docIndex -Raw } else { '' }
Invoke-Case -Id 'doctor-json' -Arguments @('doctor', '--json') -ExitCode 0 -Contains '"verdict":"ready"'
Invoke-Case -Id 'cache-status' -Arguments @('cache', 'status', $clean) -ExitCode 0 -Contains 'size:'
Invoke-Case -Id 'fmt-apply' -Arguments @('fmt', $clean) -ExitCode 0 -Contains ''
Invoke-Case -Id 'fmt-check-clean' -Arguments @('fmt', '--check', $clean) -ExitCode 0 -Contains ''
Invoke-Case -Id 'fmt-check-messy' -Arguments @('fmt', '--check', $messy) -ExitCode 1 -Contains ''
Invoke-Case -Id 'cache-clean' -Arguments @('cache', 'clean', $clean) -ExitCode 0 -Contains ''

$results += [pscustomobject]@{
    Case   = 'doc-content'
    Status = if ($docContents -match 'add') { 'PASS' } else { 'FAIL' }
    Detail = if ($docContents -match 'add') { '' } else { 'index.md does not mention the documented function' }
}
$results += [pscustomobject]@{
    Case   = 'cache-removed'
    Status = if (-not (Test-Path (Join-Path $clean '.ravum'))) { 'PASS' } else { 'FAIL' }
    Detail = if (Test-Path (Join-Path $clean '.ravum')) { '.ravum still present after cache clean' } else { '' }
}

# `watch --max-rebuilds 0` builds once and exits: a bounded way to exercise the
# watch loop without a timeout.
Invoke-Case -Id 'watch-bounded' -Arguments @('watch', '--max-rebuilds', '0', $clean) -ExitCode 0 -Contains 'build ok'

$results | Format-Table -AutoSize
$failed = @($results | Where-Object { $_.Status -ne 'PASS' })
Write-Host ("RESUME: {0} PASS / {1} FAIL (of {2})" -f ($results.Count - $failed.Count), $failed.Count, $results.Count)

if (-not $Keep) { Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue }
if ($failed.Count -gt 0) { exit 1 }
exit 0
