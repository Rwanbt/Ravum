<#
.SYNOPSIS
    Démonstration COLLAB-2 / COLLAB-3 : même fichier, symboles différents, et
    vérification sémantique réelle du snapshot fusionné avant publication Git.

.DESCRIPTION
    Scénario 1 (COLLAB-2, positif)
      1. Crée un dépôt Git temporaire à partir d'une vraie crate
         (`fixtures/collab2/demo-crate` : Cargo.toml + src/lib.rs + tests).
      2. Interroge le snapshot de base (`ravum-collab base`).
      3. Matérialise deux propositions : agent A modifie `alpha`, agent B
         modifie `beta` — même fichier, symboles différents, aucune ligne
         partagée.
      4. Exécute `ravum-collab integrate --verify-tests --commit` : chaque
         `AUTO_SAFE` exige la compilation du snapshot fusionné
         (`rustc --emit=metadata`) ET l'exécution de ses tests
         (`rustc --test` + binaire de test) avant toute publication.
      5. Vérifie que la publication Git conserve les DEUX changements, puis
         recompile et réexécute indépendamment les tests du code publié.

    Scénario 2 (COLLAB-2/3, négatif)
      1. Deux renommages indépendants en apparence introduisent un nom dupliqué.
         Le premier s'intègre, le second produit un snapshot fusionné qui ne
         compile pas (E0428).
      2. La proposition fautive est REFUSÉE avec la preuve du compilateur
         (code + evidence id journalisés), et n'est pas publiée ; seul le
         premier changement est commité.

    Aucune écriture hors de `target/ravum/collab2-demo`. Le script propage son
    code de sortie (0 = tout est conforme ; 1 = une assertion ou une commande a
    échoué).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/demo-collab2.ps1
#>
[CmdletBinding()]
param(
    [string]$Root = '',
    [switch]$NoBuild
)

$ErrorActionPreference = 'Stop'

if (-not $Root) { $Root = Split-Path -Parent $PSScriptRoot }
if (-not $Root) { $Root = (Get-Location).Path }

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "ASSERTION FAILED: $Message" }
}

function Write-Utf8([string]$Path, [string]$Text) {
    $parent = Split-Path -Parent $Path
    if ($parent -and -not (Test-Path $parent)) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    # Explicit UTF-8 without BOM: the JSON proposal parser must not have to guess.
    [System.IO.File]::WriteAllText($Path, $Text, (New-Object System.Text.UTF8Encoding($false)))
}

function Read-Utf8([string]$Path) {
    return [System.IO.File]::ReadAllText($Path, [System.Text.Encoding]::UTF8)
}


function Invoke-Captured(
    [string]$FilePath,
    [string[]]$Arguments,
    [string]$StdoutPath,
    [string]$StderrPath
) {
    # Windows PowerShell 5.1 turns native stderr into a terminating error when it
    # is redirected AND $ErrorActionPreference is Stop. Capture around it with a
    # local Continue preference, then restore the caller's policy.
    $saved = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $captured = (& $FilePath @Arguments 2> $StderrPath) -join "`n"
        $code = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $saved
    }
    Write-Utf8 $StdoutPath $captured
    return $code
}
function Invoke-Git([string]$Repo, [string[]]$Arguments) {
    $output = & git -C $Repo @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw ("git " + ($Arguments -join ' ') + " a échoué: " + ($output -join "`n"))
    }
    return ($output -join "`n")
}

function New-Repo([string]$Path) {
    if (Test-Path $Path) { Remove-Item -Recurse -Force $Path }
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
    Invoke-Git $Path @('init', '-q', '-b', 'main') | Out-Null
    Invoke-Git $Path @('config', 'user.name', 'ravum-demo') | Out-Null
    Invoke-Git $Path @('config', 'user.email', 'demo@ravum.invalid') | Out-Null
    Invoke-Git $Path @('config', 'core.autocrlf', 'false') | Out-Null
    Invoke-Git $Path @('config', 'commit.gpgsign', 'false') | Out-Null
}

function Add-BaseCommit([string]$Repo) {
    Invoke-Git $Repo @('add', '.') | Out-Null
    Invoke-Git $Repo @('commit', '-q', '-m', 'chore: base fixture') | Out-Null
}

function Write-Proposal(
    [string]$Path,
    [string]$Agent,
    [string]$Task,
    [string[]]$Symbols,
    [string]$BaseSnapshot,
    [string]$FileHash,
    [string]$Patch
) {
    $proposal = [ordered]@{
        schema_version      = 1
        agent_id            = $Agent
        task_id             = $Task
        base_snapshot_id    = $BaseSnapshot
        target_paths        = @('src/lib.rs')
        target_symbols      = $Symbols
        expected_hashes     = [ordered]@{ 'src/lib.rs' = $FileHash }
        declared_dependencies = @()
        impacted_contracts  = @()
        unified_patch       = $Patch
        note                = $null
    }
    Write-Utf8 $Path (($proposal | ConvertTo-Json -Depth 8) + "`n")
}

Push-Location $Root
try {
    $exe = Join-Path $Root 'target\debug\ravum-collab.exe'
    if (-not $NoBuild) {
        Write-Host '[0/6] Build des binaires du coordinateur (machine chargée : une seule compilation)'
        & cargo build -p ravum-workspace-coordinator --bins
        Assert-True ($LASTEXITCODE -eq 0) "cargo build a échoué (exit $LASTEXITCODE)"
    }
    Assert-True (Test-Path $exe) "Binaire manquant: $exe (lancez sans -NoBuild)"

    $work = Join-Path $Root 'target\ravum\collab2-demo'
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force -Path $work | Out-Null

    $fixtures = Join-Path $Root 'fixtures\collab2'
    $rustc = if ($env:RUSTC) { $env:RUSTC } else { 'rustc' }

    # -----------------------------------------------------------------------
    # Scénario 1: deux fonctions indépendantes du même fichier, vérifiées
    # -----------------------------------------------------------------------
    Write-Host ''
    Write-Host '=== Scénario 1 (COLLAB-2) : deux agents, deux symboles, un fichier ==='
    $positive = Join-Path $work 'positive'
    $positiveArtifacts = Join-Path $work 'positive-artifacts'
    New-Item -ItemType Directory -Force -Path $positiveArtifacts | Out-Null
    New-Repo $positive
    Copy-Item (Join-Path $fixtures 'demo-crate\Cargo.toml') (Join-Path $positive 'Cargo.toml')
    New-Item -ItemType Directory -Force -Path (Join-Path $positive 'src') | Out-Null
    Copy-Item (Join-Path $fixtures 'demo-crate\src\lib.rs') (Join-Path $positive 'src\lib.rs')
    Add-BaseCommit $positive

    Write-Host '[1/6] Snapshot de base observé par les agents'
    $baseOutput = & $exe base --workspace $positive
    Assert-True ($LASTEXITCODE -eq 0) "ravum-collab base a échoué (exit $LASTEXITCODE)"
    $base = ($baseOutput -join "`n") | ConvertFrom-Json
    $baseSnapshot = $base.snapshot_id
    $baseHash = $base.files.'src/lib.rs'
    Write-Host ("      snapshot_id = " + $baseSnapshot)

    Write-Host '[2/6] Matérialisation des deux propositions (a.patch / b.patch)'
    $proposals = Join-Path $positiveArtifacts 'proposals'
    Write-Proposal (Join-Path $proposals '01-agent-a.json') 'agent-a' 'alpha' @('alpha') $baseSnapshot $baseHash (Read-Utf8 (Join-Path $fixtures 'demo-crate\a.patch'))
    Write-Proposal (Join-Path $proposals '02-agent-b.json') 'agent-b' 'beta' @('beta') $baseSnapshot $baseHash (Read-Utf8 (Join-Path $fixtures 'demo-crate\b.patch'))

    Write-Host '[3/6] Intégration avec vérification sémantique réelle (--verify-tests)'
    $positiveArgs = @(
        'integrate',
        '--workspace', $positive,
        '--proposals', $proposals,
        '--journal', (Join-Path $positiveArtifacts 'journal.jsonl'),
        '--publication-journal', (Join-Path $positiveArtifacts 'publications.jsonl'),
        '--work', (Join-Path $positiveArtifacts 'verify'),
        '--verify-tests',
        '--commit',
        '--author', 'ravum-demo',
        '--email', 'demo@ravum.invalid',
        '--json'
    )
    $positiveStdoutFile = Join-Path $positiveArtifacts 'integrate.stdout.json'
    $positiveStderrFile = Join-Path $positiveArtifacts 'integrate.stderr.log'
    $positiveCode = Invoke-Captured $exe $positiveArgs $positiveStdoutFile $positiveStderrFile
    $positiveStdout = Read-Utf8 $positiveStdoutFile
    $positiveErr = Read-Utf8 $positiveStderrFile
    if ($positiveErr -and $positiveErr.Trim().Length -gt 0) { Write-Host ("      avertissement: " + $positiveErr.Trim()) }
    Assert-True ($positiveCode -eq 0) "integrate (scénario 1) a échoué avec exit $positiveCode"
    $report = $positiveStdout | ConvertFrom-Json
    Assert-True ($report.verification.mode -eq 'semantic') 'la vérification sémantique doit être active'
    Assert-True ($report.verification.requested -eq $true) '--verify-tests doit être enregistré comme demande explicite'
    Assert-True ($report.summary.auto_safe -eq 2) ("AUTO_SAFE attendu = 2, obtenu = " + $report.summary.auto_safe)
    Assert-True ($report.summary.conflict -eq 0) 'aucun conflit attendu entre deux fonctions indépendantes'
    Assert-True ($report.accepted_changes.Count -eq 2) ("2 changements acceptés attendus, obtenu = " + $report.accepted_changes.Count)

    $recordIndex = 0
    foreach ($record in $report.accepted_changes) {
        $recordIndex = $recordIndex + 1
        Assert-True ($record.decision -eq 'AUTO_SAFE') ("record $recordIndex : décision inattendue " + $record.decision)
        Assert-True ($record.verification.status -eq 'passed') ("record $recordIndex : vérification non passée")
        $kinds = @($record.verification.checks | ForEach-Object { $_.kind })
        Assert-True ($kinds -contains 'metadata_check') ("record $recordIndex : metadata_check manquant")
        Assert-True ($kinds -contains 'test_compile') ("record $recordIndex : test_compile manquant")
        Assert-True ($kinds -contains 'test_run') ("record $recordIndex : test_run manquant")
        $testRun = @($record.verification.checks | Where-Object { $_.kind -eq 'test_run' })[0]
        Assert-True ($testRun.summary -match 'test result: ok') ("record $recordIndex : les tests du fusionné ne sont pas verts")
        Assert-True ($record.verification.evidence_id.Length -eq 64) ("record $recordIndex : evidence id invalide")
        Write-Host ("      record #{0}: {1} -> {2} [{3}] {4}" -f $recordIndex, $record.snapshot_before.Substring(0, 12), $record.snapshot_after.Substring(0, 12), $record.verification.status, $record.verification.evidence_id.Substring(0, 16))
        foreach ($check in $record.verification.checks) {
            Write-Host ("        - {0,-15} {1,-7} {2}" -f $check.kind, $check.status, $check.summary)
        }
    }

    Write-Host '[4/6] Publication Git : les deux changements doivent être conservés'
    $published = Invoke-Git $positive @('show', 'HEAD:src/lib.rs')
    Assert-True ($published -match 'fn alpha\(\) -> u32 \{\s*10') 'le changement de alpha doit être publié'
    Assert-True ($published -match 'fn beta\(\) -> u32 \{\s*20') 'le changement de beta doit être publié'
    $commitCount = (Invoke-Git $positive @('rev-list', '--count', 'HEAD')).Trim()
    Assert-True ($commitCount -eq '2') ("un seul commit de publication attendu, obtenu = " + $commitCount)
    $clean = (Invoke-Git $positive @('status', '--porcelain')).Trim()
    Assert-True ($clean.Length -eq 0) ("l'arbre de travail doit rester propre, obtenu: " + $clean)
    $body = Invoke-Git $positive @('log', '-1', '--format=%B')
    Assert-True ($body -match 'Ravum-Evidence:') 'le commit doit porter la preuve de vérification'
    Assert-True ($null -ne $report.publication) 'le rapport doit exposer la publication Git'
    Write-Host ("      commit = " + $report.publication.sha.Substring(0, 12) + " (parent " + $report.publication.parent.Substring(0, 12) + ")")

    Write-Host '[5/6] Contre-vérification indépendante du code publié (rustc --test)'
    $publishedSource = Join-Path $positiveArtifacts 'published-lib.rs'
    Write-Utf8 $publishedSource ($published + "`n")
    $testExe = Join-Path $positiveArtifacts 'published-tests.exe'
    & $rustc --edition 2021 --test -o $testExe $publishedSource
    Assert-True ($LASTEXITCODE -eq 0) "le code publié ne compile pas en mode test (exit $LASTEXITCODE)"
    $testStdoutFile = Join-Path $positiveArtifacts 'published-tests.stdout.log'
    $testStderrFile = Join-Path $positiveArtifacts 'published-tests.stderr.log'
    $testCode = Invoke-Captured $testExe @() $testStdoutFile $testStderrFile
    $testOutput = (Read-Utf8 $testStdoutFile) + "`n" + (Read-Utf8 $testStderrFile)
    Write-Host ("      " + (($testOutput -split "`n") | Where-Object { $_ -match 'test result' }))
    Assert-True ($testCode -eq 0) "les tests du code publié échouent (exit $testCode)"

    # -----------------------------------------------------------------------
    # Scénario 2: fusion textuellement propre, compilation cassée
    # -----------------------------------------------------------------------
    Write-Host ''
    Write-Host '=== Scénario 2 (COLLAB-2/3) : la fusion textuelle ne compile pas ==='
    $negative = Join-Path $work 'negative'
    $negativeArtifacts = Join-Path $work 'negative-artifacts'
    New-Item -ItemType Directory -Force -Path $negativeArtifacts | Out-Null
    New-Repo $negative
    New-Item -ItemType Directory -Force -Path (Join-Path $negative 'src') | Out-Null
    Copy-Item (Join-Path $fixtures 'name_collision\src\lib.rs') (Join-Path $negative 'src\lib.rs')
    Add-BaseCommit $negative

    $negativeBaseOutput = & $exe base --workspace $negative
    Assert-True ($LASTEXITCODE -eq 0) "ravum-collab base (scénario 2) a échoué"
    $negativeBase = ($negativeBaseOutput -join "`n") | ConvertFrom-Json
    $negativeProposals = Join-Path $negativeArtifacts 'proposals'
    Write-Proposal (Join-Path $negativeProposals '01-rename-a.json') 'agent-a' 'rename-a' @('slot_a') $negativeBase.snapshot_id $negativeBase.files.'src/lib.rs' (Read-Utf8 (Join-Path $fixtures 'name_collision\a.patch'))
    Write-Proposal (Join-Path $negativeProposals '02-rename-b.json') 'agent-b' 'rename-b' @('slot_b') $negativeBase.snapshot_id $negativeBase.files.'src/lib.rs' (Read-Utf8 (Join-Path $fixtures 'name_collision\b.patch'))

    $negativeArgs = @(
        'integrate',
        '--workspace', $negative,
        '--proposals', $negativeProposals,
        '--journal', (Join-Path $negativeArtifacts 'journal.jsonl'),
        '--publication-journal', (Join-Path $negativeArtifacts 'publications.jsonl'),
        '--work', (Join-Path $negativeArtifacts 'verify'),
        '--verify',
        '--commit',
        '--json'
    )
    $negativeStdoutFile = Join-Path $negativeArtifacts 'integrate.stdout.json'
    $negativeStderrFile = Join-Path $negativeArtifacts 'integrate.stderr.log'
    $negativeCode = Invoke-Captured $exe $negativeArgs $negativeStdoutFile $negativeStderrFile
    $negativeStdout = Read-Utf8 $negativeStdoutFile
    Assert-True ($negativeCode -eq 0) "integrate (scénario 2) a échoué avec exit $negativeCode"
    $negativeReport = $negativeStdout | ConvertFrom-Json
    Assert-True ($negativeReport.summary.auto_safe -eq 1) ("AUTO_SAFE attendu = 1, obtenu = " + $negativeReport.summary.auto_safe)
    Assert-True ($negativeReport.summary.reject -eq 1) ("REJECT attendu = 1, obtenu = " + $negativeReport.summary.reject)
    Assert-True ($negativeReport.accepted_changes.Count -eq 1) 'seul le premier changement doit être accepté'
    $refused = $negativeReport.decisions[1]
    Assert-True ($refused.decision -eq 'REJECT') ("décision attendue REJECT, obtenue " + $refused.decision)
    $refusalText = ($refused.reasons -join ' | ')
    Assert-True ($refusalText -match 'E0428') 'le refus doit citer le diagnostic du compilateur (E0428)'
    Write-Host ("      refus = " + $refusalText)

    $negativeHead = Invoke-Git $negative @('show', 'HEAD:src/lib.rs')
    Assert-True ($negativeHead -match 'fn helper2') 'le premier renommage doit être publié'
    Assert-True ($negativeHead -match 'fn slot_b') 'le second renommage ne doit pas être publié'
    Assert-True (-not ($negativeHead -match 'fn slot_a')) 'le renommage refusé ne doit pas apparaître'
    $negativeCommitCount = (Invoke-Git $negative @('rev-list', '--count', 'HEAD')).Trim()
    Assert-True ($negativeCommitCount -eq '2') ("un seul commit de publication attendu, obtenu = " + $negativeCommitCount)

    Write-Host ''
    Write-Host '[6/6] Résultat global'
    Write-Host '      Scénario 1 : AUTO_SAFE=2, snapshot fusionné compilé + tests exécutés, publication Git à deux changements.'
    Write-Host '      Scénario 2 : REJECT avec preuve E0428, proposition fautive non publiée.'
    Write-Host ("      Journaux: " + (Join-Path $positiveArtifacts 'journal.jsonl') + " ; " + (Join-Path $negativeArtifacts 'journal.jsonl'))
    Write-Host 'DÉMO COLLAB-2/3 : SUCCÈS'
    exit 0
}
catch {
    Write-Host ''
    Write-Host ('DÉMO COLLAB-2/3 : ÉCHEC — ' + $_.Exception.Message)
    exit 1
}
finally {
    Pop-Location
}



