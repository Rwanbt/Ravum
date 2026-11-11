<#
.SYNOPSIS
    Exécute la suite de tests avec une concurrence limitée (machine 16 Go).

.DESCRIPTION
    - CARGO_BUILD_JOBS=2 : évite de saturer la RAM disponible.
    - --test-threads=1 : les tests M1 compilent de vraies bibliothèques natives ;
      les paralléliser fait grimper la mémoire et fausse les mesures.
    La sortie brute est conservée pour le rapport de preuves.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/run-tests.ps1
#>
[CmdletBinding()]
param(
    [string]$Root = '',
    [string]$OutFile = ''
)

$ErrorActionPreference = 'Stop'
if (-not $Root) { $Root = Split-Path -Parent $PSScriptRoot }
if (-not $Root) { $Root = (Get-Location).Path }
Push-Location $Root
try {
    $env:CARGO_BUILD_JOBS = '2'
    if (-not $OutFile) {
        $dir = Join-Path $Root 'evidence\local'
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        $OutFile = Join-Path $dir "cargo-test-$stamp.txt"
    }
    Write-Host "Sortie complète: $OutFile"

    # Windows PowerShell 5.1 converts native stderr into a *terminating*
    # NativeCommandError when $ErrorActionPreference is 'Stop' (set above). A
    # single rustc warning on stderr aborted the run before the output file was
    # created (observed 2026-11-09). Keep the preference non-terminating for the
    # duration of the child process only, then restore it.
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & cargo test --workspace --offline -- --test-threads=1 2>&1 | Tee-Object -FilePath $OutFile
        $code = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }

    Write-Host ''
    Write-Host ('Code de sortie cargo test: ' + $code)
    exit $code
}
finally {
    Pop-Location
}
