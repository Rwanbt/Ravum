# P0-A probe: run `check` then `build` on every accepted-mode conformance case.
#
# Not a gate yet: this script measures the divergence families before the
# conformance harness gains a `build-pass` mode. It never modifies the corpus:
# every case is copied into a scratch directory first.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path,
    [string]$Toolchain = "",
    [string]$Out = "",
    [switch]$IncludeRunPass
)

$ErrorActionPreference = "Stop"

if (-not $Toolchain) { $Toolchain = Join-Path $RepoRoot "target\debug\ravum-lang.exe" }
if (-not (Test-Path $Toolchain)) { throw "toolchain not found: $Toolchain" }
if (-not $Out) { $Out = Join-Path $RepoRoot "target\p0a-sweep.json" }

$casesRoot = Join-Path $RepoRoot "conformance\cases"
$scratch = Join-Path $RepoRoot "target\p0a-sweep"
if (Test-Path $scratch) { Remove-Item -Recurse -Force $scratch }
New-Item -ItemType Directory -Path $scratch | Out-Null

$modes = @("compile-pass")
if ($IncludeRunPass) { $modes += "run-pass" }

$results = New-Object System.Collections.ArrayList
$index = 0

Get-ChildItem -Recurse -File -Filter case.json -Path $casesRoot | ForEach-Object {
    $json = Get-Content $_.FullName -Raw | ConvertFrom-Json
    if ($modes -notcontains $json.mode) { return }
    $index++
    $caseDir = $_.Directory
    $rel = $caseDir.FullName.Substring($casesRoot.Length + 1).Replace('\', '/')
    $dest = Join-Path $scratch ("{0:d3}-{1}" -f $index, ($rel -replace '[^A-Za-z0-9]', '-'))
    Copy-Item -Recurse -Force $caseDir.FullName $dest

    $project = if ($json.project) { [string]$json.project } else { "." }
    $target = Join-Path $dest $project

    $checkOut = & $Toolchain check $target 2>&1 | Out-String
    $checkExit = $LASTEXITCODE
    $buildOut = ""
    $buildExit = -1
    if ($checkExit -eq 0) {
        $buildOut = & $Toolchain build $target 2>&1 | Out-String
        $buildExit = $LASTEXITCODE
    }

    $divergence = ($checkExit -eq 0 -and $buildExit -ne 0)
    [void]$results.Add([pscustomobject]@{
        case       = $rel
        mode       = $json.mode
        project    = $project
        check_exit = $checkExit
        build_exit = $buildExit
        divergence = $divergence
        check_head = (($checkOut -split "`n") | Where-Object { $_ -match 'error\[' } | Select-Object -First 1)
        build_err  = (($buildOut -split "`n") | Where-Object { $_ -match '^(error|warning: unused)' } | Select-Object -First 3) -join " | "
    })
    Write-Host ("{0,-50} check={1} build={2}{3}" -f $rel, $checkExit, $buildExit, $(if ($divergence) { "  <== DIVERGENCE" } else { "" }))
}

$summary = [pscustomobject]@{
    toolchain  = $Toolchain
    total      = $results.Count
    check_pass = ($results | Where-Object { $_.check_exit -eq 0 }).Count
    build_ok   = ($results | Where-Object { $_.check_exit -eq 0 -and $_.build_exit -eq 0 }).Count
    divergences = ($results | Where-Object { $_.divergence }).Count
    results    = $results
}
$summary | ConvertTo-Json -Depth 6 | Set-Content -Path $Out -Encoding UTF8

Write-Host ""
Write-Host ("SUMMARY: {0} cases; check-pass {1}; build-pass {2}; DIVERGENCES {3}" -f `
    $summary.total, $summary.check_pass, $summary.build_ok, $summary.divergences)
Write-Host ("report: {0}" -f $Out)
if ($summary.divergences -gt 0) { exit 1 }
