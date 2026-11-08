# Licence audit (MVP-0b / L5.3)
#
# Two independent questions, because the issue's acceptance criteria mix them:
#   1. Is a licence file present that would be an *implicit* licence decision?
#      The project deliberately has none: the choice belongs to the maintainer (#34).
#   2. Does the dependency tree contain a licence that the project could not ship
#      under most plausible choices (strong copyleft, network copyleft, non-commercial),
#      or a package with no licence metadata at all?
#
# Exit codes: 0 = pass, 1 = implicit licence decision found, 2 = incompatible or
# unknown dependency licence. Evidence is printed as JSON with -Json.
param(
    [switch]$Json
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

$licenceFiles = @('LICENSE', 'LICENSE.md', 'LICENSE.txt', 'COPYING', 'COPYING.md', 'UNLICENSE')
$found = @()
foreach ($name in $licenceFiles) {
    $path = Join-Path $root $name
    if (Test-Path -LiteralPath $path) { $found += $name }
}

# Strong/network copyleft, non-commercial, or "all rights reserved" markers. A package
# whose only option is one of these cannot be redistributed by the future Ravum products
# without a decision; the audit must surface it instead of silently accepting it.
$deny = '(?i)(^|[^A-Za-z])AGPL|(^|[^A-Za-z])GPL-?[23]|(^|[^A-Za-z])GPL($|[^A-Za-z-])|SSPL|BUSL|Business Source|CC-BY-NC|Commons Clause|proprietary|All rights reserved'
# LGPL is a weaker copyleft: flag it separately (relinking obligations, not a hard
# redistribution ban) but do not fail on it here.
$warn = '(?i)LGPL'

$metadataJson = & cargo metadata --format-version 1 --manifest-path (Join-Path $root 'Cargo.toml') 2>$null
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($metadataJson)) {
    Write-Error 'cargo metadata failed; cannot audit dependency licences'
    exit 2
}
$metadata = $metadataJson | ConvertFrom-Json

$denied = @()
$unlicensed = @()
$warnings = @()
$inventory = @()
foreach ($pkg in $metadata.packages) {
    $isWorkspace = $pkg.name -like 'ravum-*' -and $pkg.source -eq $null
    if ($isWorkspace) { continue }
    $licence = if ($pkg.license) { [string]$pkg.license } else { '' }
    $inventory += [pscustomobject]@{
        name    = $pkg.name
        version = $pkg.version
        license = $licence
        source  = if ($pkg.repository) { [string]$pkg.repository } else { '' }
    }
    if ([string]::IsNullOrWhiteSpace($licence) -and -not $pkg.license_file) {
        $unlicensed += "$($pkg.name) $($pkg.version)"
        continue
    }
    if ($licence -match $deny) { $denied += "$($pkg.name) $($pkg.version) [$licence]" }
    elseif ($licence -match $warn) { $warnings += "$($pkg.name) $($pkg.version) [$licence]" }
}

$result = [pscustomobject]@{
    kind              = 'ravum-licence-audit'
    schema_version    = 1
    repository        = $root
    implicit_licence  = $found
    dependency_count  = $inventory.Count
    denied            = $denied
    unlicensed        = $unlicensed
    weaker_copyleft   = $warnings
    dependencies      = $inventory
    verdict           = if ($found.Count -gt 0) { 'implicit-decision' } elseif ($denied.Count -gt 0 -or $unlicensed.Count -gt 0) { 'blocked' } else { 'ok-pending-human-decision' }
}

$exitCode = 0
if ($found.Count -gt 0) { $exitCode = 1 }
elseif ($denied.Count -gt 0 -or $unlicensed.Count -gt 0) { $exitCode = 2 }

if ($Json) {
    $result | ConvertTo-Json -Depth 6
} else {
    Write-Host "Licence audit for $root"
    Write-Host "  implicit licence file(s): $(if ($found.Count -eq 0) { 'none' } else { $found -join ', ' })"
    Write-Host "  third-party dependencies: $($inventory.Count)"
    Write-Host "  incompatible licences:    $(if ($denied.Count -eq 0) { 'none' } else { $denied -join '; ' })"
    Write-Host "  unknown licences:         $(if ($unlicensed.Count -eq 0) { 'none' } else { $unlicensed -join '; ' })"
    Write-Host "  weaker copyleft (warn):   $(if ($warnings.Count -eq 0) { 'none' } else { $warnings -join '; ' })"
    Write-Host "  verdict: $($result.verdict)"
    Write-Host ''
    Write-Host 'This audit never publishes a licence. The final choice belongs to the maintainer (issue #34).'
}

exit $exitCode
