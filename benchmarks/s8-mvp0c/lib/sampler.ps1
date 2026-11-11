# Échantillonneur de processus DIRECTS du harnais S8 MVP-0c.
#
# CONTRAINTE DE LA MACHINE : les requêtes CIM/WMI (Get-CimInstance Win32_*) se
# bloquent ou échouent par intermittence sur cette machine partagée. Cet
# échantillonneur n'utilise donc QUE `Get-Process -Id` (System.Diagnostics) :
# il suit les PID des processus directement lancés par le harnais (agents Node,
# cargo) et écrit leurs WorkingSet/CPU. Les descendants (rustc, binaires de
# test) ne sont PAS isolables sans WMI : cette limite est déclarée dans la
# preuve. La charge machine est mesurée côté Node (`os.cpus`/`os.freemem`).
#
# Écrit une ligne JSONL par tick : {t_ms, ws_sum, proc_count, cpu_total_s, procs}
# S'arrête quand `-StopFile` existe (dernier tick écrit inclus).
param(
    [Parameter(Mandatory = $true)][string]$TrackFile,
    [Parameter(Mandatory = $true)][string]$OutFile,
    [Parameter(Mandatory = $true)][string]$StopFile,
    [int]$IntervalMs = 250
)

$ErrorActionPreference = 'SilentlyContinue'
$state = @{}
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
        # réécriture atomique en cours : on garde l'état précédent
    }
    return $result
}

function Get-StartTicks {
    param([int]$procId)
    $process = Get-Process -Id $procId -ErrorAction SilentlyContinue
    if ($null -eq $process) { return [int64]0 }
    try { return [int64]$process.StartTime.ToUniversalTime().Ticks } catch { return [int64]0 }
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
            if ($null -ne $state[$procId].cpu) { $script:exitedCpu += [double]$state[$procId].cpu }
            $state.Remove($procId)
        }
    }
    $cpuAlive = 0.0
    $procs = @()
    foreach ($procId in @($state.Keys)) {
        $cpu = $state[$procId].cpu
        if ($null -ne $cpu) { $cpuAlive += [double]$cpu }
        $process = $alive | Where-Object { $null -ne $_ -and [int]$_.Id -eq $procId } | Select-Object -First 1
        if ($null -ne $process) {
            $procs += [pscustomobject]@{
                pid   = $procId
                name  = [string]$process.ProcessName
                label = [string]$state[$procId].label
                ws_mb = [Math]::Round([double]$process.WorkingSet64 / 1MB, 1)
                cpu_s = [Math]::Round([double]$cpu, 3)
            }
        }
    }
    $line = [pscustomobject]@{
        t_ms        = [int64]$sw.ElapsedMilliseconds
        epoch_ms    = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        ws_sum      = $ws
        proc_count  = $state.Count
        cpu_total_s = [Math]::Round($script:exitedCpu + $cpuAlive, 3)
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
                $state[$entry.pid] = @{ cpu = $null; label = $entry.label; start_ticks = (Get-StartTicks -procId $entry.pid) }
            } else {
                $state[$entry.pid].label = $entry.label
            }
        }
        Write-Sample
        Start-Sleep -Milliseconds $IntervalMs
    }
    Write-Sample
} finally {
    $writer.Flush()
    $writer.Dispose()
}
