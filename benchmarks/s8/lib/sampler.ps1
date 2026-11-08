# Échantillonneur de processus du harnais S8.
#
# Suit les PID fournis par le harnais Node dans `-TrackFile` (JSON :
# [{ "pid": 123, "label": "cargo" }]) et, pour les labels `cargo`/`coordinator`,
# découvre aussi leurs descendants (rustc, binaires de test) via une requête CIM
# filtrée par ParentProcessId. Écrit une ligne JSONL par tick :
#   { t_ms, epoch_ms, ws_sum, proc_count, cpu_total_s }
# S'arrête proprement quand `-StopFile` existe (dernier tick écrit inclus).
#
# NB : ne jamais nommer une variable `$pid` (variable automatique en lecture
# seule) ; on utilise `$procId`.
param(
    [Parameter(Mandatory = $true)][string]$TrackFile,
    [Parameter(Mandatory = $true)][string]$OutFile,
    [Parameter(Mandatory = $true)][string]$StopFile,
    [int]$IntervalMs = 250
)

$ErrorActionPreference = 'SilentlyContinue'
$state = @{}        # procId (int) -> @{ cpu = double|null ; label = string }
$script:exitedCpu = 0.0
$tick = 0
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$writer = [System.IO.StreamWriter]::new($OutFile, $false)
$writer.AutoFlush = $false

function Read-TrackFile {
    $result = @()
    try {
        $text = [System.IO.File]::ReadAllText($TrackFile)
        if (-not [string]::IsNullOrWhiteSpace($text)) {
            foreach ($item in ($text | ConvertFrom-Json)) {
                $result += [pscustomobject]@{ pid = [int]$item.pid; label = [string]$item.label; not_before_ms = [int64]$item.not_before_ms }
            }
        }
    } catch {
        # fichier en cours de réécriture atomique : on garde l'état précédent
    }
    return $result
}

function Get-ProcessIdentity {
    param([int]$procId)
    $process = Get-Process -Id $procId -ErrorAction SilentlyContinue
    if ($null -eq $process) { return $null }
    try {
        return [int64]$process.StartTime.ToUniversalTime().Ticks
    } catch {
        return [int64]0
    }
}

function Test-FreshProcess {
    param([int]$procId, [int64]$notBeforeMs)
    if ($notBeforeMs -le 0) { return $true }
    $process = Get-Process -Id $procId -ErrorAction SilentlyContinue
    if ($null -eq $process) { return $false }
    try {
        $startMs = [DateTimeOffset]$process.StartTime
        # Tolérance 3 s : couvre la latence de spawn Node -> écriture du fichier.
        return ($startMs.ToUnixTimeMilliseconds() -ge ($notBeforeMs - 3000))
    } catch {
        return $true
    }
}

function Write-Sample {
    if ($state.Count -gt 0) {
        $alive = Get-Process -Id @($state.Keys) -ErrorAction SilentlyContinue
    } else {
        $alive = @()
    }
    $aliveIds = @{}
    $ws = [int64]0
    foreach ($process in $alive) {
        if ($null -eq $process) { continue }
        $procId = [int]$process.Id
        # Un PID réutilisé n'est pas le processus suivi : on le traite comme sorti.
        if ($state.ContainsKey($procId) -and $state[$procId].start_ticks -gt 0) {
            $currentTicks = [int64]0
            try { $currentTicks = [int64]$process.StartTime.ToUniversalTime().Ticks } catch { $currentTicks = -1 }
            if ($currentTicks -ne $state[$procId].start_ticks) {
                if ($null -ne $state[$procId].cpu) { $script:exitedCpu += [double]$state[$procId].cpu }
                $state.Remove($procId)
                continue
            }
        }
        $aliveIds[$procId] = $true
        $ws += [int64]$process.WorkingSet64
        $state[$procId].cpu = [double]$process.CPU
    }
    foreach ($procId in @($state.Keys)) {
        if (-not $aliveIds.ContainsKey($procId)) {
            $last = $state[$procId].cpu
            # Portée script obligatoire : sinon `+=` crée une variable locale à la
            # fonction et le CPU cumulé des processus sortis est perdu.
            if ($null -ne $last) { $script:exitedCpu += [double]$last }
            $state.Remove($procId)
        }
    }
    $cpuAlive = 0.0
    $procs = @()
    foreach ($procId in @($state.Keys)) {
        $cpu = $state[$procId].cpu
        if ($null -ne $cpu) { $cpuAlive += [double]$cpu }
        $procProcess = $alive | Where-Object { $null -ne $_ -and [int]$_.Id -eq $procId } | Select-Object -First 1
        if ($null -ne $procProcess) {
            $procs += [pscustomobject]@{
                pid     = $procId
                name    = [string]$procProcess.ProcessName
                label   = [string]$state[$procId].label
                ws_mb   = [Math]::Round([double]$procProcess.WorkingSet64 / 1MB, 1)
                cpu_s   = [Math]::Round([double]$cpu, 3)
            }
        }
    }
    $cpuTotal = [Math]::Round($script:exitedCpu + $cpuAlive, 3)
    $line = [pscustomobject]@{
        t_ms        = [int64]$sw.ElapsedMilliseconds
        epoch_ms    = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        ws_sum      = $ws
        proc_count  = $state.Count
        cpu_total_s = $cpuTotal
        procs       = $procs
    } | ConvertTo-Json -Compress -Depth 4
    $writer.WriteLine($line)
    if (($tick % 4) -eq 0) { $writer.Flush() }
}

try {
    while (-not (Test-Path -LiteralPath $StopFile)) {
        $tick += 1
        foreach ($entry in (Read-TrackFile)) {
            if ($entry.pid -le 0) { continue }
            if (-not $state.ContainsKey($entry.pid)) {
                if (-not (Test-FreshProcess -procId $entry.pid -notBeforeMs $entry.not_before_ms)) { continue }
                $state[$entry.pid] = @{ cpu = $null; label = $entry.label; start_ticks = (Get-ProcessIdentity -procId $entry.pid) }
            }
            else {
                $state[$entry.pid].label = $entry.label
            }
        }

        # Découverte des descendants (rustc, binaires de test) : une seule requête
        # CIM plein-table par tick de découverte, puis fermeture transitive de
        # l'arbre sous les processus suivis de type cargo/coordinator. Plus fiable
        # et moins coûteux que N requêtes filtrées par ParentProcessId (les rustc
        # sont des petits-enfants de cargo et vivent peu).
        $hasCoordinator = $false
        $hasCargo = $false
        foreach ($entry in @($state.Values)) {
            if ($entry.label -match '^coordinator') { $hasCoordinator = $true }
            if ($entry.label -match '^cargo') { $hasCargo = $true }
        }
        $discoveryEvery = if ($hasCoordinator) { 1 } elseif ($hasCargo) { 2 } else { 3 }
        if (($tick % $discoveryEvery) -eq 0) {
            $roots = @{}
            foreach ($procId in @($state.Keys)) {
                if ($state[$procId].label -match '^(cargo|coordinator)') { $roots[$procId] = $true }
            }
            if ($roots.Count -gt 0) {
                $tree = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId)
                $added = @{}
                $changed = $true
                while ($changed) {
                    $changed = $false
                    foreach ($row in $tree) {
                        $childId = [int]$row.ProcessId
                        if ($state.ContainsKey($childId) -or $added.ContainsKey($childId)) { continue }
                        $parentId = [int]$row.ParentProcessId
                        $parentLabel = $null
                        if ($state.ContainsKey($parentId)) { $parentLabel = $state[$parentId].label }
                        elseif ($added.ContainsKey($parentId)) { $parentLabel = $added[$parentId] }
                        if ($null -ne $parentLabel) {
                            $label = if ($parentLabel -match '^(cargo|coordinator)') { "$parentLabel:desc" } else { 'descendant' }
                            $added[$childId] = $label
                            $state[$childId] = @{ cpu = $null; label = $label; start_ticks = (Get-ProcessIdentity -procId $childId) }
                            $changed = $true
                        }
                    }
                }
            }
        }

        Write-Sample
        if ($hasCoordinator) { Start-Sleep -Milliseconds 100 }
        elseif ($hasCargo) { Start-Sleep -Milliseconds 200 }
        else { Start-Sleep -Milliseconds $IntervalMs }
    }
    Write-Sample
} finally {
    $writer.Flush()
    $writer.Dispose()
}
