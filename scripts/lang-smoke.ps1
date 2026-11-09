# Smoke test de la chaîne Ravum (Windows PowerShell)
#
# Exécute la vraie chaîne `ravum-lang` sur les projets `examples/` et vérifie
# stdout/exit codes. Aucun résultat simulé : un échec est un échec, une commande
# absente est NOT_RUN.
#
#   pwsh -File scripts/lang-smoke.ps1            # build + smoke
#   pwsh -File scripts/lang-smoke.ps1 -SkipBuild # utilise le binaire existant
param(
    [switch]$SkipBuild,
    [string]$Bin = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$binPath = if ($Bin) { $Bin } else { Join-Path $root "target\debug\ravum-lang.exe" }
if (-not (Test-Path $binPath) -and -not $SkipBuild) {
    Write-Host "== build ravum-lang =="
    Push-Location $root
    cargo build -p ravum-lang-cli --bin ravum-lang
    if ($LASTEXITCODE -ne 0) { Pop-Location; Write-Host "BUILD FAILED"; exit 1 }
    Pop-Location
}
if (-not (Test-Path $binPath)) { Write-Host "NOT_RUN: binaire introuvable: $binPath"; exit 3 }

$script:results = @()
function Record($case, $status, $detail) {
    $script:results += [pscustomobject]@{ Case = $case; Status = $status; Detail = $detail }
    $color = switch ($status) { "PASS" { "Green" } "FAIL" { "Red" } default { "Yellow" } }
    Write-Host ("[{0}] {1} {2}" -f $status, $case, $detail) -ForegroundColor $color
}

function Invoke-Case {
    param([string]$Case, [string]$Dir, [string[]]$CmdArgs, [string]$ExpectStdout = "", [int]$ExpectExit = 0, [int]$TimeoutSec = 120)
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ravum-smoke-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        $work = if ($Dir) { $Dir } else { $tmp }
        $outFile = Join-Path $tmp "stdout.txt"
        $errFile = Join-Path $tmp "stderr.txt"
        $p = Start-Process -FilePath $binPath -ArgumentList $CmdArgs -WorkingDirectory $work -Wait -PassThru -NoNewWindow -RedirectStandardOutput $outFile -RedirectStandardError $errFile
        $stdout = (Get-Content $outFile -Raw -ErrorAction SilentlyContinue) ?? ""
        $stderr = (Get-Content $errFile -Raw -ErrorAction SilentlyContinue) ?? ""
        if ($p.ExitCode -ne $ExpectExit) {
            Record $Case "FAIL" ("exit=" + $p.ExitCode + " attendu=" + $ExpectExit + " stderr=" + ($stderr.Trim() -replace "`r?`n", " | "))
            return $null
        }
        if ($ExpectStdout -and -not $stdout.Contains($ExpectStdout)) {
            Record $Case "FAIL" ("stdout ne contient pas '" + $ExpectStdout + "' : " + ($stdout.Trim() -replace "`r?`n", " | "))
            return $null
        }
        Record $Case "PASS" ""
        return $stdout
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

# --- commandes de base ---
$v = Invoke-Case "version" "" @("--version") "ravum-lang" 0
if ($null -eq $v) { Write-Host "STOP: le binaire ne répond pas à --version"; $script:results | Format-Table -AutoSize; exit 1 }

Invoke-Case "doctor" "" @("doctor") "rustc" 0

# --- init + hello de zéro ---
$initDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ravum-init-" + [guid]::NewGuid().ToString("N"))
try {
    Invoke-Case "init" "" @("init", $initDir) "" 0
    if (Test-Path (Join-Path $initDir "ravum.toml")) { Record "init-files" "PASS" "" } else { Record "init-files" "FAIL" "ravum.toml absent" }
    Invoke-Case "init-check" $initDir @("check") "" 0
    Invoke-Case "init-build" $initDir @("build") "" 0
    Invoke-Case "init-run" $initDir @("run") "hello" 0
} finally {
    Remove-Item -Recurse -Force $initDir -ErrorAction SilentlyContinue
}

# --- exemples ---
function Run-Example([string]$Name, [string]$Expect, [string[]]$RunArgs = @(), [string]$ExpectExit = 0) {
    $dir = Join-Path $root ("examples\" + $Name)
    Invoke-Case ("check:" + $Name) $dir @("check") "" 0 | Out-Null
    Invoke-Case ("build:" + $Name) $dir @("build") "" 0 | Out-Null
    $args = @("run")
    if ($RunArgs.Count -gt 0) { $args += "--"; $args += $RunArgs }
    Invoke-Case ("run:" + $Name) $dir $args $Expect $ExpectExit | Out-Null
}

Run-Example "hello" "hello, ravum"
Run-Example "fib" "55"
Run-Example "calculator" "8" @("5", "+", "3")
Run-Example "stats" "2.5" @((Join-Path $root "examples\stats\sample.txt"))
Run-Example "dsp-gain-iir" "0.1264" @()
Run-Example "package-demo\app" "42"
Run-Example "ffi-abs" "42"
Invoke-Case "test:tests-demo" (Join-Path $root "examples\tests-demo") @("test") "passed" 0 | Out-Null
Invoke-Case "fmt-check:hello" (Join-Path $root "examples\hello") @("fmt", "--check") "" 0 | Out-Null

# --- échecs attendus (diagnostics) ---
$badDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ravum-bad-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $badDir | Out-Null
try {
    Set-Content -Path (Join-Path $badDir "bad.rv") -Value "fn main() { let x: i32 = `"text`"; }"
    Invoke-Case "check-fail-json" $badDir @("check", "--json") "E-TYP-MISMATCH" 1 | Out-Null
} finally {
    Remove-Item -Recurse -Force $badDir -ErrorAction SilentlyContinue
}

# --- résumé ---
Write-Host ""
$pass = ($script:results | Where-Object Status -eq "PASS").Count
$fail = ($script:results | Where-Object Status -eq "FAIL").Count
$notrun = ($script:results | Where-Object Status -eq "NOT_RUN").Count
Write-Host ("RESUME: {0} PASS / {1} FAIL / {2} NOT_RUN" -f $pass, $fail, $notrun)
if ($fail -gt 0) { exit 1 }
exit 0
