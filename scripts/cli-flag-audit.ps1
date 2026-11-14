#!/usr/bin/env pwsh
# cli-flag-audit — every flag the CLI documents in its own usage string must be
# accepted by the corresponding parser.
#
# The usage strings live in `crates/ravum-lang-cli/src/args.rs`; the parsers sit
# right below them. A flag added to the help text but not to the parser (or the
# reverse) is invisible to users until they try it. This audit runs each
# documented flag against the real binary and requires that it is *not* rejected
# as an unknown option.
#
# Usage: pwsh -File scripts/cli-flag-audit.ps1

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$exe = if ($IsWindows -or $env:OS -eq 'Windows_NT') { 'ravum-lang.exe' } else { 'ravum-lang' }
$binary = Join-Path $root (Join-Path 'target' (Join-Path 'debug' $exe))
if (-not (Test-Path $binary)) {
    throw "ravum-lang not found at ${binary}: run cargo build -p ravum-lang-cli first"
}

$argsPath = Join-Path $root 'crates/ravum-lang-cli/src/args.rs'
$text = Get-Content $argsPath -Raw

# Usage strings look like:  "check" => "USAGE: ravum-lang check [PATH] [--json] ..."
$usagePattern = '"([a-z]+)"\s*=>\s*"USAGE: ravum-lang \1([^"]*)"'
$documented = @{}
foreach ($match in [regex]::Matches($text, $usagePattern)) {
    $command = $match.Groups[1].Value
    # Only the USAGE line declares flags. The description below it quotes other
    # tools (`rustc --edition 2021`) and other commands (`lint --deny`), which are
    # not flags of this command — extracting them produced three false positives.
    $usageLine = ($match.Groups[2].Value -split '\\n')[0]
    $flags = [regex]::Matches($usageLine, '(--[a-z][a-z-]*)') |
        ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
    if ($flags.Count -gt 0) { $documented[$command] = $flags }
}

# Flags that consume a value, with a valid value.
$values = @{
    '--profile'      = 'core'
    '--target'       = 'wasm32-wasip1'
    '--filter'       = 'none'
    '--deny'         = 'all'
    '--out'          = 'target/flag-audit-doc'
    '--max-rebuilds' = '0'
    '--emit-rust'    = 'target/flag-audit-emit.rs'
    '--rt-entry'     = 'main'
}

# Flags that only make sense with a companion flag. `check --rt-entry` requires
# `--profile rt` (a `build` with an entry selects the rt profile itself), so the
# audit supplies the companion: without it the invocation is a usage error and
# the flag would look rejected.
$companions = @{
    '--rt-entry' = @{
        'check' = @('--profile', 'rt')
    }
}

$work = Join-Path $root (Join-Path 'target' 'flag-audit')
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Force -Path (Join-Path $work 'src') | Out-Null
[System.IO.File]::WriteAllText(
    (Join-Path $work 'ravum.toml'),
    "[package]`nname = ""flagaudit""`nversion = ""0.1.0""`nedition = ""core-1.0""`n")
[System.IO.File]::WriteAllText(
    (Join-Path $work 'src/main.rv'),
    "use std::io;`n`nfn main() {`n    io::println(""x"");`n}`n")

$results = @()
foreach ($command in ($documented.Keys | Sort-Object)) {
    foreach ($flag in $documented[$command]) {
        $invocation = @($command)
        if ($command -in @('check', 'build', 'run', 'test', 'fmt', 'lint', 'doc')) {
            $invocation += $work
        }
        elseif ($command -eq 'init') {
            $invocation += (Join-Path $work ('init-' + $flag.TrimStart('-')))
        }
        if ($command -eq 'watch') {
            # Bound the loop: without this the audit waits forever.
            $invocation += @('--max-rebuilds', '0')
        }
        $invocation += $flag
        if ($values.ContainsKey($flag)) { $invocation += $values[$flag] }
        if ($companions.ContainsKey($flag)) {
            $extra = $companions[$flag][$command]
            if ($extra) { $invocation += $extra }
        }

        # Bounded run: an audit must never hang on a command that builds or runs
        # a program. A timeout is a FAIL with an explicit reason.
        $stdout = [System.IO.Path]::GetTempFileName()
        $stderr = [System.IO.Path]::GetTempFileName()
        $quoted = $invocation | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }
        $process = Start-Process -FilePath $binary -ArgumentList $quoted -NoNewWindow -PassThru `
            -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $finished = $process.WaitForExit(20000)
        if (-not $finished) {
            try { $process.Kill($true) } catch { }
            $status = 'FAIL'
            $detail = 'timeout after 20s'
            $code = -1
        }
        else {
            $code = $process.ExitCode
            $detail = ((Get-Content $stdout -Raw) + (Get-Content $stderr -Raw))
            $status = if ($code -eq 2 -or $detail -match 'unknown option') { 'FAIL' } else { 'PASS' }
        }
        Remove-Item $stdout, $stderr -ErrorAction SilentlyContinue
        $results += [pscustomobject]@{
            Command = $command
            Flag    = $flag
            Status  = $status
            Detail  = ($detail -replace "`r?`n", ' ').Substring(0, [math]::Min(80, ($detail -replace "`r?`n", ' ').Length))
        }
    }
}

$results | Format-Table -AutoSize
$failed = @($results | Where-Object { $_.Status -ne 'PASS' })
Write-Host ("RESUME: {0} documented flag(s) accepted / {1} rejected (of {2})" -f ($results.Count - $failed.Count), $failed.Count, $results.Count)

if (-not $Keep) { Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue }
if ($failed.Count -gt 0) { exit 1 }
exit 0
