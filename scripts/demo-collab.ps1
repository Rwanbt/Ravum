<#
.SYNOPSIS
    Démonstration COLLAB-1 : propositions concurrentes de plusieurs agents,
    intégration contrôlée, conflit explicite, écrivain Git unique.

.DESCRIPTION
    1. Crée un mini-dépôt Git temporaire (jamais le dépôt Ravum) avec deux fichiers.
    2. Interroge le snapshot de base réel (`ravum-collab base`).
    3. Matérialise les propositions JSON depuis evidence/collab-demo/proposals
       en y injectant l'identité de snapshot et les hashes réels.
    4. Exécute le coordinateur et affiche le journal de décision.
    5. Avec -Commit, matérialise le snapshot intégré et commit uniquement les
       chemins intégrés, via l'écrivain Git unique.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/demo-collab.ps1 -Commit
#>
[CmdletBinding()]
param(
    [string]$Root = '',
    [switch]$Commit
)

$ErrorActionPreference = 'Stop'
if (-not $Root) { $Root = Split-Path -Parent $PSScriptRoot }
if (-not $Root) { $Root = (Get-Location).Path }
Push-Location $Root
try {
    $exe = Join-Path $Root 'target\debug\ravum-collab.exe'
    if (-not (Test-Path $exe)) {
        throw "Binaire manquant: $exe. Lancez d'abord: cargo build --workspace"
    }

    $work = Join-Path $Root 'target\ravum\collab-demo'
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force -Path $work | Out-Null
    Copy-Item (Join-Path $Root 'fixtures\collab\two_functions.rs') (Join-Path $work 'two_functions.rs')
    Copy-Item (Join-Path $Root 'fixtures\collab\notes.rs') (Join-Path $work 'notes.rs')

    git -C $work init -q -b main
    git -C $work config user.name 'ravum-demo'
    git -C $work config user.email 'demo@ravum.invalid'
    git -C $work add .
    git -C $work commit -q -m 'chore: base fixture'

    Write-Host '[1/4] Snapshot de base observé par les agents'
    $baseJson = & $exe base --workspace $work
    if ($LASTEXITCODE -ne 0) { throw 'ravum-collab base a échoué' }
    $base = $baseJson | ConvertFrom-Json
    Write-Host ("      snapshot_id = " + $base.snapshot_id)

    Write-Host '[2/4] Matérialisation des propositions'
    $generated = Join-Path $Root 'target\ravum\collab-proposals'
    if (Test-Path $generated) { Remove-Item -Recurse -Force $generated }
    New-Item -ItemType Directory -Force -Path $generated | Out-Null
    $templateDir = Join-Path $Root 'evidence\collab-demo\proposals'
    foreach ($template in Get-ChildItem $templateDir -Filter '*.json.tmpl' | Sort-Object Name) {
        $text = [System.IO.File]::ReadAllText($template.FullName, [System.Text.Encoding]::UTF8)
        $text = $text.Replace('{{BASE}}', $base.snapshot_id)
        $text = $text.Replace('{{HASH_TWO_FUNCTIONS}}', $base.files.'two_functions.rs')
        $target = Join-Path $generated ($template.Name -replace '\.tmpl$', '')
        # WriteAllText with explicit UTF-8 without BOM: a JSON parser must not have
        # to guess, and PowerShell 5.1's Set-Content adds a BOM.
        [System.IO.File]::WriteAllText($target, $text, (New-Object System.Text.UTF8Encoding($false)))
        Write-Host ("      " + (Split-Path -Leaf $target))
    }

    Write-Host '[3/4] Intégration par le coordinateur (écrivain Git unique)'
    $journal = Join-Path $Root 'target\ravum\collab-journal.jsonl'
    Remove-Item $journal -ErrorAction SilentlyContinue
    # The publication journal must live in the disposable demo workspace, not in
    # `target/`: CI restores a cached `target/` and a publication record surviving from a
    # previous run makes the idempotent writer refuse the same publication id with
    # `AlreadyPublished`. Scoping it to $work (recreated above) makes the demo re-runnable
    # and keeps the writer's idempotence intact instead of disabling it.
    $publicationJournal = Join-Path $work 'publication-journal.jsonl'
    $integrationArgs = @(
        'integrate',
        '--workspace', $work,
        '--proposals', $generated,
        '--journal', $journal,
        '--publication-journal', $publicationJournal
    )
    if ($Commit) { $integrationArgs += '--commit' }
    & $exe @integrationArgs
    $code = $LASTEXITCODE

    Write-Host ''
    Write-Host '[4/4] État final'
    Write-Host '--- contenu intégré de two_functions.rs (extrait) ---'
    Get-Content (Join-Path $work 'two_functions.rs') | Select-String 'x \+ 10\.0|helper\(x\) \* 3\.0'
    Write-Host '--- notes.rs (doit rester inchangé : proposition REBASE_REVALIDATE) ---'
    Get-Content (Join-Path $work 'notes.rs') | Select-String 'WINDOW'
    Write-Host '--- Git ---'
    git -C $work status --porcelain
    git -C $work log --oneline -n 3
    Write-Host ''
    Write-Host "Journal: $journal"

    exit $code
}
finally {
    Pop-Location
}
