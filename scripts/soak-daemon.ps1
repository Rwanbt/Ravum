# Ravum MVP-0b - short soak probe for the persistent analysis daemon.
#
# Drives the real `ravum-daemon` JSON-lines server for a bounded number of requests,
# alternating fresh contents and re-submissions of an already-seen content, and samples
# the process working set and the on-disk state after every request. This is a SHORT
# soak (minutes, not hours): it detects gross leaks/regressions over a bounded campaign,
# not a long-lived production soak.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/soak-daemon.ps1 [-Seconds 120] [-Requests 40] [-Out evidence/...]
#
# Exit code: 0 when every request succeeded and the daemon shut down cleanly, 1 otherwise.

param(
    [string]$Root = '',
    [ValidateRange(20, 1800)]
    [int]$Seconds = 120,
    [ValidateRange(8, 500)]
    [int]$Requests = 40,
    [string]$WorkDir = '',
    [string]$Out = '',
    [string]$DaemonBinary = ''
)

$ErrorActionPreference = 'Stop'
if (-not $Root) { $Root = Split-Path -Parent $PSScriptRoot }
Push-Location $Root
try {
    if (-not $DaemonBinary) { $DaemonBinary = Join-Path $Root 'target\debug\ravum-daemon.exe' }
    if (-not (Test-Path $DaemonBinary)) {
        Write-Host "daemon binary not found: $DaemonBinary (run: cargo build -p ravum-daemon --bin ravum-daemon)"
        exit 1
    }
    if (-not $WorkDir) { $WorkDir = Join-Path $Root 'target\ravum\soak-daemon' }
    Remove-Item -Recurse -Force $WorkDir -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
    if (-not $Out) { $Out = Join-Path $Root 'evidence/mvp0b-daemon-soak-2026-11-08.json' }

    # Run an immutable copy so a concurrent `cargo build` in another task cannot swap the
    # binary under the measured process.
    $runDir = Join-Path $Root 'target\ravum\soak-run'
    Remove-Item -Recurse -Force $runDir -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $runDir | Out-Null
    $binary = Join-Path $runDir 'ravum-daemon.exe'
    Copy-Item $DaemonBinary $binary -Force

    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $binary
    $info.Arguments = "--work-dir `"$WorkDir`""
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.CreateNoWindow = $true
    $process = [System.Diagnostics.Process]::Start($info)
    $stderrTask = $process.StandardError.ReadToEndAsync()

    function Read-JsonLine([int]$timeoutMs) {
        $task = $process.StandardOutput.ReadLineAsync()
        if (-not $task.Wait($timeoutMs)) { throw "timeout waiting for a daemon response" }
        return ($task.Result | ConvertFrom-Json)
    }

    $ready = Read-JsonLine 20000
    if ($ready.event -ne 'ready') { throw "unexpected first line: $($ready | ConvertTo-Json -Compress)" }

    function Get-DiskBytes([string]$dir) {
        if (-not (Test-Path $dir)) { return 0 }
        $sum = (Get-ChildItem $dir -Recurse -File -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum).Sum
        if ($null -eq $sum) { return 0 }
        return [int64]$sum
    }
    function Get-Load {
        $cpu = (Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage)
        $free = (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory * 1024
        return @{ cpu_percent = [int]$cpu; free_bytes = [int64]$free }
    }

    $started = Get-Date
    $deadline = $started.AddSeconds($Seconds)
    $samples = New-Object System.Collections.ArrayList
    $failures = 0; $failureDetails = New-Object System.Collections.ArrayList
    $previous = $null
    $revision = 0
    $seen = @{}
    $index = 0
    while ($index -lt $Requests -and (Get-Date) -lt $deadline) {
        $index++
        $revision++
        # Every fourth request re-submits a previously seen content: that must be a cache
        # hit with no rustc run. The others are genuinely new contents.
        if ($index % 4 -eq 0 -and $seen.Count -gt 0) {
            $values = @($seen.Values); $content = $values[(Get-Random -Minimum 0 -Maximum $values.Count)]
            $expect_new = $false
        } else {
            $content = "pub fn soak_value_$index() -> u64 { $index }`n"
            $seen[$index] = $content
            $expect_new = $true
        }
        $request = [ordered]@{
            id = "$index"
            cmd = 'analyze'
            revision = $revision
            label = "soak-$index"
            root = 'src/lib.rs'
            files = @(@{ path = 'src/lib.rs'; content = $content })
            context = [ordered]@{ edition = '2021'; crate_name = 'soak'; crate_type = 'lib'; flags = @() }
        } | ConvertTo-Json -Depth 8 -Compress
        $process.StandardInput.WriteLine($request)
        $process.StandardInput.Flush()
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $response = Read-JsonLine 60000
        $sw.Stop()
        $process.Refresh()
        if (-not $response.ok -or -not $report.success) { $failures++; [void]$failureDetails.Add([pscustomobject]@{ index = $index; revision = $revision; ok = $response.ok; success = $report.success; status = $report.status; error = $response.error; raw = ($response | ConvertTo-Json -Depth 3 -Compress) }) }
        $disk = Get-DiskBytes $WorkDir
        $load = Get-Load
        $report = $response.report
        $sample = [ordered]@{
            index = $index
            revision = $revision
            wall_ms = [int64]$sw.ElapsedMilliseconds
            daemon_wall_ms = $response.wall_ms
            ok = [bool]$response.ok
            success = [bool]$report.success
            status = $report.status
            cache_kind = $report.cache.kind
            analysis_ms = $report.timings.analysis_ms
            total_ms = $report.timings.total_ms
            working_set_bytes = [int64]$process.WorkingSet64
            private_bytes = [int64]$process.PrivateMemorySize64
            disk_bytes = [int64]$disk
            cpu_percent = $load.cpu_percent
            free_bytes = $load.free_bytes
            rss_delta_bytes = if ($null -eq $previous) { 0 } else { [int64]$process.WorkingSet64 - [int64]$previous }
        }
        [void]$samples.Add([pscustomobject]$sample)
        $previous = $process.WorkingSet64
        Write-Host ("[{0,3}/{1}] ok={2} status={3} cache={4} total={5}ms rss={6}MiB disk={7}MiB" -f `
            $index, $Requests, $response.ok, $report.status, $report.cache.kind, `
            $report.timings.total_ms, [math]::Round($process.WorkingSet64 / 1MB, 2), [math]::Round($disk / 1MB, 2))
    }

    $process.StandardInput.WriteLine('{"id":"stats","cmd":"stats"}')
    $process.StandardInput.Flush()
    $stats = Read-JsonLine 20000
    $process.StandardInput.WriteLine('{"id":"shutdown","cmd":"shutdown"}')
    $process.StandardInput.Flush()
    $shutdown = Read-JsonLine 20000
    $process.WaitForExit(20000) | Out-Null
    $stderrText = $stderrTask.Result
    $exitCode = if ($process.HasExited) { $process.ExitCode } else { -1 }

    $rss = $samples | ForEach-Object { $_.working_set_bytes }
    $warm = $rss | Select-Object -Skip ([math]::Min(3, [math]::Max(0, $rss.Count - 1)))
    $summary = [ordered]@{
        schema_version = 1
        tool = 'ravum-daemon soak'
        date = (Get-Date).ToString('yyyy-MM-dd')
        started_at = $started.ToString('o')
        duration_s = [math]::Round(((Get-Date) - $started).TotalSeconds, 1)
        requests = $samples.Count
        requested_requests = $Requests
        failures = $failures
        failure_details = $failureDetails
        expected_seconds_budget = $Seconds
        work_dir = $WorkDir
        daemon_pid = $process.Id
        daemon_exit_code = $exitCode
        shutdown_ack = $shutdown.ok
        stderr_tail = ($stderrText -split "`n" | Select-Object -Last 5) -join "`n"
        warmup_samples = 3
        rss_bytes = [ordered]@{
            first = $rss[0]
            warmup_last = $rss[[math]::Min(3, $rss.Count - 1)]
            last = $rss[$rss.Count - 1]
            min = ($rss | Measure-Object -Minimum).Minimum
            max = ($rss | Measure-Object -Maximum).Maximum
            warmup_delta_bytes = $rss[$rss.Count - 1] - $rss[[math]::Min(3, $rss.Count - 1)]
        }
        disk_bytes = [ordered]@{
            first = $samples[0].disk_bytes
            last = $samples[$samples.Count - 1].disk_bytes
            delta = $samples[$samples.Count - 1].disk_bytes - $samples[0].disk_bytes
        }
        cache_hits_memory = @($samples | Where-Object { $_.cache_kind -eq 'Memory' }).Count
        cache_hits_disk = @($samples | Where-Object { $_.cache_kind -eq 'Disk' }).Count
        cache_misses = @($samples | Where-Object { $_.cache_kind -eq 'Miss' -or $_.cache_kind -eq 'miss' }).Count
        stats = $stats.stats
        samples = $samples
        limits = @(
            'short bounded soak (minutes), not a multi-hour production soak',
            'single process, single client, one daemon; no crash/restart during the campaign',
            'the daemon is asked sequentially (one request at a time), not under concurrent load',
            'RSS is the OS working set of the whole process, sampled per request',
            'load values come from the shared machine and other agents may run concurrently'
        )
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Out) | Out-Null
    [System.IO.File]::WriteAllText($Out, ($summary | ConvertTo-Json -Depth 8), (New-Object System.Text.UTF8Encoding($false)))
    Write-Host "soak: requests=$($summary.requests) failures=$failures rss_first=$([math]::Round($summary.rss_bytes.first/1MB,2))MiB rss_last=$([math]::Round($summary.rss_bytes.last/1MB,2))MiB rss_delta=$([math]::Round($summary.rss_bytes.warmup_delta_bytes/1MB,2))MiB disk_delta=$([math]::Round($summary.disk_bytes.delta/1MB,2))MiB exit=$exitCode"
    Write-Host "written: $Out"
    if ($failures -gt 0 -or $exitCode -ne 0 -or -not $shutdown.ok) { exit 1 }
    exit 0
}
finally {
    Pop-Location
}
