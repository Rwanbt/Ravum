<#
.SYNOPSIS
    Intègre les propositions produites par plusieurs agents réels dans le
    workspace partagé, avec un seul écrivain Git.

.DESCRIPTION
    Lit `target/ravum/live/agent-*/report.json`, génère un diff unifié entre le
    fichier de base du workspace et le fichier produit par chaque agent, construit
    les propositions JSON, puis exécute `ravum-collab integrate`.

    Aucun agent n'écrit dans le workspace : leurs modifications restent dans leur
    répertoire privé jusqu'à l'intégration.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/live-collab.ps1 -Commit
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
    if (-not (Test-Path $exe)) { throw "Binaire manquant: $exe" }

    $live = Join-Path $Root 'target\ravum\live'
    $workspace = Join-Path $live 'workspace'
    if (-not (Test-Path $workspace)) { throw "Workspace absent: $workspace (voir docs/mvp/M1-evidence.md)" }

    if (-not (Test-Path (Join-Path $workspace '.git'))) {
        git -C $workspace init -q -b main
        git -C $workspace config user.name 'ravum-live'
        git -C $workspace config user.email 'live@ravum.invalid'
        git -C $workspace add .
        git -C $workspace commit -q -m 'chore: live base'
    }

    $base = & $exe base --workspace $workspace | ConvertFrom-Json
    Write-Host ("base snapshot = " + $base.snapshot_id)

    $baseFile = Join-Path $workspace 'two_functions.rs'
    $generated = Join-Path $live 'proposals'
    if (Test-Path $generated) { Remove-Item -Recurse -Force $generated }
    New-Item -ItemType Directory -Force -Path $generated | Out-Null

    $count = 0
    foreach ($dir in Get-ChildItem $live -Directory | Where-Object { $_.Name -like 'agent-*' } | Sort-Object Name) {
        $reportPath = Join-Path $dir.FullName 'report.json'
        $agentFile = Join-Path $dir.FullName 'two_functions.rs'
        if (-not (Test-Path $reportPath) -or -not (Test-Path $agentFile)) {
            Write-Host ("  " + $dir.Name + " : rapport ou fichier absent, ignoré")
            continue
        }
        $report = Get-Content $reportPath -Raw | ConvertFrom-Json

        # Unified diff between the base file and the agent's private copy.
        # `2>$null` alone is not enough on PowerShell 5.1: git writes CRLF warnings
        # to stderr and the error preference turns them into terminating errors.
        $previousPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $raw = & git -C $Root diff --no-index --no-prefix -- $baseFile $agentFile 2>$null
        $ErrorActionPreference = $previousPreference
        $start = -1
        for ($i = 0; $i -lt $raw.Count; $i++) {
            if ($raw[$i].StartsWith('@@')) { $start = $i; break }
        }
        if ($start -lt 0) {
            Write-Host ("  " + $dir.Name + " : aucune différence avec la base, ignoré")
            continue
        }
        $patchLines = @('--- a/two_functions.rs', '+++ b/two_functions.rs') + $raw[$start..($raw.Count - 1)]
        $patch = ($patchLines -join "`n") + "`n"

        $proposal = [ordered]@{
            schema_version       = 1
            agent_id             = [string]$report.agent_id
            task_id              = [string]$report.task_id
            base_snapshot_id     = [string]$base.snapshot_id
            target_paths         = @('two_functions.rs')
            target_symbols       = @($report.target_symbols)
            expected_hashes      = @{ 'two_functions.rs' = [string]$base.files.'two_functions.rs' }
            declared_dependencies = @($report.declared_dependencies)
            impacted_contracts   = @($report.impacted_contracts)
            unified_patch        = $patch
            note                 = [string]$report.summary
        }
        $count++
        $target = Join-Path $generated ("{0:D2}-{1}.json" -f $count, $report.agent_id)
        $json = $proposal | ConvertTo-Json -Depth 6
        [System.IO.File]::WriteAllText($target, $json, (New-Object System.Text.UTF8Encoding($false)))
        Write-Host ("  proposition: " + (Split-Path -Leaf $target))
    }

    $journal = Join-Path $live 'journal.jsonl'
    $args = @('integrate', '--workspace', $workspace, '--proposals', $generated, '--journal', $journal)
    if ($Commit) { $args += '--commit' }
    & $exe @args
    $code = $LASTEXITCODE

    Write-Host ''
    Write-Host '--- fichier intégré (extraits) ---'
    Get-Content (Join-Path $workspace 'two_functions.rs') | Select-String 'x \+ [0-9]+\.0|helper\(x\) \* [0-9]+\.0'
    Write-Host '--- Git ---'
    git -C $workspace log --oneline -n 3
    exit $code
}
finally {
    Pop-Location
}
