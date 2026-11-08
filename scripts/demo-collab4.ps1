#requires -Version 5.1
<#
.SYNOPSIS
    Démo reproductible COLLAB-4 / L3.4 (analyse ciblée des changements acceptés).

.DESCRIPTION
    Exécute `ravum collab-cycle` sur la fixture `fixtures/collab4/workspace`:

      1. `cargo build -p ravum-cli` (le binaire utilisé est celui du workspace);
      2. passage à froid: 2 propositions acceptées par le coordinateur, analyses
         daemon des seules unités affectées (crate_a + dépendant crate_b), crate_c
         sautée, puis un second passage identique dans le même processus (hits
         mémoire) — rapport JSON écrit comme preuve;
      3. second processus avec le même `--daemon-work`: cache disque persistant,
         zéro exécution `rustc` (preuve que le contenu, et non le chemin ou
         l'horloge, porte l'identité).

    Le script projette le code de sortie: il s'arrête en erreur si le cycle
    échoue ou si la seconde exécution relance le compilateur.

.PARAMETER EvidencePath
    Chemin du rapport JSON de preuve. Par défaut
    `evidence/mvp0b-collab4-<AAAA-MM-JJ>.json`.

.PARAMETER NoRebuild
    Ne pas relancer `cargo build` (utilise le binaire déjà présent).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\demo-collab4.ps1
#>
[CmdletBinding()]
param(
    [string] $EvidencePath = "",
    [switch] $NoRebuild
)

$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = (Get-Location).Path
}
$Workspace = Join-Path $RepoRoot 'fixtures\collab4\workspace'
$DemoWork = Join-Path $RepoRoot 'target\ravum\collab4-demo'
$DaemonWork = Join-Path $DemoWork 'daemon'
$JournalRoot = Join-Path $DemoWork 'journal'

if ([string]::IsNullOrWhiteSpace($EvidencePath)) {
    $Date = Get-Date -Format 'yyyy-MM-dd'
    $EvidencePath = Join-Path $RepoRoot "evidence\mvp0b-collab4-$Date.json"
}

function Write-Step([string] $Message) {
    Write-Host ''
    Write-Host "== $Message ==" -ForegroundColor Cyan
}

function Get-ReportField($Report, [string] $Path) {
    $Current = $Report
    foreach ($Part in $Path.Split('.')) {
        if ($null -eq $Current) { return $null }
        $Current = $Current.$Part
    }
    return $Current
}

Push-Location $RepoRoot
$ExitCode = 0
try {
    Write-Host 'COLLAB-4 / L3.4 - analyse ciblee des changements acceptes (MVP-0b)'
    Write-Host "workspace   : $Workspace"
    Write-Host "daemon-work : $DaemonWork"
    Write-Host "preuve      : $EvidencePath"

    if (-not (Test-Path -LiteralPath $Workspace)) {
        throw "fixture introuvable: $Workspace"
    }

    if (-not $NoRebuild) {
        Write-Step 'cargo build -p ravum-cli'
        & cargo build -p ravum-cli
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build -p ravum-cli a echoue (code $LASTEXITCODE)"
        }
    }

    $Binary = Join-Path $RepoRoot 'target\debug\ravum.exe'
    if (-not (Test-Path -LiteralPath $Binary)) {
        $Binary = Join-Path $RepoRoot 'target\debug\ravum'
    }
    if (-not (Test-Path -LiteralPath $Binary)) {
        throw "binaire ravum introuvable sous target\debug (lancez sans -NoRebuild)"
    }

    # Mesure à froid: le démon et son cache sont réinitialisés pour que les
    # chiffres du premier passage soient reproductibles.
    if (Test-Path -LiteralPath $DemoWork) {
        Remove-Item -LiteralPath $DemoWork -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $JournalRoot | Out-Null

    Write-Step 'passage 1 (processus neuf, --passes 2: hits memoire attendus au 2e passage)'
    $JournalOne = Join-Path $JournalRoot 'run1.jsonl'
    & $Binary collab-cycle $Workspace --daemon-work $DaemonWork --journal $JournalOne --out $EvidencePath --passes 2
    $FirstCode = $LASTEXITCODE
    Write-Host "exit passage 1 = $FirstCode"

    $RunOneReport = $null
    if (Test-Path -LiteralPath $EvidencePath) {
        $RunOneReport = Get-Content -LiteralPath $EvidencePath -Raw | ConvertFrom-Json
        $Passes = @($RunOneReport.passes)
        $Analyses = @($RunOneReport.analyses)
        $Skipped = @($RunOneReport.skipped_units)
        $Stats = $RunOneReport.daemon.stats

        Write-Host ("  unites analysees : {0}" -f (($Analyses | ForEach-Object { $_.unit } | Select-Object -Unique) -join ', '))
        Write-Host ("  unites sautees   : {0}" -f (($Skipped | ForEach-Object { $_.unit }) -join ', '))
        foreach ($Pass in $Passes) {
            Write-Host ("  passage {0}: analyses={1} hits={2} misses={3} rustc_runs={4}" -f `
                $Pass.pass, $Pass.analyses, $Pass.cache_hits, $Pass.cache_misses, $Pass.rustc_runs)
        }
        Write-Host ("  daemon: hits_memoire={0} hits_disque={1} misses={2} rustc_runs={3}" -f `
            $Stats.cache_hits_memory, $Stats.cache_hits_disk, $Stats.cache_misses, $Stats.rustc_runs)
        Write-Host ("  violations : {0}" -f (@($RunOneReport.violations).Count))
    } else {
        Write-Warning "rapport de preuve absent: $EvidencePath"
        $FirstCode = 2
    }

    Write-Step 'passage 2 (second processus, meme daemon-work: cache disque attendu, 0 rustc)'
    $JournalTwo = Join-Path $JournalRoot 'run2.jsonl'
    $SecondReportPath = Join-Path $DemoWork 'run2-report.json'
    & $Binary collab-cycle $Workspace --daemon-work $DaemonWork --journal $JournalTwo --out $SecondReportPath --passes 1
    $SecondCode = $LASTEXITCODE
    Write-Host "exit passage 2 = $SecondCode"

    $SecondRustcRuns = -1
    if (Test-Path -LiteralPath $SecondReportPath) {
        $RunTwoReport = Get-Content -LiteralPath $SecondReportPath -Raw | ConvertFrom-Json
        $SecondStats = $RunTwoReport.daemon.stats
        $SecondRustcRuns = [int64] $SecondStats.rustc_runs
        Write-Host ("  daemon: hits_memoire={0} hits_disque={1} misses={2} rustc_runs={3}" -f `
            $SecondStats.cache_hits_memory, $SecondStats.cache_hits_disk, $SecondStats.cache_misses, $SecondStats.rustc_runs)
        Write-Host ("  violations : {0}" -f (@($RunTwoReport.violations).Count))
    } else {
        Write-Warning "rapport du second passage absent: $SecondReportPath"
        $SecondCode = 2
    }

    Write-Step 'verification des proprietes attendues'
    $Failures = New-Object System.Collections.ArrayList
    if ($FirstCode -ne 0) {
        [void] $Failures.Add("le premier cycle a retourne $FirstCode")
    }
    if ($SecondCode -ne 0) {
        [void] $Failures.Add("le second cycle a retourne $SecondCode")
    }
    if ($null -ne $RunOneReport) {
        if ((@($RunOneReport.violations).Count) -ne 0) {
            [void] $Failures.Add("le premier cycle a enregistre des violations")
        }
        $PassList = @($RunOneReport.passes)
        if ($PassList.Count -ge 2) {
            if ([int64] $PassList[1].cache_misses -ne 0) {
                [void] $Failures.Add("le second passage en memoire a recompile du contenu identique")
            }
            if ([int64] $PassList[0].rustc_runs -ne 3) {
                [void] $Failures.Add("rustc_runs du premier passage inattendu ($($PassList[0].rustc_runs) au lieu de 3)")
            }
        }
        $AnalyzedUnits = @($RunOneReport.analyses | ForEach-Object { $_.unit } | Select-Object -Unique)
        $SkippedUnits = @($RunOneReport.skipped_units | ForEach-Object { $_.unit })
        if (-not ($AnalyzedUnits -contains 'crate_a')) { [void] $Failures.Add('crate_a non analysee') }
        if (-not ($AnalyzedUnits -contains 'crate_b')) { [void] $Failures.Add('crate_b (dependant) non analysee') }
        if ($AnalyzedUnits -contains 'crate_c') { [void] $Failures.Add('crate_c non affectee a ete analysee') }
        if (-not ($SkippedUnits -contains 'crate_c')) { [void] $Failures.Add('crate_c absente de skipped_units') }
    }
    if ($SecondRustcRuns -ne 0) {
        [void] $Failures.Add("le second processus a lance rustc ($SecondRustcRuns execution(s)) au lieu de servir le cache disque")
    }

    if ($Failures.Count -eq 0) {
        Write-Host 'OK: analyses ciblees, crate_c sautee, hits de cache memoire et disque verifies.' -ForegroundColor Green
    } else {
        foreach ($Failure in $Failures) {
            Write-Host "ECHEC: $Failure" -ForegroundColor Red
        }
        if ($FirstCode -ne 0) {
            $ExitCode = $FirstCode
        } elseif ($SecondCode -ne 0) {
            $ExitCode = $SecondCode
        } else {
            $ExitCode = 5
        }
    }

    Write-Host ''
    Write-Host "preuve JSON : $EvidencePath"
} catch {
    Write-Host "ERREUR: $($_.Exception.Message)" -ForegroundColor Red
    if ($ExitCode -eq 0) { $ExitCode = 2 }
} finally {
    Pop-Location
}

exit $ExitCode
