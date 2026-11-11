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
    param([string]$Case, [string]$Dir, [string[]]$CmdArgs, [string]$ExpectStdout = "", [int]$ExpectExit = 0, [int]$TimeoutSec = 120, [string]$Stdin = "")
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ravum-smoke-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        $work = if ($Dir) { $Dir } else { $tmp }
        $outFile = Join-Path $tmp "stdout.txt"
        $errFile = Join-Path $tmp "stderr.txt"
        $stdinArgs = @{}
        if ($Stdin) {
            $stdinFile = Join-Path $tmp "stdin.txt"
            Set-Content -Path $stdinFile -Value $Stdin -NoNewline
            $stdinArgs = @{ RedirectStandardInput = $stdinFile }
        }
        $p = Start-Process -FilePath $binPath -ArgumentList $CmdArgs -WorkingDirectory $work -Wait -PassThru -NoNewWindow -RedirectStandardOutput $outFile -RedirectStandardError $errFile @stdinArgs
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

# --- file-store : verticale P2 adossée à des fichiers (pas une base de données) ---
# Le répertoire du magasin doit exister : Core 1.0 n'a pas de `create_dir`.
# Il est créé sous `examples/file-store/.ravum/` parce que le binaire tourne
# depuis le `.ravum/bin` du paquet : sur cette machine de développement, un
# processus lancé depuis un paquet ne peut écrire que dans ce paquet (confinement
# de l'outillage, pas une règle Ravum — le même binaire copié sous `target/`
# écrit ailleurs). Un répertoire temporaire hors paquet ferait donc échouer le
# cas pour une raison d'environnement, pas pour un défaut du programme.
Run-Example "file-store" "usage"
$storeRoot = Join-Path $root "examples\file-store\.ravum\smoke-store"
Remove-Item -Recurse -Force $storeRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $storeRoot | Out-Null
try {
    $storeDir = Join-Path $root ("examples\file-store")
    Invoke-Case "run:file-store-put-v1" $storeDir @("run", "--", $storeRoot, "put", "greeting") "stored greeting v1" 0 -Stdin "hello world" | Out-Null
    Invoke-Case "run:file-store-get-v1" $storeDir @("run", "--", $storeRoot, "get", "greeting") "hello world" 0 | Out-Null
    Invoke-Case "run:file-store-put-v2" $storeDir @("run", "--", $storeRoot, "put", "greeting") "stored greeting v2" 0 -Stdin "hello again" | Out-Null
    # Nouveau processus : la valeur doit survivre au redémarrage.
    Invoke-Case "run:file-store-get-v2" $storeDir @("run", "--", $storeRoot, "get", "greeting") "hello again" 0 | Out-Null
    Invoke-Case "run:file-store-list" $storeDir @("run", "--", $storeRoot, "list") "greeting" 0 | Out-Null
    Invoke-Case "run:file-store-history" $storeDir @("run", "--", $storeRoot, "history", "greeting") "v2" 0 | Out-Null
    Invoke-Case "run:file-store-missing-key" $storeDir @("run", "--", $storeRoot, "get", "ghost") "" 1 | Out-Null
    Invoke-Case "run:file-store-traversal" $storeDir @("run", "--", $storeRoot, "get", "../etc/passwd") "" 1 | Out-Null
    if ((Test-Path (Join-Path $storeRoot "greeting.v2")) -and (Test-Path (Join-Path $storeRoot "greeting.v1"))) {
        Record "run:file-store-versions" "PASS" ""
    } else {
        Record "run:file-store-versions" "FAIL" "les fichiers de version v1/v2 sont absents"
    }
} finally {
    Remove-Item -Recurse -Force $storeRoot -ErrorAction SilentlyContinue
}

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
