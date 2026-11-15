# evidence.ps1 — one machine-readable snapshot of the quick controls.
#
# The same figures were recopied by hand into four documents, and each replay
# needed several manual corrections (audit 2026-11-14, N7/DF). This script runs
# the *quick* controls, writes `evidence/current.json` (SHA, date, command,
# exit code, parsed numbers, output tail) and prints a table. Documents cite
# these numbers; `python tools/report_consistency.py --evidence` fails when a
# current-state declaration disagrees with the file.
#
# Scope, stated honestly: the workspace test suite is NOT run here (it is not
# quick). The controls are fmt, conformance (`--verify-build`), the code
# registry, FeatureId traceability, MUST traceability, the Python tool tests and
# the existing report-consistency check.
#
# Usage: pwsh -File scripts/evidence.ps1 [-Output evidence/current.json]

param(
    [string]$Output = ''
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$root = Split-Path -Parent $PSScriptRoot
if (-not $Output) {
    $Output = Join-Path $root 'evidence\current.json'
}
$Output = [System.IO.Path]::GetFullPath($Output)

$controls = @()
$metrics = [ordered]@{}

function Add-Result {
    param(
        [string]$Name,
        [string]$Command,
        [string]$Text,
        [int]$Code,
        [double]$Milliseconds,
        [hashtable]$Numbers
    )
    $script:controls += [ordered]@{
        name        = $Name
        command     = $Command
        result      = $(if ($Code -eq 0) { 'PASS' } else { 'FAIL' })
        exit_code   = $Code
        wall_ms     = [math]::Round($Milliseconds, 3)
        numbers     = $Numbers
        output_tail = (($Text -split "`r?`n" | Where-Object { $_ -ne '' } | Select-Object -Last 6) -join "`n")
    }
    foreach ($key in $Numbers.Keys) {
        $script:metrics[$key] = $Numbers[$key]
    }
}

function Invoke-Control {
    param([string]$Name, [string]$Command, [scriptblock]$Body)
    Write-Host "evidence: $Name — $Command"
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $text = (& $Body 2>&1 | Out-String)
    $code = $LASTEXITCODE
    $watch.Stop()
    $numbers = [ordered]@{}
    return [pscustomobject]@{
        name   = $Name
        command = $Command
        text   = $text
        code   = $code
        ms     = $watch.Elapsed.TotalMilliseconds
        numbers = $numbers
    }
}

# --- fmt -------------------------------------------------------------------
$run = Invoke-Control -Name 'fmt' -Command 'cargo fmt --all -- --check' -Body { cargo fmt --all -- --check }
Add-Result -Name $run.name -Command $run.command -Text $run.text -Code $run.code -Milliseconds $run.ms -Numbers @{}

# --- conformance + build perimeter -----------------------------------------
$run = Invoke-Control -Name 'conformance' -Command 'cargo run -q -p ravum-conformance -- run --verify-build' -Body { cargo run -q -p ravum-conformance -- run --verify-build }
$numbers = @{}
if ($run.text -match 'summary: total=(\d+) pass=(\d+) fail=(\d+) skip=(\d+) not_run=(\d+)') {
    $numbers['conformance.cases'] = [int]$matches[1]
    $numbers['conformance.pass'] = [int]$matches[2]
    $numbers['conformance.fail'] = [int]$matches[3]
    $numbers['conformance.skip'] = [int]$matches[4]
    $numbers['conformance.not_run'] = [int]$matches[5]
}
if ($run.text -match 'build: perimeter=(\d+) covered=(\d+) verified=(\d+) implicit=(\d+) failed=(\d+)') {
    $numbers['build.perimeter'] = [int]$matches[1]
    $numbers['build.covered'] = [int]$matches[2]
    $numbers['build.verified'] = [int]$matches[3]
    $numbers['build.implicit'] = [int]$matches[4]
    $numbers['build.failed'] = [int]$matches[5]
}
Add-Result -Name $run.name -Command $run.command -Text $run.text -Code $run.code -Milliseconds $run.ms -Numbers $numbers

# --- code registry ---------------------------------------------------------
$run = Invoke-Control -Name 'registry' -Command 'python tools/registry_audit.py' -Body { python tools/registry_audit.py }
$numbers = @{}
if ($run.text -match 'registry codes\s*:\s*(\d+)') { $numbers['registry.declared'] = [int]$matches[1] }
if ($run.text -match 'codes emitted\s*:\s*(\d+)') { $numbers['registry.emitted'] = [int]$matches[1] }
if ($run.text -match 'unregistered\s*:\s*(\d+)') { $numbers['registry.unregistered'] = [int]$matches[1] }
if ($run.text -match 'never emitted\s*:\s*(\d+)') { $numbers['registry.never_emitted'] = [int]$matches[1] }
Add-Result -Name $run.name -Command $run.command -Text $run.text -Code $run.code -Milliseconds $run.ms -Numbers $numbers

# --- FeatureId traceability ------------------------------------------------
$run = Invoke-Control -Name 'features' -Command 'python tools/features_coverage.py --strict' -Body { python tools/features_coverage.py --strict }
$numbers = @{}
if ($run.text -match 'strict traceability:\s*(\d+)/(\d+)') {
    $numbers['features.covered'] = [int]$matches[1]
    $numbers['features.total'] = [int]$matches[2]
}
Add-Result -Name $run.name -Command $run.command -Text $run.text -Code $run.code -Milliseconds $run.ms -Numbers $numbers

# --- MUST traceability -----------------------------------------------------
$run = Invoke-Control -Name 'must' -Command 'python tools/must_audit.py' -Body { python tools/must_audit.py }
$numbers = @{}
if ($run.text -match 'MUST statements\s*:\s*(\d+)') { $numbers['must.total'] = [int]$matches[1] }
if ($run.text -match 'UNMAPPED[^:]*:\s*(\d+)') {
    $unmapped = [int]$matches[1]
    $numbers['must.unmapped'] = $unmapped
    if ($numbers.ContainsKey('must.total')) { $numbers['must.mapped'] = [int]$numbers['must.total'] - $unmapped }
}
Add-Result -Name $run.name -Command $run.command -Text $run.text -Code $run.code -Milliseconds $run.ms -Numbers $numbers

# --- Python tool tests -----------------------------------------------------
$run = Invoke-Control -Name 'python_tools' -Command 'python -m unittest discover -s tools/tests' -Body { python -m unittest discover -s tools/tests }
$numbers = @{}
if ($run.text -match 'Ran (\d+) tests') { $numbers['python_tests.count'] = [int]$matches[1] }
Add-Result -Name $run.name -Command $run.command -Text $run.text -Code $run.code -Milliseconds $run.ms -Numbers $numbers

# --- existing report consistency -------------------------------------------
$run = Invoke-Control -Name 'report_consistency' -Command 'python tools/report_consistency.py' -Body { python tools/report_consistency.py }
Add-Result -Name $run.name -Command $run.command -Text $run.text -Code $run.code -Milliseconds $run.ms -Numbers @{}

# --- snapshot --------------------------------------------------------------
$sha = (& git -C $root rev-parse HEAD | Out-String).Trim()
$branch = (& git -C $root branch --show-current | Out-String).Trim()
$status = (& git -C $root status --porcelain=v1 | Out-String).Trim()
$snapshot = [ordered]@{
    schema_version = 1
    generated_at   = (Get-Date).ToUniversalTime().ToString('o')
    git_sha        = $sha
    git_sha_short  = $sha.Substring(0, [math]::Min(12, $sha.Length))
    git_branch     = $branch
    git_dirty      = ($status.Length -gt 0)
    os             = (Get-CimInstance Win32_OperatingSystem).Caption
    os_version     = (Get-CimInstance Win32_OperatingSystem).Version
    cpu            = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()
    ram_bytes      = [int64](Get-CimInstance Win32_OperatingSystem).TotalVisibleMemorySize * 1024
    rustc          = (& rustc --version | Out-String).Trim()
    metrics        = $metrics
    controls       = $controls
}

$directory = Split-Path -Parent $Output
if ($directory -and -not (Test-Path -LiteralPath $directory)) {
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
}
$snapshot | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $Output -Encoding utf8

Write-Host ''
Write-Host ('{0,-20} {1,-6} {2,10}' -f 'control', 'result', 'wall_ms')
foreach ($control in $controls) {
    Write-Host ('{0,-20} {1,-6} {2,10:N0}' -f $control.name, $control.result, $control.wall_ms)
}
Write-Host ''
Write-Host ('metrics: ' + (($metrics.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ', '))
Write-Host "evidence: wrote $Output"
$failed = @($controls | Where-Object { $_.result -ne 'PASS' })
if ($failed.Count -gt 0) {
    Write-Host "evidence: $($failed.Count) control(s) failed"
    exit 1
}
exit 0
