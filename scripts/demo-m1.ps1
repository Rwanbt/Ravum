<#
.SYNOPSIS
    Démonstration M1 : modification du code -> analyse -> compilation -> vérification
    -> activation à chaud dans un processus déjà en cours d'exécution.

.DESCRIPTION
    Le processus de démonstration n'est jamais redémarré. Le script :
      1. prépare une copie de travail de la fixture calculator v1 ;
      2. lance `ravum watch` qui démarre l'application hôte ;
      3. réécrit la source avec calculator v2 ;
      4. vérifie que la valeur observée change SANS changement de PID.

    Sortie attendue : des ticks calculés par v1, l'enregistrement d'activation,
    puis des ticks calculés par v2 dans le même processus.

    PowerShell 5.1 compatible. Dépendances : cargo/rustc/git uniquement.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/demo-m1.ps1
#>
[CmdletBinding()]
param(
    [string]$Root = '',
    [int]$StartupWaitMs = 4000,
    [int]$TimeoutMs = 60000
)

$ErrorActionPreference = 'Stop'
# $PSScriptRoot is not reliably bound during parameter binding in PowerShell 5.1.
if (-not $Root) { $Root = Split-Path -Parent $PSScriptRoot }
if (-not $Root) { $Root = (Get-Location).Path }

Push-Location $Root
try {
    $exe = Join-Path $Root 'target\debug\ravum.exe'
    $hostExe = Join-Path $Root 'target\debug\ravum-demo-host.exe'
    foreach ($path in @($exe, $hostExe)) {
        if (-not (Test-Path $path)) {
            throw "Binaire manquant: $path. Lancez d'abord: cargo build --workspace"
        }
    }

    $demoDir = Join-Path $Root 'target\ravum\demo'
    New-Item -ItemType Directory -Force -Path $demoDir | Out-Null
    $workFile = Join-Path $demoDir 'compute.rs'
    Copy-Item (Join-Path $Root 'fixtures\calculator\v1.rs') $workFile -Force

    $stdout = Join-Path $Root 'target\ravum\demo-watch.out'
    $stderr = Join-Path $Root 'target\ravum\demo-watch.err'
    Remove-Item $stdout, $stderr -ErrorAction SilentlyContinue

    Write-Host '[1/4] Démarrage de ravum watch (application hôte + boucle continue)'
    $proc = Start-Process -FilePath $exe `
        -ArgumentList @('watch', $workFile, '--max-activations', '1', '--duration-ms', '45000', '--interval-ms', '100', '--post-activation-ms', '800', '--json') `
        -WorkingDirectory $Root -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr

    Start-Sleep -Milliseconds $StartupWaitMs

    Write-Host '[2/4] Édition de la source : calculator v1 -> v2 (x*2 devient x*2+1)'
    Copy-Item (Join-Path $Root 'fixtures\calculator\v2.rs') $workFile -Force

    Write-Host '[3/4] Attente de la boucle (analyse, compilation, vérification, activation)'
    if (-not $proc.WaitForExit($TimeoutMs)) {
        $proc.Kill()
        throw "Délai dépassé: ravum watch n'a pas terminé en $TimeoutMs ms"
    }

    $events = @()
    if (Test-Path $stdout) {
        foreach ($line in Get-Content $stdout) {
            if ([string]::IsNullOrWhiteSpace($line)) { continue }
            try { $events += ($line | ConvertFrom-Json) } catch { }
        }
    }

    $activationIndex = -1
    for ($i = 0; $i -lt $events.Count; $i++) {
        if ($events[$i].event -eq 'loop' -and $events[$i].outcome -eq 'activated') {
            $activationIndex = $i
            break
        }
    }
    if ($activationIndex -lt 0) {
        Write-Host '--- stdout brut ---'
        Get-Content $stdout -ErrorAction SilentlyContinue
        Write-Host '--- stderr brut ---'
        Get-Content $stderr -ErrorAction SilentlyContinue
        Write-Host ''
        Write-Host 'ÉCHEC: aucune activation observée.' -ForegroundColor Red
        exit 1
    }

    $before = @()
    $after = @()
    for ($i = 0; $i -lt $events.Count; $i++) {
        if ($events[$i].event -ne 'tick') { continue }
        if ($i -lt $activationIndex) { $before += $events[$i] } else { $after += $events[$i] }
    }
    $activation = $events[$activationIndex]

    Write-Host '[4/4] Résultat'
    Write-Host ("      PID hôte                    : " + $activation.host_pid)
    Write-Host ("      calculs avant activation    : " + $before.Count + " ticks, version " + ($before[-1].version) + ", dernière valeur " + ($before[-1].value))
    Write-Host ("      activation                  : " + $activation.outcome + " en " + $activation.timings.total_ms + " ms")
    Write-Host ("        check=" + $activation.timings.check_ms + "ms build=" + $activation.timings.build_ms + "ms verify=" + $activation.timings.verify_ms + "ms load=" + $activation.timings.load_ms + "ms activate=" + $activation.timings.activate_ms + "ms")
    if ($after.Count -gt 0) {
        Write-Host ("      calculs après activation    : " + $after.Count + " ticks, version " + ($after[-1].version) + ", dernière valeur " + ($after[-1].value))
    } else {
        Write-Host '      calculs après activation    : (aucun tick capturé)'
    }

    if ($after.Count -lt 1) {
        Write-Host ''
        Write-Host 'ÉCHEC: aucun calcul observé après activation (preuve incomplète).' -ForegroundColor Red
        exit 1
    }
    if ($before[-1].version -eq $after[-1].version) {
        Write-Host ''
        Write-Host 'ÉCHEC: la version rapportée par l''hôte n''a pas changé.' -ForegroundColor Red
        exit 1
    }

    Write-Host ''
    Write-Host 'SUCCÈS: le même processus a changé de comportement sans redémarrage.' -ForegroundColor Green
    Write-Host 'Enregistrement complet de l''activation:'
    $activation | ConvertTo-Json -Depth 6
}
finally {
    Pop-Location
}
