<#
.SYNOPSIS
    Lanceur local du graphe d'impact Ravum (MVP-0b, L4-E).

.DESCRIPTION
    Calcule les tests requis pour un diff (réel ou synthétique) avec
    `tools/impact/impact.mjs`, affiche la décision, et — avec -Run — exécute
    EXACTEMENT les commandes requises, sérialisées (`--test-threads=1`).

    Projections du code de sortie (aucun vert silencieux) :
      0  décision rendue et, avec -Run, toutes les commandes exécutées avec succès
      1  au moins une commande requise a échoué (avec -Run)
      2  mapping INCERTAIN (suite complète exécutée de secours avec -Run)
      3  outil d'impact introuvable/illisible, Node absent, arguments invalides
      4  décision FULL avec -FailOnFull (refus explicite, rien n'est exécuté)
      5  commande docs requise mais aucun interpréteur Python trouvé

    Contraintes machine (référence 16 Go, machine chargée) : les commandes sont
    exécutées une par une, avec CARGO_BUILD_JOBS=2 et --test-threads=1.

.PARAMETER Base
    Base Git du diff (défaut de l'outil : origin/main puis main).

.PARAMETER Head
    Tête Git du diff (défaut : HEAD). Permet de rejouer un diff historique réel.

.PARAMETER Files
    Diff synthétique, par ex. "crates/ravum-store/src/lib.rs" ou
    "R100:crates/a/src/lib.rs->crates/b/src/lib.rs" ; séparateur , ou ;.

.PARAMETER Explain
    Affiche le rapport lisible (preuve anti-classification-naïve incluse).

.PARAMETER Run
    Exécute réellement les commandes requises (défaut : calcul seulement).

.PARAMETER Offline
    Ajoute `--offline` aux commandes cargo (local, cache chaud) ; n'exclut
    aucun test.

.PARAMETER FailOnFull
    Si la décision est FULL, s'arrête avec le code 4 et un message explicite
    (au lieu d'exécuter la suite complète). Incompatible avec -Run.

.PARAMETER Json
    Affiche aussi le JSON brut de la décision.

.PARAMETER Root
    Racine du dépôt (défaut : racine du dépôt contenant ce script).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/impact.ps1 -Explain

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/impact.ps1 -Files "crates/ravum-todo-core/src/lib.rs" -Explain

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/impact.ps1 -Files "crates/ravum-store/src/lib.rs" -Run -Offline
#>
[CmdletBinding()]
param(
    [string]$Base = '',
    [string]$Head = '',
    [string]$Files = '',
    [switch]$Explain,
    [switch]$Run,
    [switch]$Offline,
    [switch]$FailOnFull,
    [switch]$Json,
    [string]$Root = ''
)

$ErrorActionPreference = 'Stop'

if (-not $Root) {
    $Root = Split-Path -Parent $PSScriptRoot
}
if (-not (Test-Path -LiteralPath (Join-Path $Root 'tools\impact\impact.mjs'))) {
    Write-Host "ERREUR: tools/impact/impact.mjs introuvable sous $Root" -ForegroundColor Red
    exit 3
}
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Host "ERREUR: Node.js introuvable dans PATH (Node 26 attendu)." -ForegroundColor Red
    exit 3
}
if ($Run -and $FailOnFull) {
    Write-Host "ERREUR: -FailOnFull refuse d'exécuter la suite complète, -Run demande de l'exécuter : paramètres incompatibles." -ForegroundColor Red
    exit 3
}

Push-Location $Root
$tmpJson = $null
$tmpErr = $null
try {
    $nodeArgs = @('tools/impact/impact.mjs')
    if ($PSBoundParameters.ContainsKey('Files')) {
        # '' est un diff synthétique vide VOLONTAIRE (=> FULL + INCERTAIN, exit 2).
        $nodeArgs += @('--files', $Files)
    }
    else {
        if ($Base) { $nodeArgs += @('--base', $Base) }
        if ($Head) { $nodeArgs += @('--head', $Head) }
    }
    $nodeArgs += '--json'
    if ($Explain) { $nodeArgs += '--explain' }
    if ($FailOnFull) { $nodeArgs += '--fail-on-full' }

    $tmpJson = New-TemporaryFile
    $tmpErr = New-TemporaryFile
    & node @nodeArgs 1> $tmpJson.FullName 2> $tmpErr.FullName
    $toolExit = $LASTEXITCODE

    $stderrText = Get-Content -LiteralPath $tmpErr.FullName -Raw -ErrorAction SilentlyContinue
    if ($stderrText -and $stderrText.Trim().Length -gt 0) {
        Write-Host $stderrText.TrimEnd()
    }
    $raw = Get-Content -LiteralPath $tmpJson.FullName -Raw -ErrorAction SilentlyContinue
}
finally {
    Pop-Location
    if ($tmpJson) { Remove-Item -LiteralPath $tmpJson.FullName -Force -ErrorAction SilentlyContinue }
    if ($tmpErr) { Remove-Item -LiteralPath $tmpErr.FullName -Force -ErrorAction SilentlyContinue }
}

if (-not $raw -or $raw.Trim().Length -eq 0) {
    Write-Host "ERREUR: le calcul d'impact n'a produit aucune décision (exit $toolExit)." -ForegroundColor Red
    exit 3
}

$decision = $null
try {
    $decision = $raw | ConvertFrom-Json
}
catch {
    Write-Host "ERREUR: JSON d'impact illisible: $($_.Exception.Message)" -ForegroundColor Red
    exit 3
}
if ($null -eq $decision.full) {
    Write-Host "ERREUR: décision d'impact sans champ 'full' (schéma invalide)." -ForegroundColor Red
    exit 3
}

if ($Json) {
    Write-Host $raw.TrimEnd()
    Write-Host ''
}

$decisionLabel = switch ([string]$decision.decision) {
    'selective'         { 'SÉLECTIVE' }
    'full-conservative' { 'FULL — règle conservatrice (issue légitime : vert si les tests passent)' }
    'full-uncertain'    { 'FULL — mapping INCERTAIN (échec explicite exigé)' }
    default             { if ($decision.full -eq $true) { 'FULL (suite complète)' } else { 'SÉLECTIVE' } }
}
$uncertainLabel = if ($decision.uncertain -eq $true) { 'OUI (échec explicite exigé)' } else { 'non' }
Write-Host ('Décision d''impact : ' + $decisionLabel)
Write-Host ('Mapping incertain  : ' + $uncertainLabel)
if ($decision.base) { Write-Host ('Base               : ' + $decision.base) }
if ($decision.head) { Write-Host ('Tête               : ' + $decision.head) }
Write-Host ('Fichiers modifiés  : ' + @($decision.changed).Count)
$packagesLabel = if ($decision.full -eq $true) { '(workspace entier)' } else { (@($decision.packages) -join ', ') }
Write-Host ('Paquets requis     : ' + $packagesLabel)

Write-Host ''
Write-Host 'Commandes requises :'
$commandIndex = 0
foreach ($command in @($decision.commands)) {
    $commandIndex += 1
    Write-Host ('  ' + $commandIndex + '. [' + $command.kind + '] ' + (@($command.argv) -join ' '))
    Write-Host ('     raison: ' + $command.reason)
}
if (@($decision.commands).Count -eq 0) {
    Write-Host "  (aucune — aucun test automatisé requis pour ce diff)"
}

if (@($decision.reasons).Count -gt 0) {
    Write-Host ''
    Write-Host 'Raisons :'
    foreach ($reason in @($decision.reasons)) { Write-Host ('  - ' + $reason) }
}
if (@($decision.unknown_paths).Count -gt 0) {
    Write-Host ''
    Write-Host 'Chemins inconnus (fail-closed) :'
    foreach ($unknown in @($decision.unknown_paths)) { Write-Host ('  - ' + $unknown) }
}

if ($decision.full -eq $true -and $FailOnFull) {
    Write-Host ''
    Write-Host "REFUS: le mapping exige la suite complète (FULL). Relancer sans -FailOnFull (ou avec -Run) pour l'exécuter." -ForegroundColor Yellow
    exit 4
}

if (-not $Run) {
    Write-Host ''
    Write-Host ('Mode calcul seulement (exit projeté: ' + $toolExit + '). Utiliser -Run pour exécuter les commandes requises.')
    exit $toolExit
}

# ---------------------------------------------------------------------------
# Exécution réelle, sérialisée.
# ---------------------------------------------------------------------------
function Resolve-PythonArgv {
    param([string[]]$Argv)
    $candidates = @('python', 'python3', 'py')
    foreach ($candidate in $candidates) {
        $resolved = Get-Command $candidate -ErrorAction SilentlyContinue
        if ($resolved) {
            $effective = @($resolved.Source) + @($Argv[1..($Argv.Length - 1)])
            if ($candidate -ne $Argv[0]) {
                Write-Host ("  note: interpréteur Python substitué ('" + $Argv[0] + "' introuvable, utilisation de '" + $candidate + "')")
            }
            return , $effective
        }
    }
    return $null
}

function Resolve-CargoArgv {
    param([string[]]$Argv)
    if (-not $Offline) { return , $Argv }
    # cargo test [--offline] ...
    $effective = @($Argv[0], $Argv[1], '--offline') + @($Argv[2..($Argv.Length - 1)])
    return , $effective
}

$env:CARGO_BUILD_JOBS = '2'
Write-Host ''
Write-Host 'Exécution SÉRIALISÉE (CARGO_BUILD_JOBS=2, --test-threads=1 dans les commandes) :'
$failures = New-Object System.Collections.Generic.List[string]
$executed = 0
$totalSeconds = 0.0

foreach ($command in @($decision.commands)) {
    $argv = @($command.argv)
    if ($command.kind -eq 'docs') {
        $effective = Resolve-PythonArgv -Argv $argv
        if ($null -eq $effective) {
            Write-Host "ERREUR: commande docs requise mais aucun interpréteur Python (python/python3/py) trouvé." -ForegroundColor Red
            exit 5
        }
    }
    elseif ($command.kind -eq 'cargo') {
        $effective = Resolve-CargoArgv -Argv $argv
    }
    else {
        Write-Host ("ERREUR: type de commande inconnu: " + $command.kind) -ForegroundColor Red
        exit 3
    }

    $executed += 1
    $label = (@($effective) -join ' ')
    Write-Host ''
    Write-Host ('[' + $executed + '/' + @($decision.commands).Count + '] ' + $label)
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    & $effective[0] @($effective[1..($effective.Length - 1)])
    $code = $LASTEXITCODE
    $stopwatch.Stop()
    $seconds = [math]::Round($stopwatch.Elapsed.TotalSeconds, 1)
    $totalSeconds += $seconds
    if ($code -ne 0) {
        Write-Host ('  ÉCHEC (exit ' + $code + ', ' + $seconds + ' s)') -ForegroundColor Red
        $failures.Add($label) | Out-Null
    }
    else {
        Write-Host ('  OK (' + $seconds + ' s)')
    }
}

Write-Host ''
Write-Host ('Bilan : ' + $executed + ' commande(s) exécutée(s) en ' + [math]::Round($totalSeconds, 1) + ' s, ' + $failures.Count + ' échec(s).')
if ($failures.Count -gt 0) {
    Write-Host 'Commandes en échec :' -ForegroundColor Red
    foreach ($failure in $failures) { Write-Host ('  - ' + $failure) -ForegroundColor Red }
    exit 1
}
if ($decision.uncertain -eq $true) {
    Write-Host 'ATTENTION: mapping incertain — exit 2 projeté même si la suite complète de secours est passée.' -ForegroundColor Yellow
    exit 2
}
Write-Host 'Toutes les commandes requises ont réussi.'
exit [int]$toolExit
