#!/usr/bin/env pwsh
# Repro-check — prove that `ravum-lang` builds bit-for-bit identically.
#
# Two builds are performed from *different absolute root paths* into *different
# target directories*; the SHA-256 of the produced binary must match. Anything
# less (same path, same target dir) proves nothing about path independence.
#
# Why each setting is required (measured 2026-11-09, Windows 11, rustc 1.95.0):
#   * CARGO_INCREMENTAL=0     — incremental objects embed run-specific state.
#   * SOURCE_DATE_EPOCH=0     — pins any honoured build timestamp.
#   * --remap-path-prefix     — without it, absolute paths leak into the binary.
#   * -C link-arg=/Brepro     — MSVC writes the wall-clock time into the COFF
#                               header and the debug directory. Measured: without
#                               it, two builds differ by exactly 20 bytes, all of
#                               them timestamps.
#
# Without the flags above, three consecutive builds of the same source produced
# three different hashes (see docs/qualification/REPRODUCIBILITY.md).
#
# Usage:  pwsh -File scripts/repro-check.ps1 [-Keep] [-Package ravum-lang-cli]

[CmdletBinding()]
param(
    [switch]$Keep,
    [string]$Package = 'ravum-lang-cli',
    [string]$Profile = 'release'
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$binaryName = if ($IsWindows -or $env:OS -eq 'Windows_NT') { 'ravum-lang.exe' } else { 'ravum-lang' }
$suffix = [guid]::NewGuid().ToString('N').Substring(0, 8)
$clone = Join-Path ([System.IO.Path]::GetTempPath()) "ravum-repro-$suffix"
$firstTarget = Join-Path $root "target\repro-$suffix-a"
$secondTarget = Join-Path $root "target\repro-$suffix-b"

# Only the flags that make the build path-independent; the linker flag is
# MSVC-specific and must not be passed to a GNU/LLVM linker.
$linkFlag = if ($IsWindows -or $env:OS -eq 'Windows_NT') { '-C link-arg=/Brepro' } else { '-C link-arg=-Wl,--build-id=sha1' }

function Invoke-Build {
    param([string]$Workdir, [string]$TargetDir, [string]$Remap)
    $env:CARGO_INCREMENTAL = '0'
    $env:SOURCE_DATE_EPOCH = '0'
    $env:RUSTFLAGS = "$Remap $linkFlag"
    $env:CARGO_TARGET_DIR = $TargetDir
    Push-Location $Workdir
    try {
        & cargo build --profile $Profile --locked -p $Package 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed in $Workdir"
        }
    }
    finally {
        Pop-Location
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    }
}

Write-Host "repro-check: cloning $root -> $clone"
$excluded = @('target', '.git', '.agents', '.ai-native', '.claude', '.opencode', 'node_modules', '.vscode-test')
$robocopyArgs = @($root, $clone, '/E', '/NFL', '/NDL', '/NJH', '/NJS', '/NP')
foreach ($name in $excluded) { $robocopyArgs += @('/XD', (Join-Path $root $name)) }
& robocopy @robocopyArgs | Out-Null
if ($LASTEXITCODE -ge 8) {
    throw "robocopy failed with exit code $LASTEXITCODE"
}

Write-Host "repro-check: build #1 (root A, target A)"
Invoke-Build -Workdir $root -TargetDir $firstTarget -Remap "--remap-path-prefix=$root=/rv --remap-path-prefix=$clone=/rv"

Write-Host "repro-check: build #2 (root B, target B)"
Invoke-Build -Workdir $clone -TargetDir $secondTarget -Remap "--remap-path-prefix=$root=/rv --remap-path-prefix=$clone=/rv"

$first = Join-Path $firstTarget "$Profile\$binaryName"
$second = Join-Path $secondTarget "$Profile\$binaryName"
$hashA = (Get-FileHash $first -Algorithm SHA256).Hash
$hashB = (Get-FileHash $second -Algorithm SHA256).Hash

Write-Host "repro-check: A = $hashA"
Write-Host "repro-check: B = $hashB"

$ok = $hashA -eq $hashB
if (-not $Keep) {
    Remove-Item -Recurse -Force $firstTarget, $secondTarget, $clone -ErrorAction SilentlyContinue
}

if ($ok) {
    Write-Host "repro-check: PASS — the two builds are bit-for-bit identical ($hashA)"
    exit 0
}
Write-Host "repro-check: FAIL — the builds differ"
exit 1
