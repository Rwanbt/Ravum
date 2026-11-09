#requires -Version 7.0
<#
.SYNOPSIS
    Genere un SBOM SPDX 2.3 deterministe du workspace Ravum.

.DESCRIPTION
    1. Execute `cargo metadata --format-version 1 --locked` (local, sans reseau).
    2. Passe le JSON a `tools/sbom/sbom.mjs` (Node standard uniquement).
    3. Verifie que le fichier produit est un JSON SPDX 2.3 valide, affiche les compteurs
       et le SHA-256.

    Le SBOM decrit les membres du workspace et les dependances resolues, avec
    `licenseDeclared` copie de Cargo (NOASSERTION sinon), `licenseConcluded` toujours
    NOASSERTION, et des relations DEPENDS_ON issues du graphe `--locked`.

    Determinisme : identique pour les memes metadonnees et le meme horodatage.
    Sans -Timestamp, `created` vaut l'heure UTC courante ; utiliser
    `$env:SOURCE_DATE_EPOCH='0'` ou -Timestamp '1970-01-01T00:00:00Z' pour un
    fichier reproductible octet pour octet.

.PARAMETER Output
    Fichier de sortie. Defaut : <repo>\target\sbom\ravum.spdx.json.

.PARAMETER RepoRoot
    Racine du depot. Defaut : parent du repertoire du script.

.PARAMETER Timestamp
    Horodatage ISO-8601 UTC fixe (ex. 1970-01-01T00:00:00Z).

.PARAMETER Name
    Nom du document SPDX. Defaut : ravum-workspace.

.EXAMPLE
    pwsh -File scripts/sbom.ps1
    pwsh -File scripts/sbom.ps1 -Output C:\tmp\sbom.json -Timestamp 1970-01-01T00:00:00Z

.NOTES
    Sorties : 0 succes, 1 echec (cargo/node/JSON), 2 usage invalide.
#>
[CmdletBinding()]
param(
    [string]$Output,
    [string]$RepoRoot,
    [string]$Timestamp,
    [string]$Name = 'ravum-workspace'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$EXIT_OK = 0
$EXIT_FAIL = 1
$EXIT_USAGE = 2

function Fail {
    param([string]$Message, [int]$Code = 1)
    [Console]::Error.WriteLine("sbom: error: $Message")
    exit $Code
}

if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
}
$root = (Resolve-Path $RepoRoot).Path
$manifest = Join-Path $root 'Cargo.toml'
if (-not (Test-Path -LiteralPath $manifest)) { Fail "Cargo.toml introuvable : $manifest" $EXIT_USAGE }

$generator = Join-Path $root 'tools\sbom\sbom.mjs'
if (-not (Test-Path -LiteralPath $generator)) { Fail "generateur introuvable : $generator" $EXIT_FAIL }

if ([string]::IsNullOrWhiteSpace($Output)) {
    $Output = Join-Path $root 'target\sbom\ravum.spdx.json'
}
$outFull = [IO.Path]::GetFullPath($Output)

if (-not (Get-Command node -ErrorAction SilentlyContinue)) { Fail "node introuvable dans PATH" $EXIT_FAIL }
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Fail "cargo introuvable dans PATH" $EXIT_FAIL }

$outDir = Split-Path -Parent $outFull
if ($outDir) { New-Item -ItemType Directory -Force -Path $outDir | Out-Null }

$tmp = [IO.Path]::Combine([IO.Path]::GetTempPath(), ('ravum-metadata-' + [guid]::NewGuid().ToString('N') + '.json'))
try {
    $metadataJson = $null
    Push-Location $root
    try {
        $metadataJson = (& cargo metadata --format-version 1 --locked --manifest-path $manifest 2>$null | Out-String)
        if ($LASTEXITCODE -ne 0) { Fail "cargo metadata a echoue (code $LASTEXITCODE)" $EXIT_FAIL }
    } finally {
        Pop-Location
    }
    if ([string]::IsNullOrWhiteSpace($metadataJson)) { Fail "cargo metadata a produit une sortie vide" $EXIT_FAIL }
    [IO.File]::WriteAllText($tmp, $metadataJson, (New-Object System.Text.UTF8Encoding($false)))

    $commit = 'nogit'
    try {
        $gitOut = & git -C $root rev-parse --short=12 HEAD 2>$null
        if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($gitOut)) { $commit = $gitOut.Trim() }
    } catch {
        $commit = 'nogit'
    }

    $nodeArgs = @($generator, '--metadata-file', $tmp, '--out', $outFull, '--name', $Name, '--commit', $commit)
    if (-not [string]::IsNullOrWhiteSpace($Timestamp)) { $nodeArgs += @('--timestamp', $Timestamp) }
    & node @nodeArgs
    if ($LASTEXITCODE -ne 0) { Fail "ravum-sbom a echoue (code $LASTEXITCODE)" $EXIT_FAIL }

    if (-not (Test-Path -LiteralPath $outFull)) { Fail "fichier SBOM absent apres generation : $outFull" $EXIT_FAIL }
    $doc = Get-Content -LiteralPath $outFull -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($doc.spdxVersion -ne 'SPDX-2.3') { Fail "spdxVersion inattendu : $($doc.spdxVersion)" $EXIT_FAIL }
    if ($null -eq $doc.packages -or $doc.packages.Count -eq 0) { Fail "aucun package dans le SBOM" $EXIT_FAIL }

    $sha = (Get-FileHash -LiteralPath $outFull -Algorithm SHA256).Hash.ToLowerInvariant()
    Write-Host "sbom: wrote $outFull"
    Write-Host ("sbom: spdxVersion={0} packages={1} relationships={2} created={3}" -f $doc.spdxVersion, $doc.packages.Count, @($doc.relationships).Count, $doc.creationInfo.created)
    Write-Host "sbom: sha256=$sha"
    exit $EXIT_OK
} finally {
    Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
}

