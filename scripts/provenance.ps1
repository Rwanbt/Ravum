#!/usr/bin/env pwsh
# provenance — emit (and verify) a build provenance manifest for a release artifact.
#
# The manifest binds, in one JSON document, the things that make a binary
# attributable:
#   * the source commit and whether the tree was dirty when the build happened,
#   * the exact toolchain (rustc/cargo versions, target triple),
#   * the SHA-256 of the binary and of the SBOM,
#   * the reproducible-build settings used (so the binary can be rebuilt).
#
# It is NOT signed. Signing the manifest is what `scripts/attest-release.ps1`
# does once a project key exists (decision D9); an unsigned manifest is a claim,
# a signed one is an attestation.
#
# Usage:
#   pwsh -File scripts/provenance.ps1 -Binary target\release\ravum-lang.exe
#   pwsh -File scripts/provenance.ps1 -Verify -Manifest target\provenance\ravum-provenance.json -Binary target\release\ravum-lang.exe

[CmdletBinding()]
param(
    [string]$Binary = 'target\release\ravum-lang.exe',
    [string]$Sbom = 'target\sbom\ravum.spdx.json',
    [string]$Out = 'target\provenance\ravum-provenance.json',
    [switch]$Verify
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Push-Location $root
try {
    function Get-Sha256([string]$Path) {
        if (-not (Test-Path $Path)) { return 'absent' }
        (Get-FileHash $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    }

    $binaryHash = Get-Sha256 $Binary

    if ($Verify) {
        if (-not (Test-Path $Out)) { throw "manifest not found: $Out" }
        $manifest = Get-Content $Out -Raw | ConvertFrom-Json
        $problems = @()
        if ($manifest.artifacts.binary_sha256 -ne $binaryHash) {
            $problems += "binary digest mismatch: manifest=$($manifest.artifacts.binary_sha256) actual=$binaryHash"
        }
        $sbomHash = Get-Sha256 $Sbom
        if ($manifest.artifacts.sbom_sha256 -ne $sbomHash) {
            $problems += "SBOM digest mismatch: manifest=$($manifest.artifacts.sbom_sha256) actual=$sbomHash"
        }
        $head = (git rev-parse HEAD).Trim()
        if ($manifest.source.commit -ne $head) {
            $problems += "commit mismatch: manifest=$($manifest.source.commit) actual=$head"
        }
        if ($problems.Count -gt 0) {
            $problems | ForEach-Object { Write-Host "MISMATCH : $_" }
            Write-Host 'provenance: FAIL — the manifest does not describe this tree'
            exit 1
        }
        Write-Host 'provenance: PASS — the manifest describes this binary, SBOM and commit'
        exit 0
    }

    if ($binaryHash -eq 'absent') { throw "binary not found: $Binary (run cargo build --release first)" }

    $rustc = (& rustc -vV) -split "`n"
    $cargoVersion = (& cargo --version)
    $dirty = -not [string]::IsNullOrWhiteSpace((git status --porcelain))

    $manifest = [ordered]@{
        schema_version = 1
        tool           = 'ravum-provenance'
        source         = [ordered]@{
            commit        = (git rev-parse HEAD).Trim()
            branch        = (git rev-parse --abbrev-ref HEAD).Trim()
            dirty         = [bool]$dirty
            dirty_files   = [string[]]@(if ($dirty) { (git status --porcelain) -split "`n" | Where-Object { $_ } | ForEach-Object { $_.Substring(3) } })
        }
        toolchain      = [ordered]@{
            rustc         = ($rustc | Where-Object { $_ -like 'release:*' }) -replace '^release:\s*', ''
            host          = ($rustc | Where-Object { $_ -like 'host:*' }) -replace '^host:\s*', ''
            cargo         = $cargoVersion
        }
        artifacts      = [ordered]@{
            binary        = $Binary
            binary_sha256 = $binaryHash
            sbom          = $Sbom
            sbom_sha256   = Get-Sha256 $Sbom
        }
        repro_settings = [ordered]@{
            cargo_incremental = 0
            source_date_epoch = 0
            rustflags         = '--remap-path-prefix=<root>=/rv -C link-arg=/Brepro'
            command           = 'pwsh -File scripts/repro-check.ps1'
        }
        signed         = $false
        signed_by      = $null
        signing_note   = 'unsigned: no project key exists (decision D9); sign with scripts/attest-release.ps1 once one does'
    }

    $directory = Split-Path -Parent $Out
    if ($directory) { New-Item -ItemType Directory -Force -Path $directory | Out-Null }
    $json = $manifest | ConvertTo-Json -Depth 6
    Set-Content -Path $Out -Value $json -Encoding utf8
    Write-Host "provenance: wrote $Out"
    Write-Host "provenance: binary=$($manifest.artifacts.binary_sha256.Substring(0,16))… sbom=$($manifest.artifacts.sbom_sha256.Substring(0,16))… commit=$($manifest.source.commit.Substring(0,7)) dirty=$($manifest.source.dirty)"
}
finally {
    Pop-Location
}
