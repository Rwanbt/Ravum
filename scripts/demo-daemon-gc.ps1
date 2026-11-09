<#
.SYNOPSIS
    Démonstration du GC de rétention borné du daemon d'analyse Ravum (P0-H).

.DESCRIPTION
    1. construit le crate `ravum-daemon` ;
    2. accumule trois contenus réellement analysés (trois `rustc`) ;
    3. exécute le GC one-shot en `--dry-run` (plan non vide, rien supprimé) ;
    4. vérifie qu'une politique invalide sort avec le code 1 ;
    5. exécute le GC réel (plafonds cache/snapshots/état incrémental) ;
    6. redémarre le serveur et vérifie qu'un contenu récent reste un cache hit
       sans `rustc`, tandis qu'un contenu ancien est un miss reconstruit.

    Le script propage le code de sortie : 0 succès, non-zéro échec.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/demo-daemon-gc.ps1
#>
$ErrorActionPreference = 'Stop'

function Fail([string]$message, [int]$code = 1) {
    Write-Host "[demo-daemon-gc] ECHEC: $message" -ForegroundColor Red
    exit $code
}

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repo
try {
    Write-Host '[demo-daemon-gc] build ravum-daemon'
    cargo build -q -p ravum-daemon --offline
    if ($LASTEXITCODE -ne 0) { Fail "cargo build a echoue (exit $LASTEXITCODE)" $LASTEXITCODE }

    $exe = Join-Path $repo 'target\debug\ravum-daemon.exe'
    if (-not (Test-Path $exe)) { Fail "binaire introuvable: $exe" }

    $work = Join-Path $repo 'target\ravum-daemon-gc-demo'
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue

    function New-AnalyzeCommand([string]$id, [int]$revision, [int]$seed) {
        $body = "pub fn demo_$seed(x: f64) -> f64 { x * $($seed + 1).0 + $seed.0 }"
        $command = @{
            id       = $id
            cmd      = 'analyze'
            revision = $revision
            label    = "demo-$seed"
            root     = 'src/lib.rs'
            files    = @(@{ path = 'src/lib.rs'; content = $body })
            context  = @{
                edition    = '2021'
                crate_name = 'gc_demo_unit'
                crate_type = 'lib'
                flags      = @()
            }
        }
        return ($command | ConvertTo-Json -Compress -Depth 6)
    }

    Write-Host '[demo-daemon-gc] 3 analyses reelles via le serveur JSON-lines'
    $server_commands = @(
        (New-AnalyzeCommand 'a1' 1 1),
        (New-AnalyzeCommand 'a2' 2 2),
        (New-AnalyzeCommand 'a3' 3 3),
        '{"id":"s1","cmd":"stats"}',
        '{"id":"x1","cmd":"shutdown"}'
    ) -join "`n"
    $server_output = $server_commands | & $exe --work-dir $work
    if ($LASTEXITCODE -ne 0) { Fail "le serveur a echoue (exit $LASTEXITCODE)" $LASTEXITCODE }
    $events = @($server_output | Where-Object { $_ -and $_.Trim() -ne '' } | ForEach-Object { $_ | ConvertFrom-Json })
    $ready = $events | Where-Object { $_.event -eq 'ready' } | Select-Object -First 1
    if (-not $ready) { Fail "pas de ligne 'ready' du serveur" }
    $analyses = @($events | Where-Object { $_.id -like 'a*' })
    if ($analyses.Count -ne 3) { Fail "3 analyses attendues, recu $($analyses.Count)" }
    if (@($analyses | Where-Object { $_.report.cache.kind -ne 'miss' }).Count -ne 0) {
        Fail 'les trois analyses initiales doivent etre des misses'
    }
    $stats_after_analyses = ($events | Where-Object { $_.id -eq 's1' }).stats
    Write-Host ("[demo-daemon-gc] rustc_runs={0} cache_hits={1} cache_misses={2}" -f `
        $stats_after_analyses.rustc_runs, `
        ($stats_after_analyses.cache_hits_memory + $stats_after_analyses.cache_hits_disk), `
        $stats_after_analyses.cache_misses)

    $before_bytes = (Get-ChildItem $work -Recurse -File -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum).Sum
    if (-not $before_bytes) { $before_bytes = 0 }

    Write-Host '[demo-daemon-gc] GC one-shot --dry-run (plan seulement)'
    $dry_output = & $exe --gc --work-dir $work --dry-run --keep-recent 2 --max-cache-bytes 1 --max-snapshot-bytes 1
    if ($LASTEXITCODE -ne 0) { Fail "GC dry-run a echoue (exit $LASTEXITCODE)" $LASTEXITCODE }
    $dry = $dry_output | ConvertFrom-Json
    if ($dry.kind -ne 'ravum-daemon-gc-report') { Fail "rapport GC inattendu: $($dry.kind)" }
    if ($dry.dry_run -ne $true) { Fail 'dry_run attendu vrai' }
    $planned = $dry.planned.cache_keys.Count + $dry.planned.snapshots.Count + $dry.planned.contexts.Count
    if ($planned -lt 1) { Fail 'dry-run doit planifier au moins une victime' }
    $mid_bytes = (Get-ChildItem $work -Recurse -File | Measure-Object -Property Length -Sum).Sum
    if ($mid_bytes -ne $before_bytes) { Fail 'dry-run a modifie le disque' }
    Write-Host ("[demo-daemon-gc] dry-run: victimes planifiees={0} planned_freed={1} B freed={2} B (disque inchange)" -f `
        $planned, $dry.planned_freed_bytes, $dry.freed_bytes)

    Write-Host '[demo-daemon-gc] politique invalide -> exit 1 attendu'
    $previous_eap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $invalid_output = & $exe --gc --work-dir $work --keep-recent 0 2>&1
    $invalid_code = $LASTEXITCODE
    $ErrorActionPreference = $previous_eap
    if ($invalid_code -ne 1) { Fail "keep_recent=0 doit sortir avec 1 (recu $invalid_code)" }
    if (-not (($invalid_output -join "`n") -match 'keep_recent')) {
        Fail 'le message d''erreur doit expliquer la politique invalide'
    }

    Write-Host '[demo-daemon-gc] GC one-shot reel'
    $real_output = & $exe --gc --work-dir $work --keep-recent 2 --max-cache-bytes 1 --max-snapshot-bytes 1 --max-incremental-bytes 1
    if ($LASTEXITCODE -ne 0) { Fail "GC reel a echoue (exit $LASTEXITCODE)" $LASTEXITCODE }
    $real = $real_output | ConvertFrom-Json
    if ($real.dry_run -ne $false) { Fail 'dry_run attendu faux pour le GC reel' }
    $after_bytes = (Get-ChildItem $work -Recurse -File | Measure-Object -Property Length -Sum).Sum
    Write-Host ("[demo-daemon-gc] GC reel: cache {0}->{1} entrees, snapshots {2}->{3}, duree={4} ms, freed={5} B, disque {6} -> {7} octets" -f `
        $real.before.cache_entries, $real.after.cache_entries, `
        $real.before.snapshots, $real.after.snapshots, `
        $real.duration_ms, $real.freed_bytes, $before_bytes, $after_bytes)
    if ($real.freed_bytes -lt 1) { Fail 'le GC reel doit liberer des octets' }

    Write-Host '[demo-daemon-gc] redemarrage du serveur: recent = hit, ancien = miss reconstruit'
    $replay_commands = @(
        (New-AnalyzeCommand 'r3' 10 3),
        (New-AnalyzeCommand 'r1' 11 1),
        '{"id":"s2","cmd":"stats"}',
        '{"id":"x2","cmd":"shutdown"}'
    ) -join "`n"
    $replay_output = $replay_commands | & $exe --work-dir $work
    if ($LASTEXITCODE -ne 0) { Fail "le serveur de replay a echoue (exit $LASTEXITCODE)" $LASTEXITCODE }
    $replay_events = @($replay_output | Where-Object { $_ -and $_.Trim() -ne '' } | ForEach-Object { $_ | ConvertFrom-Json })
    $recent = ($replay_events | Where-Object { $_.id -eq 'r3' }).report
    $old = ($replay_events | Where-Object { $_.id -eq 'r1' }).report
    $replay_stats = ($replay_events | Where-Object { $_.id -eq 's2' }).stats
    if (-not $recent -or -not $old) { Fail 'reponses de replay manquantes' }
    if ($recent.cache.kind -eq 'miss') { Fail 'le contenu recent doit rester un cache hit' }
    if ($recent.timings.analysis_ms -ne 0) { Fail 'un cache hit ne doit pas lancer rustc' }
    if ($old.cache.kind -ne 'miss') { Fail 'le contenu collecte doit etre un miss' }
    if (-not $old.success) { Fail 'le contenu collecte doit etre reconstruit avec succes' }
    Write-Host ("[demo-daemon-gc] recent: cache={0} analysis_ms={1} | ancien: cache={2} analysis_ms={3} | rustc_runs={4}" -f `
        $recent.cache.kind, $recent.timings.analysis_ms, `
        $old.cache.kind, $old.timings.analysis_ms, $replay_stats.rustc_runs)

    Write-Host '[demo-daemon-gc] OK'
    exit 0
}
finally {
    Pop-Location
}
