#!/usr/bin/env pwsh
# Attest-release — sign and verify release artifacts with an SSH (Ed25519) key.
#
# The scheme is OpenSSH signature ("sshsig"): `ssh-keygen -Y sign` produces a
# detached `.sig` next to each artifact, and `ssh-keygen -Y verify` checks it
# against an `allowed_signers` file that pins the trusted public key.
#
# IMPORTANT — what this script does NOT do is decide *whose* key is trusted.
# Generating a key here would make the signature worthless (the private key would
# sit next to the artifact it signs). The key, its custody and the
# `allowed_signers` file are a human decision. This script only proves that the
# mechanism works end to end on the real artifacts.
#
# Usage:
#   pwsh -File scripts/attest-release.ps1 -Mode sign   -Key <private> -Files a,b
#   pwsh -File scripts/attest-release.ps1 -Mode verify -Signers <allowed_signers> -Identity ravum-release -Files a,b
#
# Signing namespace: `ravum-release` (a signature made for another purpose cannot
# be replayed here).

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('sign', 'verify')][string]$Mode,
    [Parameter(Mandatory = $true)][string[]]$Files,
    [string]$Key,
    [string]$Signers,
    [string]$Identity = 'ravum-release',
    [string]$Namespace = 'ravum-release'
)

$ErrorActionPreference = 'Stop'
$sshKeygen = (Get-Command ssh-keygen -ErrorAction SilentlyContinue).Source
if (-not $sshKeygen) {
    throw 'ssh-keygen not found: OpenSSH is required for artifact signing'
}

foreach ($file in $Files) {
    if (-not (Test-Path $file)) {
        throw "artifact not found: $file"
    }
}

if ($Mode -eq 'sign') {
    if (-not $Key) { throw '-Key is required for signing' }
    if (-not (Test-Path $Key)) { throw "signing key not found: $Key" }
    foreach ($file in $Files) {
        & $sshKeygen -Y sign -f $Key -n $Namespace $file 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "signing failed for $file" }
        Write-Host "signed   : $file -> $file.sig"
    }
    exit 0
}

if (-not $Signers) { throw '-Signers is required for verification' }
if (-not (Test-Path $Signers)) { throw "allowed_signers not found: $Signers" }

$failed = 0
foreach ($file in $Files) {
    $signature = "$file.sig"
    if (-not (Test-Path $signature)) {
        Write-Host "MISSING  : $signature"
        $failed += 1
        continue
    }
    # `ssh-keygen -Y verify` reads the message on stdin; PowerShell has no `<`
    # operator and piping a binary through the pipeline would corrupt it, so the
    # redirection is delegated to cmd.exe with fully quoted paths.
    $command = '"{0}" -Y verify -f "{1}" -I "{2}" -n "{3}" -s "{4}" < "{5}"' -f `
        $sshKeygen, $Signers, $Identity, $Namespace, $signature, $file
    $output = & cmd.exe /c $command 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Host "verified : $file"
    }
    else {
        Write-Host "REJECTED : $file"
        $output | ForEach-Object { Write-Host "           $_" }
        $failed += 1
    }
}

if ($failed -eq 0) {
    Write-Host "attest-release: PASS — every artifact matches its signature"
    exit 0
}
Write-Host "attest-release: FAIL — $failed artifact(s) rejected"
exit 1
