#requires -Version 7.0
<#
.SYNOPSIS
    Installe (ou desinstalle) le compilateur Ravum `ravum-lang` depuis les sources locales.

.DESCRIPTION
    Construit `ravum-lang-cli` en release verrouillee (`cargo build --release --locked`),
    puis installe le binaire et un manifeste de provenance dans un prefixe dedie :
      <Prefix>\bin\ravum-lang.exe
      <Prefix>\ravum-install.json

    Fail-closed : si le build, la copie ou le remplacement echoue, la version
    precedente est restauree et le script sort en erreur. Aucune ecriture hors du
    prefixe (le build ecrit dans target/ du depot).

    Aucun reseau n'est utilise par ce script : `--locked` s'appuie sur Cargo.lock et
    le cache local. Aucune signature n'est produite (voir docs/release/PROVENANCE.md).

.PARAMETER Prefix
    Repertoire d'installation. Defaut : %LOCALAPPDATA%\Ravum.
    Le binaire va dans <Prefix>\bin et le manifeste a la racine du prefixe.

.PARAMETER AddToPath
    Ajoute <Prefix>\bin au PATH *utilisateur* (pas systeme), de facon idempotente.
    Un nouveau terminal est necessaire pour que le changement soit visible.

.PARAMETER Uninstall
    Supprime le binaire, le manifeste et les repertoires vides. Idempotent.

.PARAMETER RemoveFromPath
    Avec -Uninstall : retire <Prefix>\bin du PATH utilisateur (idempotent).

.PARAMETER SkipBuild
    N'execute pas cargo build ; exige un binaire deja present dans target\release.

.EXAMPLE
    pwsh -File scripts/install.ps1
    pwsh -File scripts/install.ps1 -Prefix C:\Tools\Ravum -AddToPath
    pwsh -File scripts/install.ps1 -Prefix C:\Tools\Ravum -Uninstall -RemoveFromPath

.NOTES
    Sorties : 0 succes, 1 echec (build/copie/manifeste), 2 usage invalide.
    Le manifeste contient la version, le commit, la date UTC, le SHA-256 du binaire
    et la version de rustc. Il ne contient aucun secret.
#>
[CmdletBinding()]
param(
    [string]$Prefix = (Join-Path $env:LOCALAPPDATA 'Ravum'),
    [switch]$AddToPath,
    [switch]$Uninstall,
    [switch]$RemoveFromPath,
    [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$EXIT_OK = 0
$EXIT_FAIL = 1
$EXIT_USAGE = 2

function Fail {
    param([string]$Message, [int]$Code = 1)
    [Console]::Error.WriteLine("ravum-install: error: $Message")
    exit $Code
}

function Info {
    param([string]$Message)
    Write-Host "ravum-install: $Message"
}

function Get-UserPathValue {
    return [Environment]::GetEnvironmentVariable('Path', 'User')
}

function Normalize-PathEntry {
    param([string]$Entry)
    return $Entry.Trim().TrimEnd('\', '/')
}

function Test-PathContainsEntry {
    param([string]$PathValue, [string]$Entry)
    if ([string]::IsNullOrWhiteSpace($PathValue)) { return $false }
    $target = Normalize-PathEntry $Entry
    foreach ($part in $PathValue.Split(';')) {
        if ([string]::IsNullOrWhiteSpace($part)) { continue }
        if ((Normalize-PathEntry $part) -ieq $target) { return $true }
    }
    return $false
}

function Add-UserPathEntry {
    param([string]$Entry)
    $current = Get-UserPathValue
    if (Test-PathContainsEntry $current $Entry) {
        Info "PATH utilisateur contient deja : $Entry"
        return
    }
    $newValue = if ([string]::IsNullOrWhiteSpace($current)) { $Entry } else { $current.TrimEnd(';') + ';' + $Entry }
    [Environment]::SetEnvironmentVariable('Path', $newValue, 'User')
    Info "PATH utilisateur mis a jour (ouvrir un nouveau terminal) : $Entry"
}

function Remove-UserPathEntry {
    param([string]$Entry)
    $current = Get-UserPathValue
    if ([string]::IsNullOrWhiteSpace($current)) { return $false }
    $target = Normalize-PathEntry $Entry
    $kept = @($current.Split(';') | Where-Object {
        -not [string]::IsNullOrWhiteSpace($_) -and (Normalize-PathEntry $_) -ine $target
    })
    $newValue = ($kept -join ';')
    if ($newValue -eq $current) { return $false }
    [Environment]::SetEnvironmentVariable('Path', $newValue, 'User')
    return $true
}

# ---------------------------------------------------------------- validation
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not (Test-Path -LiteralPath (Join-Path $repoRoot 'Cargo.toml'))) {
    Fail "Cargo.toml introuvable dans $repoRoot : executer depuis le depot Ravum." $EXIT_USAGE
}
if ([string]::IsNullOrWhiteSpace($Prefix)) {
    Fail "-Prefix est vide." $EXIT_USAGE
}
$prefixFull = [IO.Path]::GetFullPath($Prefix).TrimEnd('\', '/')
if ($prefixFull -eq [IO.Path]::GetFullPath($repoRoot).TrimEnd('\', '/')) {
    Fail "-Prefix ne peut pas etre la racine du depot." $EXIT_USAGE
}
if ($RemoveFromPath -and -not $Uninstall) {
    Fail "-RemoveFromPath exige -Uninstall." $EXIT_USAGE
}

$binDir = Join-Path $prefixFull 'bin'
$destExe = Join-Path $binDir 'ravum-lang.exe'
$manifestPath = Join-Path $prefixFull 'ravum-install.json'

# --------------------------------------------------------------- uninstall
if ($Uninstall) {
    $removed = New-Object System.Collections.Generic.List[string]
    if (Test-Path -LiteralPath $destExe) {
        Remove-Item -LiteralPath $destExe -Force
        $removed.Add($destExe)
    }
    if (Test-Path -LiteralPath $manifestPath) {
        Remove-Item -LiteralPath $manifestPath -Force
        $removed.Add($manifestPath)
    }
    if (Test-Path -LiteralPath $binDir) {
        $remaining = @(Get-ChildItem -LiteralPath $binDir -Force)
        if ($remaining.Count -eq 0) {
            Remove-Item -LiteralPath $binDir -Force
            $removed.Add($binDir)
        }
    }
    if ($RemoveFromPath) {
        if (Remove-UserPathEntry $binDir) {
            Info "PATH utilisateur nettoye : $binDir"
            $removed.Add('PATH utilisateur')
        } else {
            Info "PATH utilisateur ne contenait pas : $binDir"
        }
    }
    if ($removed.Count -eq 0) {
        Info "rien a desinstaller (prefixe : $prefixFull)"
    } else {
        Info ("desinstalle : " + ($removed -join ', '))
    }
    exit $EXIT_OK
}

# ------------------------------------------------------------------- build
$builtExe = Join-Path $repoRoot 'target\release\ravum-lang.exe'
if (-not $SkipBuild) {
    Info "cargo build -p ravum-lang-cli --release --locked"
    Push-Location $repoRoot
    try {
        & cargo build -p ravum-lang-cli --release --locked
        if ($LASTEXITCODE -ne 0) {
            Fail "cargo build a echoue (code $LASTEXITCODE)." $EXIT_FAIL
        }
    } finally {
        Pop-Location
    }
}
if (-not (Test-Path -LiteralPath $builtExe)) {
    Fail "binaire construit introuvable : $builtExe" $EXIT_FAIL
}

# ------------------------------------------------------- metadonnees locales
$versionLine = ((& $builtExe --version 2>&1) | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($versionLine)) {
    Fail "ravum-lang --version a echoue : $versionLine" $EXIT_FAIL
}
$match = [regex]::Match($versionLine, '^ravum-lang\s+(\S+)')
if (-not $match.Success) {
    Fail "sortie --version inattendue : $versionLine" $EXIT_FAIL
}
$packageVersion = $match.Groups[1].Value

$commit = 'UNKNOWN'
try {
    $gitOut = & git -C $repoRoot rev-parse HEAD 2>$null
    if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($gitOut)) { $commit = $gitOut.Trim() }
} catch {
    $commit = 'UNKNOWN'
}

$gitDirty = $false
try {
    $statusOut = & git -C $repoRoot status --porcelain 2>$null
    if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace(($statusOut | Out-String))) {
        $gitDirty = $true
    }
} catch {
    $gitDirty = $false
}

$rustcLine = 'UNKNOWN'
try {
    $rustcOut = & rustc -V 2>$null
    if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($rustcOut)) { $rustcLine = $rustcOut.Trim() }
} catch {
    $rustcLine = 'UNKNOWN'
}

$sha256 = (Get-FileHash -LiteralPath $builtExe -Algorithm SHA256).Hash.ToLowerInvariant()
$installedAtUtc = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')

$manifest = [ordered]@{
    schema_version   = 1
    name             = 'ravum-lang'
    version          = $versionLine
    package_version  = $packageVersion
    git_commit       = $commit
    git_dirty        = $gitDirty
    installed_at_utc = $installedAtUtc
    sha256           = $sha256
    rustc            = $rustcLine
    platform         = 'windows'
    arch             = if ($env:PROCESSOR_ARCHITECTURE) { $env:PROCESSOR_ARCHITECTURE } else { 'UNKNOWN' }
    profile          = 'release'
    locked           = $true
    binary           = 'bin/ravum-lang.exe'
    prefix           = $prefixFull
    installer        = 'scripts/install.ps1'
}

# --------------------------------------------------- staging + move atomique
$staging = Join-Path $prefixFull ('.staging-' + [guid]::NewGuid().ToString('N'))
$stagingBin = Join-Path $staging 'bin'
$backupExe = $destExe + '.bak'
$backupManifest = $manifestPath + '.bak'
$hadExe = Test-Path -LiteralPath $destExe
$hadManifest = Test-Path -LiteralPath $manifestPath

try {
    New-Item -ItemType Directory -Force -Path $stagingBin | Out-Null
    Copy-Item -LiteralPath $builtExe -Destination (Join-Path $stagingBin 'ravum-lang.exe') -Force
    $json = ($manifest | ConvertTo-Json -Depth 4) + "`n"
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText((Join-Path $staging 'ravum-install.json'), $json, $utf8NoBom)

    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    if ($hadExe) { Copy-Item -LiteralPath $destExe -Destination $backupExe -Force }
    if ($hadManifest) { Copy-Item -LiteralPath $manifestPath -Destination $backupManifest -Force }

    [IO.File]::Move((Join-Path $stagingBin 'ravum-lang.exe'), $destExe, $true)
    [IO.File]::Move((Join-Path $staging 'ravum-install.json'), $manifestPath, $true)
} catch {
    if ($hadExe -and (Test-Path -LiteralPath $backupExe)) {
        Copy-Item -LiteralPath $backupExe -Destination $destExe -Force -ErrorAction SilentlyContinue
    }
    if ($hadManifest -and (Test-Path -LiteralPath $backupManifest)) {
        Copy-Item -LiteralPath $backupManifest -Destination $manifestPath -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
    Fail "installation echouee (binaire en cours d'utilisation ?) : $($_.Exception.Message)" $EXIT_FAIL
} finally {
    Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $backupExe -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $backupManifest -Force -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------- verification
$installedSha = (Get-FileHash -LiteralPath $destExe -Algorithm SHA256).Hash.ToLowerInvariant()
if ($installedSha -ne $sha256) {
    Fail "SHA-256 installe different du binaire construit ($installedSha != $sha256)" $EXIT_FAIL
}

if ($AddToPath) {
    Add-UserPathEntry $binDir
}

Info "installe : $versionLine"
Info "binaire  : $destExe"
Info "manifeste: $manifestPath"
Info "sha256   : $sha256"
Info "commit   : $commit"
if ($gitDirty) {
    [Console]::Error.WriteLine("ravum-install: warning: arbre de travail modifie (git_dirty=true) : le binaire peut contenir des changements non commités.")
}
if (-not $AddToPath) {
    Info "PATH non modifie. Ajouter : $binDir (ou relancer avec -AddToPath)."
}
exit $EXIT_OK
