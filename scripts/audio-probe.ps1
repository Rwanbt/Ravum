<#
.SYNOPSIS
    Sonde audio reelle MVP-0b / L4-C : chemin DSP Ravum execute sur un peripherique
    waveOut Windows, en n'emettant que du silence.

.DESCRIPTION
    Compile deux versions de la fixture DSP (`fixtures/dsp/iir_v1.rs` et
    `fixtures/dsp/iir_v2.rs`) avec `ravum build`, puis lance `ravum-audio-probe` :
    le binaire ouvre le peripherique waveOut par defaut (ou -Device), fait tourner
    des tampons PCM 16 bits stereo et appelle le vrai chemin `BlockHost::process_rt`
    a chaque fin de tampon, avec remplacements a chaud et crossfade.

    Important : le signal DSP (sinusoide mono) est calcule, mesure et jete. Les tampons
    soumis au pilote sont toujours remplis de zeros : aucun son n'est emis.

    Le script propage le code de sortie de la sonde et affiche un resume. Si la cadence
    de tampons reellement livree par le pilote differe de la cadence attendue, c'est
    affiche comme un avertissement, pas cache.

    Options de qualification temps reel : -Mmcss (enregistrement MMCSS "Pro Audio" +
    AVRT_PRIORITY_CRITICAL sur le thread de rendu) et -ThreadPriority (priorite Win32
    brute). -Load lance N processus PowerShell en boucle occupee autour de la sonde
    (charge controlee, bornee, arretee en fin de run) pour mesurer l'effet reel de ces
    priorites sous charge. MMCSS n'est pas une garantie hard-RT et le JSON le dit.

    Prerequis : Windows, PowerShell 5.1+, toolchain Rust, depot Ravum courant.

.PARAMETER Root
    Racine du depot. Par defaut le parent du dossier scripts/.

.PARAMETER Seconds
    Duree de la mesure, 1 a 10 secondes (defaut 3).

.PARAMETER Frames
    Frames par tampon (defaut 512 = 10,67 ms a 48 kHz). 256 frames (5,33 ms) est la
    valeur par defaut du binaire, mais sur certaines interfaces (MOTU M Series, WDM)
    cette taille tombe sous le quantum de service du pilote : la cadence livree
    s'effondre et la file se vide. 512 frames est la plus petite taille mesuree qui
    tient le temps reel sur le peripherique par defaut de cette machine.

.PARAMETER Buffers
    Nombre de tampons en rotation (defaut 4).

.PARAMETER SwapEveryMs
    Intervalle entre deux remplacements a chaud, 0 pour les desactiver (defaut 250).

.PARAMETER CrossfadeFrames
    Longueur du crossfade arme pour les remplacements (defaut 256 = 5,33 ms).

.PARAMETER Device
    Index waveOut explicite. -1 (defaut) = WAVE_MAPPER, le peripherique par defaut.
    Utiliser -ListDevices pour voir les index disponibles.

.PARAMETER Out
    Chemin du JSON de preuve. Defaut : evidence/mvp0b-audio-probe-<date>.json, ou
    evidence/mvp0b-audio-mmsc-<Variant>-<date>.json si -Variant est fourni.

.PARAMETER Mmcss
    Demande au thread de rendu de s'enregistrer aupres de MMCSS Windows ("Pro Audio")
    et de demander AVRT_PRIORITY_CRITICAL (avrt.dll), puis de restaurer en fin de run.
    Un echec est explicite (code Win32) et la sonde sort en erreur : jamais de faux
    succes. MMCSS n'est PAS une garantie hard-RT.

.PARAMETER ThreadPriority
    Priorite Win32 brute du thread de rendu : normal | above | high | time-critical
    (SetThreadPriority, appliquee APRES MMCSS si les deux sont demandes).

.PARAMETER Load
    Nombre de processus PowerShell en boucle occupee lances autour de la sonde pour
    creer une charge CPU controlee (0 = aucune charge, defaut). La charge est arretee
    en fin de run, quoi qu'il arrive. La charge mesuree est dans le JSON.

.PARAMETER LoadSeconds
    Duree maximale de la boucle de chaque processus de charge, en secondes (defaut 10,
    borne volontairement courte pour ne pas bloquer les autres agents).

.PARAMETER Variant
    Nom de variante pour le fichier de preuve par defaut
    (evidence/mvp0b-audio-mmsc-<Variant>-<date>.json).

.PARAMETER ListDevices
    Affiche les peripheriques waveOut vus par la sonde et sort.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1
    powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Frames 256 -Device 1
    powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Mmcss -Variant rest-mmcss-r1
    powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Load 12 -Variant load-nommscss-r1
    powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Mmcss -Load 12 -Variant load-mmcss-r1
#>
[CmdletBinding()]
param(
    [string]$Root = '',
    [ValidateRange(1, 10)]
    [int]$Seconds = 3,
    [ValidateRange(32, 4096)]
    [int]$Frames = 512,
    [ValidateRange(1, 16)]
    [int]$Buffers = 4,
    [int]$SwapEveryMs = 250,
    [int]$CrossfadeFrames = 256,
    [int]$Device = -1,
    [switch]$Mmcss,
    [ValidateSet('', 'normal', 'above', 'high', 'time-critical')]
    [string]$ThreadPriority = '',
    [ValidateRange(0, 64)]
    [int]$Load = 0,
    [ValidateRange(2, 60)]
    [int]$LoadSeconds = 10,
    [string]$Variant = '',
    [string]$Out = '',
    [switch]$ListDevices
)

$ErrorActionPreference = 'Stop'
$loadProcesses = @()
if (-not $Root) { $Root = Split-Path -Parent $PSScriptRoot }
if (-not $Root) { $Root = (Get-Location).Path }
Push-Location $Root
$exitCode = 0
try {
    $workDir = Join-Path $Root 'target\ravum\audio-probe'
    New-Item -ItemType Directory -Force -Path $workDir | Out-Null

    function Invoke-RavumBuild {
        param([string]$Fixture, [string]$JsonPath, [string]$ErrorPath)
        Write-Host "[build] $Fixture"
        $raw = & cargo run -q --bin ravum -- build $Fixture --json 2>$ErrorPath
        $buildExit = $LASTEXITCODE
        if ($buildExit -ne 0) {
            $details = ''
            if (Test-Path $ErrorPath) { $details = Get-Content $ErrorPath -Raw }
            throw "ravum build a echoue pour $Fixture (code $buildExit) : $details"
        }
        $text = ($raw -join "`n").Trim()
        if (-not $text) { throw "ravum build n'a rien ecrit sur stdout pour $Fixture" }
        # UTF-8 without BOM: the probe parses this file as JSON.
        [System.IO.File]::WriteAllText($JsonPath, $text)
        $build = $text | ConvertFrom-Json
        if (-not $build.artifact_path) { throw "reponse de build sans artifact_path pour $Fixture" }
        if (-not (Test-Path $build.artifact_path)) {
            throw "artefact introuvable apres build : $($build.artifact_path)"
        }
        $short = $build.artifact_id.Substring(0, [Math]::Min(12, $build.artifact_id.Length))
        Write-Host ("[build] artifact {0} (id {1}...)" -f (Split-Path -Leaf $build.artifact_path), $short)
        return $build
    }

    if (-not $Out) {
        $date = Get-Date -Format 'yyyy-MM-dd'
        if ($Variant) {
            $Out = Join-Path $Root "evidence/mvp0b-audio-mmsc-$Variant-$date.json"
        } else {
            $Out = Join-Path $Root "evidence/mvp0b-audio-probe-$date.json"
        }
    } elseif (-not [System.IO.Path]::IsPathRooted($Out)) {
        $Out = Join-Path $Root $Out
    }
    $outDir = Split-Path -Parent $Out
    if ($outDir) { New-Item -ItemType Directory -Force -Path $outDir | Out-Null }

    $v1Json = Join-Path $workDir 'build-iir_v1.json'
    $v2Json = Join-Path $workDir 'build-iir_v2.json'
    $v1 = Invoke-RavumBuild -Fixture 'fixtures/dsp/iir_v1.rs' -JsonPath $v1Json -ErrorPath (Join-Path $workDir 'build-iir_v1.err')
    $v2 = Invoke-RavumBuild -Fixture 'fixtures/dsp/iir_v2.rs' -JsonPath $v2Json -ErrorPath (Join-Path $workDir 'build-iir_v2.err')

    if ($ListDevices) {
        $devOut = Join-Path $workDir 'devices.json'
        $devErr = Join-Path $workDir 'devices.err'
        & cargo run -q -p ravum-dsp-harness --bin ravum-audio-probe -- --seconds 1 --frames 512 --json 2>$devErr |
            Out-File -Encoding UTF8 $devOut
        if ($LASTEXITCODE -ne 0) { throw "la sonde a echoue (code $LASTEXITCODE) : $(Get-Content $devErr -Raw)" }
        $devices = (Get-Content $devOut -Raw | ConvertFrom-Json).devices.devices
        Write-Host 'Peripheriques waveOut :'
        foreach ($entry in $devices) {
            Write-Host ("  [{0}] {1} - {2} canaux, formats {3}" -f $entry.resolved_device_id, $entry.name, $entry.channels, $entry.formats_mask)
        }
        exit 0
    }

    $probeOut = Join-Path $workDir 'probe.out'
    $probeErr = Join-Path $workDir 'probe.err'
    $probeArgs = @(
        'run', '-q', '-p', 'ravum-dsp-harness', '--bin', 'ravum-audio-probe', '--',
        '--seconds', "$Seconds",
        '--buffers', "$Buffers",
        '--frames', "$Frames",
        '--module', $v1.artifact_path,
        '--swap-build', $v2Json,
        '--swap-build', $v1Json,
        '--swap-every-ms', "$SwapEveryMs",
        '--crossfade-frames', "$CrossfadeFrames",
        '--json',
        '--out', $Out
    )
    if ($Device -ge 0) { $probeArgs += @('--device', "$Device") }
    if ($Mmcss) { $probeArgs += '--mmcss' }
    if ($ThreadPriority) { $probeArgs += @('--thread-priority', $ThreadPriority) }

    if ($Load -gt 0) {
        Write-Host ("[charge] demarrage de {0} processus PowerShell en boucle occupee (limite {1} s)" -f $Load, $LoadSeconds)
        $loop = '$e=(Get-Date).AddSeconds(' + $LoadSeconds + '); while((Get-Date) -lt $e) { $x=1 }'
        for ($i = 0; $i -lt $Load; $i++) {
            $loadProcesses += Start-Process -FilePath 'powershell' `
                -ArgumentList @('-NoProfile', '-Command', $loop) -PassThru -WindowStyle Hidden
        }
        Start-Sleep -Milliseconds 900
        $env:RAVUM_PROBE_LOAD_NOTE = "controlled load: $Load PowerShell busy-loop processes for up to $LoadSeconds s, started by scripts/audio-probe.ps1 -Load; the measured CPU load is in load.cpu_busy_percent_measured"
        Write-Host ("[charge] note transmise a la sonde via RAVUM_PROBE_LOAD_NOTE")
    }

    Write-Host ("[probe] {0} s, {1} frames/tampon, {2} tampons, swap={3} ms, crossfade={4} frames, mmcss={5}, thread-priority={6}" -f `
        $Seconds, $Frames, $Buffers, $SwapEveryMs, $CrossfadeFrames, [bool]$Mmcss, $ThreadPriority)
    Write-Host '[probe] le peripherique ne recoit que du silence ; le signal DSP est mesure puis jete'
    & cargo @probeArgs 2>$probeErr | Out-File -Encoding UTF8 $probeOut
    $probeExit = $LASTEXITCODE

    if ($probeExit -ne 0) {
        Write-Host "La sonde a echoue (code $probeExit)."
        if (Test-Path $probeErr) {
            $errText = Get-Content $probeErr -Raw
            if ($errText) { Write-Host $errText }
        }
        if (Test-Path $Out) {
            $failed = Get-Content $Out -Raw | ConvertFrom-Json
            if ($failed.error) {
                Write-Host ("  etape   : {0}" -f $failed.error.stage)
                Write-Host ("  code    : {0} {1}" -f $failed.error.code, $failed.error.code_name)
                Write-Host ("  message : {0}" -f $failed.error.message)
            }
        }
        $exitCode = $probeExit
        exit $probeExit
    }

    if (-not (Test-Path $Out)) { throw "la sonde n'a pas ecrit la preuve JSON : $Out" }
    $report = Get-Content $Out -Raw | ConvertFrom-Json
    $m = $report.measurements

    Write-Host ''
    Write-Host '--- resultat mesure ---'
    Write-Host ("  statut        : {0}" -f $report.status)
    Write-Host ("  peripherique  : {0} (waveOut id {1})" -f $report.device.name, $report.device.resolved_device_id)
    Write-Host ("  format        : {0} {1} canaux {2} Hz, {3} frames/tampon, {4} tampons (periode {5} us)" -f `
        $report.format.sample_format, $report.format.channels, $report.format.sample_rate, `
        $report.format.frames_per_buffer, $report.format.buffers_in_rotation, $m.period_us)
    Write-Host ("  tampons       : {0} traites, cadence livree {1} tampons/s (attendue {2} tampons/s, soit {3} ech/s)" -f `
        $m.blocks_processed, [Math]::Round($m.delivered_rate_hz, 1), [Math]::Round($m.expected_completion_rate_hz, 1), [Math]::Round($m.delivered_sample_rate_hz, 0))
    Write-Host ("  temps reel    : ratio {0:0.00} (tenu : {1})" -f $report.realtime_cadence.ratio, $report.realtime_cadence.held_real_time)
    $rt = $report.realtime
    Write-Host ("  priorite      : MMCSS demande={0} acquis={1} (tache {2}, priorite {3}) ; SetThreadPriority demande={4} pose={5} ; priorite de base observee={6}" -f `
        $rt.mmcss_requested, $rt.mmcss_acquired, $rt.mmcss_task_index, $rt.mmcss_priority, `
        $rt.thread_priority_requested, $rt.thread_priority_set, $rt.thread_priority_observed)
    if ($rt.revert) {
        Write-Host ("  restauration  : MMCSS revert appele={0} ok={1} (priorite apres={2})" -f `
            $rt.revert.called, $rt.revert.ok, $rt.revert.thread_priority_after_revert)
    }
    if ($rt.notes) {
        foreach ($note in $rt.notes) { Write-Host ("               note: {0}" -f $note) }
    }
    $ld = $report.load
    Write-Host ("  charge        : CPU occupe {0:0.0} % mesure sur {1} ms ; RAM libre {2} Mio / {3} Mio" -f `
        [double]$ld.cpu_busy_percent_measured, [int]$ld.cpu_sample_window_ms, `
        ([math]::Round([double]$ld.ram_available_bytes_start / 1MB)), ([math]::Round([double]$ld.ram_total_bytes / 1MB)))
    if ($ld.probe_load_note) { Write-Host ("               note: {0}" -f $ld.probe_load_note) }
    Write-Host ("  work DSP      : p50 {0} us, p95 {1} us, p99 {2} us, max {3} us ({4} mesures)" -f `
        $m.work_us.p50, $m.work_us.p95, $m.work_us.p99, $m.work_us.max, $m.work_us.count)
    Write-Host ("  callback      : p50 {0} us, p95 {1} us, max {2} us" -f `
        $m.callback_total_us.p50, $m.callback_total_us.p95, $m.callback_total_us.max)
    Write-Host ("  lateness      : p50 {0} us, p95 {1} us, p99 {2} us, max {3} us" -f `
        $m.wake_lateness_us.p50, $m.wake_lateness_us.p95, $m.wake_lateness_us.p99, $m.wake_lateness_us.max)
    Write-Host ("  intervalles   : p50 {0} us, p95 {1} us, max {2} us" -f `
        $m.interval_us.p50, $m.interval_us.p95, $m.interval_us.max)
    Write-Host ("  marge de file : min {0} us, p50 {1} us (risques de vidage : {2})" -f `
        $m.queue_margin_us.min, $m.queue_margin_us.p50, $m.queue_drain_risk_events)
    Write-Host ("  en retard     : {0} tampons > 1 periode ({1} reveils) ; timeouts {2} ; tampons groupes {3}" -f `
        $m.buffers_late_gt_one_period, $m.wakes_late_gt_one_period, $m.wait_timeouts, $m.coalesced_completions)
    Write-Host ("  allocations   : {0} sur le thread de rendu, {1} desallocations" -f `
        $m.allocations_on_render_thread, $m.deallocations_on_render_thread)
    Write-Host ("  swaps         : {0} demandes, {1} actives, {2} refuses, {3} erreurs" -f `
        $report.swaps.requested, $report.swaps.activations, $report.swaps.rejections, $report.swaps.errors)
    if ($report.correlation) {
        Write-Host ("  delta |x[n]-x[n-1]| : max aux swaps {0:0.0000}, hors swap {1:0.0000}, reference sinusoidale {2:0.0000}" -f `
            $report.correlation.max_sample_delta_at_swaps, $report.correlation.max_sample_delta_steady, $report.dsp.baseline_sample_delta)
    }
    Write-Host ("  preuve        : {0}" -f $Out)

    $delivered = [double]$m.delivered_rate_hz
    $expected = [double]$m.expected_completion_rate_hz
    if ($expected -gt 0 -and $delivered -lt (0.9 * $expected)) {
        Write-Host ''
        Write-Warning ("cadence livree {0:0.0} Hz < 90% de la cadence attendue {1:0.0} Hz : le pilote ne tient pas le temps reel avec cette taille de tampon, la file se vide. C'est un resultat, pas une erreur du script." -f $delivered, $expected)
    }
    if ($m.queue_drain_risk_events -gt 0) {
        Write-Warning ("{0} evenements avec marge de file negative : risque de starvation (pas un XRUN mesure, waveOut ne l'expose pas)." -f $m.queue_drain_risk_events)
    }
    if ($m.allocations_on_render_thread -gt 0 -or $m.deallocations_on_render_thread -gt 0) {
        Write-Warning 'le thread de rendu a alloue ou libere de la memoire : violation du contrat audio.'
        $exitCode = 1
    }
} catch {
    Write-Host ("ERREUR: {0}" -f $_.Exception.Message)
    $exitCode = 1
} finally {
    if ($loadProcesses -and $loadProcesses.Count -gt 0) {
        foreach ($loadProcess in $loadProcesses) {
            try { Stop-Process -Id $loadProcess.Id -Force -ErrorAction SilentlyContinue } catch { }
        }
        Write-Host ("[charge] {0} processus de charge arretes" -f $loadProcesses.Count)
        $loadProcesses = @()
    }
    $env:RAVUM_PROBE_LOAD_NOTE = $null
    Pop-Location
}
exit $exitCode
