# build-flags.ps1 — mesure reproductible du coût codegen+link d'une unité de
# patch `cdylib` selon les options rustc, sans modifier le produit.
#
# Objectif (MVP-0b, reste à faire « réduire build+link à froid ») : mesurer, pas
# supposer. Le script construit exactement la même source avec plusieurs jeux
# d'options `-C`, en les ENTRELACANT à chaque tour (l'ordre tourne) pour que la
# dérive machine affecte toutes les variantes de la même façon. Il ne prouve
# rien sur la sémantique du produit : `opt-level=0` et `codegen-units` changent
# le code généré, donc leur adoption exigerait une validation DSP séparée.
#
# Usage :
#   pwsh -File scripts/build-flags.ps1                       # petit fixture
#   pwsh -File scripts/build-flags.ps1 -AppendFunctions 400 -Rounds 8 -Out evidence/mvp0b-build-flags-large.json
#
# Déterministe : aucun réseau, aucun état rustc incrémental, artefacts dans
# target/build-flags-exp (ignoré par Git).

param(
    [int]$Rounds = 12,
    [int]$AppendFunctions = 8,
    [string]$Out = 'evidence/mvp0b-build-flags.json',
    [string]$WorkDir = 'target/build-flags-exp'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$rustc = $env:RAVUM_RUSTC
if (-not $rustc) { $rustc = 'rustc' }

$work = Join-Path $root $WorkDir
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory -Path $work -Force | Out-Null

# Même fixture que `ravum cycle-bench`, plus des fonctions ajoutées comme le
# fait chaque édition mesurée (une édition = du code neuf à analyser/builder).
$basePath = Join-Path $root 'fixtures/calculator/v1.rs'
$source = Get-Content $basePath -Raw
$builder = [System.Text.StringBuilder]::new($source)
for ($i = 1; $i -le $AppendFunctions; $i++) {
    [void]$builder.AppendLine('')
    [void]$builder.AppendLine('#[no_mangle]')
    [void]$builder.AppendLine("pub extern `"C`" fn edit_fn_$i(x: f64) -> f64 {")
    [void]$builder.AppendLine("    x * $([string]$i).0 + 1.0")
    [void]$builder.AppendLine('}')
}
$srcPath = Join-Path $work 'unit.rs'
Set-Content -Path $srcPath -Value $builder.ToString() -Encoding UTF8
$sourceBytes = (Get-Item $srcPath).Length
$sourceLines = (Get-Content $srcPath | Measure-Object -Line).Lines

# Variantes : [0] = jeu d'options réellement utilisé par ravum-rust-adapter.
$variants = [ordered]@{
    'baseline'   = @('-C', 'opt-level=1', '-C', 'debuginfo=0', '-C', 'panic=abort')
    # Contrôle : options identiques à `baseline`, pour mesurer le plancher de
    # bruit apparié au même tour (si baseline-b bat baseline, l'écart est du bruit).
    'baseline-b' = @('-C', 'opt-level=1', '-C', 'debuginfo=0', '-C', 'panic=abort')
    'cgu1'       = @('-C', 'opt-level=1', '-C', 'debuginfo=0', '-C', 'panic=abort', '-C', 'codegen-units=1')
    'cgu64'      = @('-C', 'opt-level=1', '-C', 'debuginfo=0', '-C', 'panic=abort', '-C', 'codegen-units=64')
    'cgu256'     = @('-C', 'opt-level=1', '-C', 'debuginfo=0', '-C', 'panic=abort', '-C', 'codegen-units=256')
    'embed-no'   = @('-C', 'opt-level=1', '-C', 'debuginfo=0', '-C', 'panic=abort', '-C', 'embed-bitcode=no')
    'strip'      = @('-C', 'opt-level=1', '-C', 'debuginfo=0', '-C', 'panic=abort', '-C', 'strip=symbols')
    'cgu1-strip' = @('-C', 'opt-level=1', '-C', 'debuginfo=0', '-C', 'panic=abort', '-C', 'codegen-units=1', '-C', 'strip=symbols')
    # Changement de sémantique de codegen : mesuré pour information uniquement.
    'opt0'       = @('-C', 'opt-level=0', '-C', 'debuginfo=0', '-C', 'panic=abort')
}
$names = @($variants.Keys)

function Get-Stats {
    param([double[]]$Values)
    if ($Values.Count -eq 0) { return $null }
    $sorted = @($Values | Sort-Object)
    $n = $sorted.Count
    if ($n % 2 -eq 1) {
        $median = $sorted[[int][math]::Floor($n / 2)]
    } else {
        $median = ($sorted[$n / 2 - 1] + $sorted[$n / 2]) / 2
    }
    $p95Index = [math]::Min($n - 1, [math]::Max(0, [int][math]::Ceiling(0.95 * $n) - 1))
    $mean = ($sorted | Measure-Object -Average).Average
    [pscustomobject]@{
        count = $n
        min   = [math]::Round($sorted[0], 1)
        median = [math]::Round($median, 1)
        p95   = [math]::Round($sorted[$p95Index], 1)
        max   = [math]::Round($sorted[$n - 1], 1)
        mean  = [math]::Round($mean, 1)
    }
}

# Calibration : coût d'un simple spawn `rustc -vV` (plancher machine, pas le
# coût d'analyse). Sert à interpréter les écarts entre variantes.
$calibration = @()
for ($i = 0; $i -lt 5; $i++) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & $rustc -vV | Out-Null
    $sw.Stop()
    $calibration += $sw.Elapsed.TotalMilliseconds
}
$toolchain = @(& $rustc -vV 2>&1)

$raw = [System.Collections.Generic.List[object]]::new()
$failed = 0
for ($round = 0; $round -lt $Rounds; $round++) {
    for ($k = 0; $k -lt $names.Count; $k++) {
        $name = $names[($k + $round) % $names.Count]
        $artifact = Join-Path $work ("{0}-round{1}.dll" -f $name, $round)
        $argv = @('--edition', '2021', '--crate-name', 'ravum_patch', '--crate-type', 'cdylib', '--error-format=json') + $variants[$name] + @('-o', $artifact, $srcPath)
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $stderr = & $rustc @argv 2>&1
        $code = $LASTEXITCODE
        $sw.Stop()
        $bytes = 0
        if (Test-Path $artifact) { $bytes = (Get-Item $artifact).Length }
        if ($code -ne 0) {
            $failed++
            Write-Host ("ECHEC {0} (tour {1}, code {2})" -f $name, $round, $code)
            $stderr | Select-Object -Last 8 | ForEach-Object { Write-Host "  $_" }
        }
        $raw.Add([pscustomobject]@{
                variant        = $name
                round          = $round
                ms             = [math]::Round($sw.Elapsed.TotalMilliseconds, 1)
                exit_code      = $code
                artifact_bytes = $bytes
            })
    }
}

$summary = [ordered]@{}
foreach ($name in $names) {
    $values = @($raw | Where-Object { $_.variant -eq $name -and $_.exit_code -eq 0 } | ForEach-Object { [double]$_.ms })
    $artifactBytes = @($raw | Where-Object { $_.variant -eq $name } | ForEach-Object { $_.artifact_bytes })
    $summary[$name] = [pscustomobject]@{
        timing         = Get-Stats $values
        artifact_bytes = if (@($artifactBytes).Count) { (@($artifactBytes) | Measure-Object -Maximum).Maximum } else { 0 }
    }
}

$cpuName = (Get-ItemProperty 'HKLM:\HARDWARE\DESCRIPTION\System\CentralProcessor\0' -ErrorAction SilentlyContinue).ProcessorNameString
$report = [ordered]@{
    schema           = 'ravum-build-flags/1'
    generated_at     = (Get-Date).ToString('o')
    generated_by     = 'scripts/build-flags.ps1'
    machine          = [ordered]@{
        computer   = $env:COMPUTERNAME
        os         = [System.Environment]::OSVersion.VersionString
        cpu        = $cpuName
        processors = [System.Environment]::ProcessorCount
    }
    rustc            = [ordered]@{
        command     = $rustc
        version_line = if (@($toolchain).Count) { $toolchain[0] } else { 'inconnu' }
        host_line   = if (@($toolchain).Count -gt 1) { $toolchain[1] } else { 'inconnu' }
    }
    experiment       = [ordered]@{
        fixture                = 'fixtures/calculator/v1.rs + fonctions ajoutées (édition simulée)'
        appended_functions     = $AppendFunctions
        source_bytes           = $sourceBytes
        source_lines           = $sourceLines
        rounds                 = $Rounds
        variants               = [ordered]@{}
        ordering               = 'entrelacé, ordre décalé à chaque tour (dérive machine annulée)'
        incremental_state      = 'aucun : chaque invocation est un build à froid réel'
        artifact_cache         = 'aucun (rustc direct, pas de cache ravum)'
    }
    notes            = @(
        'les temps incluent le spawn de rustc ; comparer les variantes entre elles, pas au produits',
        'opt-level=0 et codegen-units changent le code généré : adoption = décision, pas appliqué ici',
        'machine partagée avec d''autres agents : la calibration donne le plancher de spawn, pas la charge',
        'aucune conclusion sur la qualité du code produit ni sur le temps réel'
    )
    calibration_ms   = Get-Stats $calibration
    failures         = $failed
    summary          = $summary
    raw              = $raw
}
foreach ($name in $names) { $report.experiment.variants[$name] = @($variants[$name]) }

$outPath = Join-Path $root $Out
$json = $report | ConvertTo-Json -Depth 10
Set-Content -Path $outPath -Value $json -Encoding UTF8

Write-Host ("Rapport: {0} ({1} octets)" -f $outPath, (Get-Item $outPath).Length)
Write-Host ("Source: {0} octets, {1} lignes, {2} fonctions ajoutées, {3} tours" -f $sourceBytes, $sourceLines, $AppendFunctions, $Rounds)
Write-Host ("Calibration spawn rustc -vV (ms): median={0} min={1} p95={2}" -f $report.calibration_ms.median, $report.calibration_ms.min, $report.calibration_ms.p95)
$baseline = $report.summary['baseline'].timing.median
foreach ($name in $names) {
    $s = $report.summary[$name].timing
    $delta = if ($baseline -gt 0) { [math]::Round((($s.median - $baseline) / $baseline) * 100, 1) } else { 0 }
    Write-Host ("{0,-11} median={1,7} ms min={2,7} p95={3,7} delta={4,6}% artifact={5} o" -f $name, $s.median, $s.min, $s.p95, $delta, $report.summary[$name].artifact_bytes)
}
if ($failed -ne 0) {
    Write-Host ("ECHEC: {0} invocation(s) rustc en erreur" -f $failed)
    exit 1
}
exit 0
