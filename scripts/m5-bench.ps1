# M5 — comparaison A (Rust) vs B (Ravum -> Rust) vs C (IR native)
#
# Mesure honnête : mêmes algorithmes, même machine, mêmes sorties comparées.
# A = rustc direct sur benchmarks/m5/rust/*.rs
# B = ravum-lang build sur examples/* (frontend + checker Ravum + codegen + rustc)
# C = NON IMPLÉMENTÉ (aucun backend IR natif) -> statut NOT_IMPLEMENTED
param(
    [int]$CompileReps = 7,
    [int]$RunReps = 15,
    [string]$Out = "evidence/m5-results.json"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $root "target\debug\ravum-lang.exe"
$rustc = if ($env:RAVUM_RUSTC) { $env:RAVUM_RUSTC } else { "rustc" }
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ravum-m5-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

function Percentile([double[]]$values, [double]$p) {
    if ($values.Count -eq 0) { return $null }
    $sorted = $values | Sort-Object
    $idx = [math]::Ceiling(($p / 100.0) * $sorted.Count) - 1
    if ($idx -lt 0) { $idx = 0 }
    return [math]::Round($sorted[$idx], 2)
}

function Measure-CommandMs([scriptblock]$Block) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & $Block | Out-Null
    $sw.Stop()
    return [math]::Round($sw.Elapsed.TotalMilliseconds, 2)
}

$programs = @(
    @{ Name = "fib"; Rust = "benchmarks/m5/rust/fib.rs"; Rv = "examples/fib"; Args = @(); Expect = "0`n1`n1`n2`n3`n5`n8`n13`n21`n34`n55" },
    @{ Name = "calculator"; Rust = "benchmarks/m5/rust/calculator.rs"; Rv = "examples/calculator"; Args = @("5", "+", "3"); Expect = "8" },
    @{ Name = "stats"; Rust = "benchmarks/m5/rust/stats.rs"; Rv = "examples/stats"; Args = @((Join-Path $root "examples\stats\sample.txt")); Expect = "2.5" },
    @{ Name = "dsp"; Rust = "benchmarks/m5/rust/dsp.rs"; Rv = "examples/dsp-gain-iir"; Args = @(); Expect = $null }
)

$results = @()
foreach ($prog in $programs) {
    $entry = [ordered]@{ program = $prog.Name; a = @{}; b = @{}; c = @{ status = "NOT_IMPLEMENTED"; reason = "no native IR backend in this iteration" }; output_parity = "NOT_RUN" }
    $aExe = Join-Path $tmp ("a-" + $prog.Name + ".exe")
    $binDir = Join-Path $root ($prog.Rv + "\.ravum\bin")
    # The CLI names the binary after the package (hyphens become underscores).
    $bExe = Join-Path $binDir (((Split-Path $prog.Rv -Leaf) -replace '-', '_') + ".exe")

    # A: compilation
    $aCompile = @()
    for ($i = 0; $i -lt $CompileReps; $i++) {
        if (Test-Path $aExe) { Remove-Item $aExe -Force }
        $aCompile += Measure-CommandMs { & $rustc -O -C overflow-checks=on -o $aExe (Join-Path $root $prog.Rust) 2>&1 | Out-Null }
    }
    $entry.a.compile_ms = @{ p50 = (Percentile $aCompile 50); p95 = (Percentile $aCompile 95); all = $aCompile }
    $entry.a.binary_bytes = (Get-Item $aExe).Length

    # A: exécution
    $aRun = @()
    for ($i = 0; $i -lt $RunReps; $i++) { $aRun += Measure-CommandMs { & $aExe @($prog.Args) 2>&1 | Out-Null } }
    $entry.a.run_ms = @{ p50 = (Percentile $aRun 50); p95 = (Percentile $aRun 95); all = $aRun }
    $aOut = ((& $aExe @($prog.Args) 2>&1 | Out-String) -replace "`r`n", "`n").Trim()

    # B: compilation via la vraie chaîne Ravum
    Push-Location $root
    $bCompile = @()
    $bOk = $true
    for ($i = 0; $i -lt $CompileReps; $i++) {
        if (Test-Path $bExe) { Remove-Item $bExe -Force }
        $bCompile += Measure-CommandMs { & $bin build (Join-Path $root $prog.Rv) 2>&1 | Out-Null }
        if (-not (Test-Path $bExe)) { $bOk = $false; break }
    }
    Pop-Location
    if ($bOk) {
        $entry.b.compile_ms = @{ p50 = (Percentile $bCompile 50); p95 = (Percentile $bCompile 95); all = $bCompile }
        $entry.b.binary_bytes = (Get-Item $bExe).Length
        $bRun = @()
        for ($i = 0; $i -lt $RunReps; $i++) { $bRun += Measure-CommandMs { & $bExe @($prog.Args) 2>&1 | Out-Null } }
        $entry.b.run_ms = @{ p50 = (Percentile $bRun 50); p95 = (Percentile $bRun 95); all = $bRun }
        $bOut = ((& $bExe @($prog.Args) 2>&1 | Out-String) -replace "`r`n", "`n").Trim()
        if ($prog.Expect) { $normalizedExpect = ($prog.Expect -replace "`r`n", "`n").Trim()
            $entry.output_parity = if (($aOut -eq $bOut) -and ($bOut -eq $normalizedExpect)) { "MATCH" } else { "MISMATCH" }
        } else {
            $entry.output_parity = if ($aOut -eq $bOut) { "MATCH" } else { "MISMATCH" }
        }
    } else {
        $entry.b.status = "BUILD_FAILED"
        $entry.b.compile_ms = @{ p50 = (Percentile $bCompile 50); p95 = (Percentile $bCompile 95); all = $bCompile }
    }
    $results += $entry
}

$report = [ordered]@{
    schema_version = 1
    date = (Get-Date).ToString("o")
    machine = [ordered]@{ os = "Windows 11 x86-64"; cpu = "i7-13620H"; ram_gb = 15.7; rustc = (& $rustc --version); rust_opt = "-O -C overflow-checks=on"; compile_reps = $CompileReps; run_reps = $RunReps }
    toolchain_b = (& $bin --version)
    decision = "B provisional (fixed by design doc; human ratification pending)"
    programs = $results
}
$outPath = Join-Path $root $Out
New-Item -ItemType Directory -Force -Path (Split-Path $outPath -Parent) | Out-Null
$report | ConvertTo-Json -Depth 8 | Set-Content -Path $outPath -Encoding utf8
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue

$results | ForEach-Object {
    $a = if ($_.a.compile_ms) { "A compile p50={0}ms p95={1}ms run p50={2}ms" -f $_.a.compile_ms.p50, $_.a.compile_ms.p95, $_.a.run_ms.p50 } else { "A n/a" }
    $b = if ($_.b.compile_ms) { "B compile p50={0}ms p95={1}ms run p50={2}ms parity={3}" -f $_.b.compile_ms.p50, $_.b.compile_ms.p95, $(if ($_.b.run_ms) { $_.b.run_ms.p50 } else { "n/a" }), $_.output_parity } else { "B " + $_.b.status }
    Write-Host ("[{0}] {1} | {2}" -f $_.program, $a, $b)
}
Write-Host ("rapport: {0}" -f $outPath)

