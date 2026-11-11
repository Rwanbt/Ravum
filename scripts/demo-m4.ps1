<#
.SYNOPSIS
    Démonstration M4 : UI + API + SQLite, contrat versionné, validation serveur,
    autorisation, migration de schéma.

.DESCRIPTION
    Démarre le serveur de référence sur un port local, exécute le parcours complet
    (création, lecture, mise à jour, suppression) puis les cas négatifs (jeton absent,
    validation refusée), et vérifie les codes HTTP attendus. Le serveur est arrêté à
    la fin, quoi qu'il arrive.

    PowerShell 5.1 compatible. Aucune dépendance hors le binaire déjà compilé.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/demo-m4.ps1
#>
[CmdletBinding()]
param(
    [string]$Root = '',
    [int]$Port = 8099,
    [int]$StartupTimeoutMs = 15000
)

$ErrorActionPreference = 'Stop'
if (-not $Root) { $Root = Split-Path -Parent $PSScriptRoot }
if (-not $Root) { $Root = (Get-Location).Path }

Push-Location $Root
$proc = $null
try {
    $exe = Join-Path $Root 'target\debug\ravum-todo.exe'
    if (-not (Test-Path $exe)) {
        throw "Binaire manquant: $exe. Lancez d'abord: cargo build --workspace"
    }
    $workDir = Join-Path $Root 'target\ravum\demo-m4'
    if (Test-Path $workDir) { Remove-Item -Recurse -Force $workDir }
    New-Item -ItemType Directory -Force -Path $workDir | Out-Null

    $stdout = Join-Path $workDir 'server.out'
    $stderr = Join-Path $workDir 'server.err'
    Write-Host "[1/5] Démarrage du serveur sur le port $Port"
    $proc = Start-Process -FilePath $exe `
        -ArgumentList @('--db', (Join-Path $workDir 'todo.db'), '--port', "$Port", '--token', 'demo-token') `
        -WorkingDirectory $Root -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr

    $base = "http://127.0.0.1:$Port"
    $ready = $false
    $deadline = (Get-Date).AddMilliseconds($StartupTimeoutMs)
    while (-not $ready -and (Get-Date) -lt $deadline) {
        try {
            $health = Invoke-RestMethod -Uri "$base/health" -Method Get -TimeoutSec 2
            $ready = $true
        } catch {
            Start-Sleep -Milliseconds 200
        }
    }
    if (-not $ready) {
        throw "Le serveur n'a pas répondu dans le délai imparti"
    }
    Write-Host ("      health: schema_version=" + $health.schema_version + " contract_version=" + $health.contract_version)

    $auth = @{ Authorization = 'Bearer demo-token' }

    Write-Host '[2/5] Contrat partagé servi par le serveur'
    $schema = Invoke-RestMethod -Uri "$base/api/schema" -Method Get
    Write-Host ("      contract_version=" + $schema.contract_version + " compatibilité: " + $schema.compatibility)

    Write-Host '[3/5] Parcours complet (création -> lecture -> mise à jour -> suppression)'
    $created = Invoke-RestMethod -Uri "$base/api/tasks" -Method Post -Headers $auth -ContentType 'application/json' -Body '{"title":"  écrire la démonstration  ","due_date":"2026-11-09"}'
    $id = $created.task.id
    Write-Host ("      créé id=$id title='" + $created.task.title + "' due=" + $created.task.due_date)

    $patched = Invoke-RestMethod -Uri "$base/api/tasks/$id" -Method Patch -Headers $auth -ContentType 'application/json' -Body '{"done":true}'
    Write-Host ("      done=" + $patched.task.done)

    try {
        Invoke-WebRequest -Uri "$base/api/tasks/$id" -Method Delete -Headers $auth -UseBasicParsing | Out-Null
        Write-Host '      suppression: 204'
    } catch {
        throw "La suppression a échoué: $($_.Exception.Message)"
    }

    $after = Invoke-RestMethod -Uri "$base/api/tasks" -Method Get
    Write-Host ("      tâches restantes: " + @($after.tasks).Count)

    Write-Host '[4/5] Cas négatifs'
    $failures = 0

    try {
        Invoke-RestMethod -Uri "$base/api/tasks" -Method Post -ContentType 'application/json' -Body '{"title":"sans jeton"}' | Out-Null
        Write-Host '      ÉCHEC: une écriture sans jeton a été acceptée' -ForegroundColor Red
        $failures++
    } catch {
        $code = [int]$_.Exception.Response.StatusCode
        Write-Host ("      sans jeton -> HTTP $code (attendu 401)")
        if ($code -ne 401) { $failures++ }
    }

    try {
        Invoke-RestMethod -Uri "$base/api/tasks" -Method Post -Headers $auth -ContentType 'application/json' -Body '{"title":"   "}' | Out-Null
        Write-Host '      ÉCHEC: un titre vide a été accepté' -ForegroundColor Red
        $failures++
    } catch {
        $code = [int]$_.Exception.Response.StatusCode
        $body = $_.ErrorDetails.Message
        Write-Host ("      titre vide -> HTTP $code (attendu 400)")
        Write-Host ("      corps: " + $body)
        if ($code -ne 400) { $failures++ }
        if ($body -notmatch 'title_empty') { $failures++ }
    }

    Write-Host '[5/5] Migration de schéma (test E2E dédié, base v1 -> v2 avec données préservées)'
    & cargo test -q -p ravum-todo --test e2e e2e_migration_from_a_legacy_database -- --exact 2>&1 | Select-Object -Last 3
    if ($LASTEXITCODE -ne 0) {
        Write-Host '      ÉCHEC: la migration n a pas préservé les données' -ForegroundColor Red
        $failures++
    } else {
        Write-Host '      migration vérifiée: ligne v1 conservée, due_date ajoutée et utilisable'
    }

    if ($failures -gt 0) {
        Write-Host ''
        Write-Host "ÉCHEC: $failures vérification(s) en erreur" -ForegroundColor Red
        exit 1
    }
    Write-Host ''
    Write-Host 'SUCCÈS: parcours complet, validation, autorisation et migration vérifiés.' -ForegroundColor Green
    exit 0
}
finally {
    if ($proc -and -not $proc.HasExited) {
        $proc.Kill()
        $proc.WaitForExit()
    }
    Pop-Location
}
