<#
.SYNOPSIS
    Lance le benchmark S8 : worktrees Git vs workspace partagé Ravum vs agents
    séquentiels, sur une même fixture Rust et un même corpus d'éditions.

.DESCRIPTION
    Wrapper reproductible autour de benchmarks/s8/run-s8.mjs (Node 26, aucune
    dépendance npm). Le harnais compile le binaire ravum-collab si nécessaire,
    puis exécute des répétitions alternées A/B/C et écrit les preuves JSON dans
    evidence/.

    IMPORTANT : les « agents » sont des éditeurs SCRIPTÉS DÉTERMINISTES, pas des
    LLM. Aucun token IA n'est consommé ni mesuré ; le temps de supervision
    humaine n'est pas mesuré non plus.

.PARAMETER Reps
    Nombre de répétitions par scénario. Le protocole officiel exige >= 5.

.PARAMETER Scenarios
    Sous-ensemble de scénarios à exécuter (défaut : A,B,C).

.PARAMETER Keep
    Conserve les répliques temporaires (pour inspection) au lieu de les supprimer.

.PARAMETER SkipBuild
    Ne recompile pas ravum-collab (utilise le binaire existant).

.PARAMETER OutDir
    Répertoire des preuves JSON (défaut : evidence/ à la racine du dépôt).

.EXAMPLE
    pwsh -File scripts/s8.ps1
    pwsh -File scripts/s8.ps1 -Reps 5 -Keep
    pwsh -File scripts/s8.ps1 -Scenarios A,B -Reps 3 -SkipBuild

.NOTES
    Code de sortie propagé depuis Node : 0 = campagnes conformes et exploitables,
    1 = échec de protocole (tests rouges, édition indépendante perdue,
    publication non transactionnelle, etc.).
#>
[CmdletBinding()]
param(
    [ValidateRange(1, 100)][int]$Reps = 5,
    [ValidateSet('A', 'B', 'C')][string[]]$Scenarios = @('A', 'B', 'C'),
    [switch]$Keep,
    [switch]$SkipBuild,
    [string]$OutDir = ''
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
if (-not $root) { $root = (Get-Location).Path }
$runner = Join-Path $root 'benchmarks/s8/run-s8.mjs'

Push-Location $root
try {
    $nodeCommand = Get-Command node -ErrorAction Stop
    $node = $nodeCommand.Source
    if (-not (Test-Path -LiteralPath $runner)) {
        throw "Harnais introuvable : $runner"
    }

    $arguments = @(
        $runner,
        '--reps', [string]$Reps,
        '--scenarios', ($Scenarios -join ',')
    )
    if ($Keep) { $arguments += '--keep' }
    if ($SkipBuild) { $arguments += '--skip-build' }
    if ($OutDir) { $arguments += @('--out-dir', $OutDir) }

    Write-Host "[s8] racine   : $root"
    Write-Host "[s8] commande : $node $($arguments -join ' ')"
    & $node @arguments
    $exitCode = $LASTEXITCODE
    Write-Host "[s8] code de sortie : $exitCode"
    exit $exitCode
}
finally {
    Pop-Location
}
