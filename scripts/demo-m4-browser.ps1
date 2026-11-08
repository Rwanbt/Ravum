<#
.SYNOPSIS
    E2E navigateur réel du MVP-0b : lance un Chromium/Edge headless via CDP et vérifie
    le DOM réellement affiché par l'application témoin ravum-todo (serveur réel, SQLite
    réelle), sans aucune dépendance npm.

.DESCRIPTION
    Étapes :
      1. vérifie Node.js (>= 24 : WebSocket/fetch natifs et node:sqlite sans drapeau) ;
      2. compile le serveur de référence (`cargo build -p ravum-todo`) ;
      3. exécute apps/ravum-todo/e2e/browser_e2e.mjs avec RAVUM_TODO_BIN pointant sur le
         binaire compilé (évite 10+ invocations cargo et la contention du verrou cargo) ;
      4. propage le code de sortie du harnais :
           0 = tous les scénarios passent,
           1 = au moins un scénario échoue (ou erreur de harnais),
           2 = SKIP explicite : aucun navigateur trouvé, le JS n'a PAS été validé.
    Le rapport JSON et les captures sont écrits sous target\ravum\browser-e2e (ignoré par
    git) ; ils servent de preuve et sont publiés par la CI comme artefact.

    PowerShell 5.1 compatible. Aucun paquet npm n'est installé.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/demo-m4-browser.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/demo-m4-browser.ps1 -RequireBrowser -Screenshots
#>
[CmdletBinding()]
param(
    [string]$Root = '',
    [switch]$RequireBrowser,
    [switch]$Screenshots,
    [string]$Only = ''
)

$ErrorActionPreference = 'Stop'
# Avoid mojibake for accented output in Windows PowerShell 5.1 consoles/CI logs.
try {
    $utf8 = New-Object System.Text.UTF8Encoding $false
    [Console]::OutputEncoding = $utf8
} catch {
    # Not fatal: only affects console rendering.
}
if (-not $Root) { $Root = Split-Path -Parent $PSScriptRoot }
if (-not $Root) { $Root = (Get-Location).Path }

Push-Location $Root
try {
    Write-Host '[1/3] Environnement Node.js'
    $node = Get-Command node -ErrorAction SilentlyContinue
    if (-not $node) {
        throw "Node.js introuvable dans le PATH. Le harnais exige Node >= 24 (WebSocket/fetch natifs et node:sqlite sans drapeau, zéro dépendance npm)."
    }
    $nodeVersion = (& node --version).Trim()
    $major = [int](($nodeVersion -replace '^v', '') -split '\.')[0]
    Write-Host "      node $nodeVersion ($($node.Source))"
    if ($major -lt 24) {
        throw "Node $nodeVersion est trop ancien : le harnais exige Node >= 24 (node:sqlite sans drapeau, WebSocket/fetch natifs)."
    }

    Write-Host '[2/3] Compilation du serveur de référence (ravum-todo)'
    & cargo build -q -p ravum-todo
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build -p ravum-todo a échoué (code $LASTEXITCODE)"
    }
    $exe = Join-Path $Root 'target\debug\ravum-todo.exe'
    if (-not (Test-Path $exe)) {
        throw "Binaire introuvable après compilation : $exe"
    }
    $env:RAVUM_TODO_BIN = $exe
    Write-Host "      $exe"

    $artifactDir = Join-Path $Root 'target\ravum\browser-e2e'
    if (Test-Path $artifactDir) { Remove-Item -Recurse -Force $artifactDir }
    New-Item -ItemType Directory -Force -Path $artifactDir | Out-Null
    $env:RAVUM_E2E_REPORT = Join-Path $artifactDir 'report.json'
    if ($Screenshots) {
        $shots = Join-Path $artifactDir 'screenshots'
        New-Item -ItemType Directory -Force -Path $shots | Out-Null
        $env:RAVUM_E2E_SCREENSHOTS = $shots
    }
    if ($RequireBrowser) {
        # CI: un runner windows-latest fournit Edge ; l'absence de navigateur doit être
        # un échec explicite, jamais un faux vert.
        $env:RAVUM_E2E_REQUIRE_BROWSER = '1'
    } else {
        Remove-Item Env:RAVUM_E2E_REQUIRE_BROWSER -ErrorAction SilentlyContinue
    }

    Write-Host '[3/3] Découverte du navigateur et scénarios CDP'
    $script = Join-Path $Root 'apps\ravum-todo\e2e\browser_e2e.mjs'
    if (-not (Test-Path $script)) {
        throw "Harnais introuvable : $script"
    }
    $nodeArguments = @($script)
    if ($Only) { $nodeArguments += "--only=$Only" }
    & node @nodeArguments
    $code = $LASTEXITCODE

    Write-Host ''
    switch ($code) {
        0 { Write-Host "SUCCÈS: le JavaScript de l'interface a été validé dans un vrai navigateur." -ForegroundColor Green }
        2 { Write-Host "SKIP: aucun navigateur détecté ; le test navigateur n'a PAS été exécuté (code 2)." -ForegroundColor Yellow }
        default { Write-Host "ÉCHEC: le test navigateur a échoué (code $code)." -ForegroundColor Red }
    }
    if (Test-Path $env:RAVUM_E2E_REPORT) { Write-Host "Rapport JSON : $env:RAVUM_E2E_REPORT" }
    exit $code
}
finally {
    Pop-Location
}
