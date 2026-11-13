# lang-timings.ps1 — per-phase wall-clock timings of the Ravum language loop.
#
# For every example in examples/, run `ravum-lang check --json --timings` and
# `ravum-lang build --timings`:
#
#   * "cold": the build cache (<pkg>/.ravum) is removed before every run, so the
#     run pays a full rustc cold build;
#   * "edit": one line is appended to the crate root before every run, so the
#     source digest changes and no previous artifact can be reused (this is the
#     loop a user actually runs).
#
# `check` does not use the build cache; its cold/edit distinction therefore
# measures process start plus a fresh analysis versus the same loop after an
# edit. `build` is the expensive one because rustc dominates.
#
# Output: JSON with p50/p95 per phase for each example and mode, plus the SHA,
# toolchain and machine description. Percentiles use the nearest-rank method
# (p in {50, 95}); all values are milliseconds, rounded to three decimals.
#
# Usage:
#   pwsh -File scripts/lang-timings.ps1 [-Runs 20] [-Output <path>] [-Examples name,name]
#   pwsh -File scripts/lang-timings.ps1 -CheckOnly     # skip the build campaign
#
# The script never modifies examples/: it copies each package to
# target/lang-timings/work/<name>/ and edits the copy.

param(
    [int]$Runs = 20,
    [string]$Output = '',
    [string[]]$Examples = @(),
    [switch]$CheckOnly
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if ($Runs -lt 1 -or $Runs -gt 200) { throw "Runs must be in 1..200, got $Runs" }

$root = Split-Path -Parent $PSScriptRoot
$binary = Join-Path $root 'target\debug\ravum-lang.exe'
if (-not (Test-Path -LiteralPath $binary)) {
    throw "ravum-lang not found at ${binary}: run 'cargo build -p ravum-lang-cli --bin ravum-lang' first"
}
if (-not $Output) {
    $Output = Join-Path $root 'target\lang-timings\lang-timings.json'
}
$work = Join-Path $root 'target\lang-timings\work'
if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
New-Item -ItemType Directory -Force -Path $work | Out-Null

function Get-Percentile {
    param([double[]]$Values, [int]$Percent)
    if ($Values.Count -eq 0) { return 0.0 }
    $sorted = @($Values | Sort-Object)
    $rank = [int][math]::Ceiling($Percent / 100.0 * $sorted.Count) - 1
    if ($rank -lt 0) { $rank = 0 }
    if ($rank -ge $sorted.Count) { $rank = $sorted.Count - 1 }
    return [math]::Round([double]$sorted[$rank], 3)
}

function Convert-ToPhaseMap {
    param([hashtable]$Raw)
    $map = [ordered]@{}
    foreach ($key in @('load', 'parse', 'resolve', 'typeck', 'effects', 'own', 'codegen', 'rustc', 'total')) {
        $value = 0.0
        if ($Raw.ContainsKey($key)) { $value = [double]$Raw[$key] }
        $map[$key] = $value
    }
    return $map
}

function Measure-Phases {
    param([object[]]$Samples)
    $summary = [ordered]@{}
    foreach ($key in @('load', 'parse', 'resolve', 'typeck', 'effects', 'own', 'codegen', 'rustc', 'total')) {
        $values = @($Samples | ForEach-Object { [double]$_[$key] })
        $summary[$key] = [ordered]@{
            p50 = Get-Percentile -Values $values -Percent 50
            p95 = Get-Percentile -Values $values -Percent 95
        }
    }
    return $summary
}

function Invoke-RunChecked {
    param([string[]]$Arguments, [string]$Label)
    $stdout = [System.IO.Path]::GetTempFileName()
    $stderr = [System.IO.Path]::GetTempFileName()
    try {
        $previous = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $output = & $binary @Arguments 2>&1
        $code = $LASTEXITCODE
        $ErrorActionPreference = $previous
        if ($code -ne 0) {
            throw "${Label} failed with exit code ${code}:`n$($output | Out-String)"
        }
        return ($output | Out-String)
    }
    finally {
        Remove-Item -LiteralPath $stdout, $stderr -ErrorAction SilentlyContinue
        $ErrorActionPreference = 'Stop'
    }
}

function ConvertFrom-TimingLine {
    param([string]$Text)
    $line = ($Text -split "`n" | Where-Object { $_ -match '^timings:' } | Select-Object -First 1)
    if (-not $line) { throw "no 'timings:' line in the command output" }
    $raw = @{}
    foreach ($match in [regex]::Matches($line, '([a-z]+)=([0-9.]+)ms')) {
        $raw[$match.Groups[1].Value] = [double]::Parse($match.Groups[2].Value, [System.Globalization.CultureInfo]::InvariantCulture)
    }
    return Convert-ToPhaseMap -Raw $raw
}

function Measure-Check {
    param([string]$Directory)
    $text = Invoke-RunChecked -Arguments @('check', $Directory, '--json', '--timings') -Label 'check'
    $json = $text.Trim() | ConvertFrom-Json
    $timings = $json.timings
    if ($null -eq $timings) { throw "check JSON has no 'timings' object" }
    $raw = @{
        load    = [double]$timings.load_ms
        parse   = [double]$timings.parse_ms
        resolve = [double]$timings.resolve_ms
        typeck  = [double]$timings.typeck_ms
        effects = [double]$timings.effects_ms
        own     = [double]$timings.own_ms
        total   = [double]$timings.total_ms
    }
    return Convert-ToPhaseMap -Raw $raw
}

function Measure-Build {
    param([string]$Directory)
    $text = Invoke-RunChecked -Arguments @('build', $Directory, '--timings') -Label 'build'
    return ConvertFrom-TimingLine -Text $text
}

function Remove-BuildCache {
    param([string]$Directory)
    $cache = Join-Path $Directory '.ravum'
    if (Test-Path -LiteralPath $cache) {
        Remove-Item -LiteralPath $cache -Recurse -Force
    }
}

function Add-OneLineEdit {
    param([string]$Directory, [int]$Iteration)
    $root = if (Test-Path -LiteralPath (Join-Path $Directory 'src\main.rv')) {
        Join-Path $Directory 'src\main.rv'
    } elseif (Test-Path -LiteralPath (Join-Path $Directory 'src\lib.rv')) {
        Join-Path $Directory 'src\lib.rv'
    } else {
        throw "no src/main.rv or src/lib.rv in ${Directory}"
    }
    Add-Content -LiteralPath $root -Value "// lang-timings edit $Iteration" -Encoding utf8
}

$examplesRoot = Join-Path $root 'examples'
$names = if ($Examples.Count -gt 0) {
    # Accept both `-Examples hello,fib` (one string from -File) and a real array.
    @($Examples | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
} else {
    Get-ChildItem -LiteralPath $examplesRoot -Directory | Select-Object -ExpandProperty Name
}
if ($names.Count -eq 0) { throw "no example found under $examplesRoot" }

$result = [ordered]@{
    schema_version     = 1
    generated_at       = (Get-Date).ToString('o')
    git_sha            = (& git -C $root rev-parse HEAD | Out-String).Trim()
    git_branch         = (& git -C $root branch --show-current | Out-String).Trim()
    os                 = (Get-CimInstance Win32_OperatingSystem).Caption
    os_version         = (Get-CimInstance Win32_OperatingSystem).Version
    cpu                = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()
    ram_bytes          = [int64](Get-CimInstance Win32_OperatingSystem).TotalVisibleMemorySize * 1024
    rustc              = (& rustc --version | Out-String).Trim()
    runs_per_phase     = $Runs
    check_only         = [bool]$CheckOnly
    examples           = @()
}

foreach ($name in $names) {
    $source = Join-Path $examplesRoot $name
    if (-not (Test-Path -LiteralPath $source)) { throw "example not found: $name" }
    $copy = Join-Path $work $name
    Copy-Item -LiteralPath $source -Destination $copy -Recurse -Force
    # Some examples are workspace-shaped (package-demo has app/ and util/):
    # measure the binary package, keep the dependency inside the copy.
    $measure = $copy
    if (-not (Test-Path -LiteralPath (Join-Path $copy 'src'))) {
        $nested = Join-Path $copy 'app'
        if (Test-Path -LiteralPath (Join-Path $nested 'src')) {
            $measure = $nested
        } else {
            Write-Warning "lang-timings: skipping $name (no src/ or app/src/)"
            continue
        }
    }
    Remove-BuildCache -Directory $measure

    Write-Host "lang-timings: $name — check cold/edit ($Runs runs each)"
    $checkCold = @()
    for ($i = 0; $i -lt $Runs; $i++) { $checkCold += Measure-Check -Directory $measure }
    $checkEdit = @()
    for ($i = 0; $i -lt $Runs; $i++) {
        Add-OneLineEdit -Directory $measure -Iteration ($i + 1)
        $checkEdit += Measure-Check -Directory $measure
    }

    $entry = [ordered]@{
        name      = $name
        check     = [ordered]@{
            cold = Measure-Phases -Samples $checkCold
            edit = Measure-Phases -Samples $checkEdit
        }
    }

    if (-not $CheckOnly) {
        Write-Host "lang-timings: $name — build cold/edit ($Runs runs each)"
        $buildCold = @()
        for ($i = 0; $i -lt $Runs; $i++) {
            Remove-BuildCache -Directory $measure
            $buildCold += Measure-Build -Directory $measure
        }
        $buildEdit = @()
        for ($i = 0; $i -lt $Runs; $i++) {
            Add-OneLineEdit -Directory $measure -Iteration (1000 + $i)
            $buildEdit += Measure-Build -Directory $measure
        }
        $entry['build'] = [ordered]@{
            cold = Measure-Phases -Samples $buildCold
            edit = Measure-Phases -Samples $buildEdit
        }
    }

    $result.examples += $entry
    Write-Host "lang-timings: $name — done"
}

$directory = Split-Path -Parent $Output
if ($directory -and -not (Test-Path -LiteralPath $directory)) {
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
}
$result | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $Output -Encoding utf8
Write-Host "lang-timings: wrote $Output"

# Human-readable summary: check total, build total and rustc p50/p95.
Write-Host ''
Write-Host ('{0,-18} {1,16} {2,16} {3,16} {4,16}' -f 'example', 'check p50', 'check p95', 'build p50', 'build p95')
foreach ($entry in $result.examples) {
    $checkCold = $entry.check.cold.total
    $buildCold = $null
    if ($entry.Contains('build')) { $buildCold = $entry.build.cold.total }
    $build50 = if ($buildCold) { '{0:N1}' -f $buildCold.p50 } else { 'n/a' }
    $build95 = if ($buildCold) { '{0:N1}' -f $buildCold.p95 } else { 'n/a' }
    Write-Host ('{0,-18} {1,16} {2,16} {3,16} {4,16}' -f $entry.name, ('{0:N1}' -f $checkCold.p50), ('{0:N1}' -f $checkCold.p95), $build50, $build95)
}
