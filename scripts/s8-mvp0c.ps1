<#
.SYNOPSIS
    Lance le pilote scripté S8 MVP-0c, ou prépare/collecte une campagne réelle.

.DESCRIPTION
    Wrapper reproductible autour de benchmarks/s8-mvp0c/run.mjs (Node 26, aucune
    dépendance npm). Par défaut : pilote scripté A/B/C, 3 répétitions, ordre
    alterné, CARGO_BUILD_JOBS=2, sans réseau. Le code de sortie de Node est
    propagé tel quel.

    IMPORTANT : les agents du pilote sont des éditeurs SCRIPTÉS DÉTERMINISTES,
    pas des LLM. Aucun token IA n'est mesuré au pilote ; le temps de supervision
    humaine n'est pas mesuré non plus. La campagne réelle (subagents DSH, tokens
    via tools/agent-usage/usage.mjs) est décrite dans docs/mvp/MVP-0c-s8.md.

.PARAMETER Reps
    Nombre de répétitions par stratégie au pilote (>= 3 pour le protocole).

.PARAMETER Jobs
    Valeur de CARGO_BUILD_JOBS (défaut 2, borne de la mission).

.PARAMETER Keep
    Conserve les répertoires temporaires de travail (inspection/débogage).

.PARAMETER Out
    Chemin de la preuve JSON produite.

.PARAMETER Mode
    pilot (défaut) ou live.

.PARAMETER Phase
    Mode live : prepare | collect | aggregate.

.PARAMETER Scenario
    Mode live prepare : A | B | C.

.PARAMETER Rep
    Numéro de répétition de la campagne live (métadonnée).

.PARAMETER Campaign
    Mode live : répertoire de campagne (prepare/collect).

.PARAMETER Campaigns
    Mode live aggregate : liste de répertoires de campagne séparés par des virgules.

.PARAMETER DoneJson
    Mode live collect : fichier d'horodatages par agent (facultatif).

.PARAMETER Usage
    Mode live collect : auto (défaut) | none | chemin d'un rapport usage.mjs.

.PARAMETER Rest
    Arguments supplémentaires transmis tels quels à run.mjs.

.EXAMPLE
    pwsh -File scripts/s8-mvp0c.ps1
    pwsh -File scripts/s8-mvp0c.ps1 -Reps 3 -Keep
    pwsh -File scripts/s8-mvp0c.ps1 -Mode live -Phase prepare -Scenario A -Rep 1 -Campaign C:\tmp\s8-A1
    pwsh -File scripts/s8-mvp0c.ps1 -Mode live -Phase collect -Campaign C:\tmp\s8-A1 -Out evidence\mvp0c-s8-live-A1.json

.NOTES
    Code de sortie propagé depuis Node : 0 = conforme, 1 = échec de protocole
    ou erreur harnais.
#>
[CmdletBinding()]
param(
    [ValidateRange(1, 50)][int]$Reps = 3,
    [ValidateRange(1, 16)][int]$Jobs = 2,
    [switch]$Keep,
    [string]$Out = '',
    [ValidateSet('pilot', 'live')][string]$Mode = 'pilot',
    [ValidateSet('prepare', 'collect', 'aggregate')][string]$Phase = '',
    [ValidateSet('A', 'B', 'C')][string]$Scenario = '',
    [int]$Rep = 1,
    [string]$Campaign = '',
    [string]$Campaigns = '',
    [string]$DoneJson = '',
    [ValidateSet('auto', 'none')][string]$Usage = 'auto',
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest = @()
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
if (-not $root) { $root = (Get-Location).Path }
$runner = Join-Path $root 'benchmarks/s8-mvp0c/run.mjs'

Push-Location $root
try {
    $nodeCommand = Get-Command node -ErrorAction Stop
    $node = $nodeCommand.Source
    if (-not (Test-Path -LiteralPath $runner)) { throw "Harnais introuvable : $runner" }

    $arguments = @($runner, '--mode', $Mode, '--reps', [string]$Reps, '--jobs', [string]$Jobs)
    if ($Mode -eq 'pilot') {
        if ($Keep) { $arguments += '--keep' }
        if ($Out) { $arguments += @('--out', $Out) }
    } else {
        if (-not $Phase) { throw "mode live : -Phase prepare|collect|aggregate est requis" }
        $arguments += @('--phase', $Phase)
        if ($Phase -eq 'prepare') {
            if (-not $Scenario) { throw "mode live prepare : -Scenario A|B|C est requis" }
            $arguments += @('--scenario', $Scenario, '--rep', [string]$Rep)
        }
        if ($Campaign) { $arguments += @('--campaign', $Campaign) }
        if ($Campaigns) { $arguments += @('--campaigns', $Campaigns) }
        if ($DoneJson) { $arguments += @('--done-json', $DoneJson) }
        if ($Phase -eq 'collect') { $arguments += @('--usage', $Usage) }
        if ($Out) { $arguments += @('--out', $Out) }
    }
    if ($Rest.Count -gt 0) { $arguments += $Rest }

    Write-Host "[s8-mvp0c] racine   : $root"
    Write-Host "[s8-mvp0c] commande : $node $($arguments -join ' ')"
    & $node @arguments
    $exitCode = $LASTEXITCODE
    Write-Host "[s8-mvp0c] code de sortie : $exitCode"
    exit $exitCode
}
finally {
    Pop-Location
}
