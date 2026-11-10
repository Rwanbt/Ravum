#!/usr/bin/env pwsh
# stdlib-codegen-sweep — every documented stdlib call must survive `build`, not
# just `check`.
#
# Why this exists: `math::sqrt(2.0)` passed `check` and failed `rustc` with E0689
# because the emitted float literal had no suffix. The conformance corpus only
# builds the programs it knows about, so that class of defect — "accepted by the
# checker, rejected by rustc" — went unseen. This sweep builds one small program
# per stdlib surface.
#
# Each entry is {id, source, expect}:
#   * `expect` = "build"   → `build` must exit 0;
#   * `expect` = "run:..." → `run` must exit 0 and print the given substring.
#
# Usage: pwsh -File scripts/stdlib-codegen-sweep.ps1 [-Keep]

[CmdletBinding()]
param([switch]$Keep)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$exe = if ($IsWindows -or $env:OS -eq 'Windows_NT') { 'ravum-lang.exe' } else { 'ravum-lang' }
$binary = Join-Path $root (Join-Path 'target' (Join-Path 'debug' $exe))
if (-not (Test-Path $binary)) {
    throw "ravum-lang not found at ${binary}: run cargo build -p ravum-lang-cli first"
}

$work = Join-Path $root (Join-Path 'target' 'stdlib-sweep')
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Force -Path $work | Out-Null

$cases = @(
    @{ id = 'math-sqrt-untyped'; expect = 'run:1.414'; src = @'
use std::io;
use std::math;
fn main() {
    let x = math::sqrt(2.0);
    io::println(&x.to_string());
}
'@ },
    @{ id = 'math-floor-ceil-round'; expect = 'run:8'; src = @'
use std::io;
use std::math;
fn main() {
    let a = math::floor(2.7);
    let b = math::ceil(2.1);
    let c = math::round(2.5);
    let d = a + b + c;
    io::println(&d.to_string());
}
'@ },
    @{ id = 'math-trig'; expect = 'run:1'; src = @'
use std::io;
use std::math;
fn main() {
    let a = math::cos(0.0);
    let b = math::sin(0.0);
    let c = a + b;
    io::println(&c.to_string());
}
'@ },
    @{ id = 'math-pow-min-max'; expect = 'run:1024'; src = @'
use std::io;
use std::math;
fn main() {
    let p = math::pow(2.0, 10.0);
    let q = math::min(p, p);
    let r = math::max(q, p);
    io::println(&r.to_string());
}
'@ },
    @{ id = 'math-abs'; expect = 'run:5'; src = @'
use std::io;
use std::math;
fn main() {
    let x = math::abs(-5.0);
    io::println(&x.to_string());
}
'@ },
    @{ id = 'f64-methods'; expect = 'run:2'; src = @'
use std::io;
fn main() {
    let x: f64 = -4.0;
    let a = x.abs();
    let b = a.sqrt();
    io::println(&b.to_string());
}
'@ },
    @{ id = 'int-abs'; expect = 'run:7'; src = @'
use std::io;
fn main() {
    let n: i32 = -7;
    let a = n.abs();
    io::println(&a.to_string());
}
'@ },
    @{ id = 'string-surface'; expect = 'run:AB'; src = @'
use std::io;
use std::string;
fn main() {
    let s = String::from("  ab  ");
    let t = s.trim().to_uppercase();
    io::println(&t);
}
'@ },
    @{ id = 'string-replace-lines'; expect = 'run:3'; src = @'
use std::io;
use std::string;
fn main() {
    let s = String::from("a\nb\nc");
    let parts = s.lines();
    let n = parts.len();
    io::println(&n.to_string());
}
'@ },
    @{ id = 'vec-surface'; expect = 'run:2'; src = @'
use std::io;
fn main() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
    let n = v.len();
    io::println(&n.to_string());
}
'@ },
    @{ id = 'option-surface'; expect = 'run:7'; src = @'
use std::io;
fn main() {
    let x: Option<i32> = Some(7);
    let y = x.unwrap();
    io::println(&y.to_string());
}
'@ },
    @{ id = 'result-surface'; expect = 'run:ok'; src = @'
use std::io;
use std::string;
fn main() {
    let parsed = string::parse_i64("42");
    let v = parsed.unwrap_or(0);
    if v > 0 {
        io::println("ok");
    }
}
'@ },
    @{ id = 'conversions'; expect = 'run:42'; src = @'
use std::io;
fn main() {
    let n: i32 = 42;
    let s = n.to_string();
    io::println(&s);
}
'@ },
    @{ id = 'bool-char-to-string'; expect = 'run:true'; src = @'
use std::io;
fn main() {
    let b = true;
    let c = 'a';
    let left = b.to_string();
    let right = c.to_string();
    io::println(&left);
    io::println(&right);
}
'@ },
    @{ id = 'fs-read-write'; expect = 'run:0'; src = @'
use std::io;
use std::fs;
fn main() {
    let text = fs::read_to_string("in.txt").unwrap_or(String::new());
    let n = text.len();
    io::println(&n.to_string());
}
'@ },
    @{ id = 'string-predicates'; expect = 'run:yes'; src = @'
use std::io;
use std::string;
fn main() {
    let s = String::from("Hello");
    let a = s.starts_with("He");
    let b = s.ends_with("lo");
    let c = s.contains("ell");
    let lower = s.to_lowercase();
    if a && b && c && lower == "hello" {
        io::println("yes");
    }
}
'@ },
    @{ id = 'string-push-and-clear'; expect = 'run:4'; src = @'
use std::io;
use std::string;
fn main() {
    let mut s = String::from("ab");
    s.push_str("c");
    s.push('d');
    let n = s.len();
    io::println(&n.to_string());
    io::println(&n.to_string());
}
'@ },
    @{ id = 'string-as-str'; expect = 'run:2'; src = @'
use std::io;
use std::string;
fn main() {
    let s = String::from("hi");
    let n = s.as_str().len();
    io::println(&n.to_string());
}
'@ },
    @{ id = 'string-parse-f64'; expect = 'run:ok'; src = @'
use std::io;
use std::string;
fn main() {
    let parsed = string::parse_f64("2.5");
    let v = parsed.unwrap_or(0.0);
    if v > 2.0 {
        io::println("ok");
    }
}
'@ },
    @{ id = 'vec-mutating-ops'; expect = 'run:1'; src = @'
use std::io;
fn main() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
    let last = v.pop();
    let n = v.len();
    io::println(&n.to_string());
    let empty = v.is_empty();
    if empty {
        io::println("0");
    } else {
        io::println("1");
    }
}
'@ },
    @{ id = 'vec-clone-and-slice'; expect = 'run:2'; src = @'
use std::io;
fn main() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.push(2);
    let w = v.clone();
    let n = w.len();
    let m = w.as_slice().len();
    if n == m {
        io::println(&n.to_string());
    }
}
'@ },
    @{ id = 'vec-clear'; expect = 'run:0'; src = @'
use std::io;
fn main() {
    let mut v: Vec<i32> = Vec::new();
    v.push(1);
    v.clear();
    let n = v.len();
    io::println(&n.to_string());
}
'@ },
    @{ id = 'option-predicates'; expect = 'run:9'; src = @'
use std::io;
fn main() {
    let some: Option<i32> = Some(9);
    let none: Option<i32> = None;
    if some.is_some() && none.is_none() {
        let v = some.unwrap_or(0);
        io::println(&v.to_string());
    }
}
'@ },
    @{ id = 'result-predicates'; expect = 'run:both'; src = @'
use std::io;
use std::string;
fn main() {
    let good = string::parse_i64("7");
    let bad = string::parse_i64("nope");
    if good.is_ok() && bad.is_err() {
        io::println("both");
    }
}
'@ },
    @{ id = 'result-err'; expect = 'run:yes'; src = @'
use std::io;
use std::string;
fn main() {
    let bad = string::parse_i64("nope");
    let maybe = bad.err();
    if maybe.is_some() {
        io::println("yes");
    }
}
'@ },
    @{ id = 'option-ok-or'; expect = 'run:yes'; src = @'
use std::io;
fn main() {
    let some: Option<i32> = Some(1);
    let converted = some.ok_or(Error::new("bad"));
    if converted.is_ok() {
        io::println("yes");
    }
}
'@ },
    @{ id = 'array-and-slice'; expect = 'run:6'; src = @'
use std::io;
fn main() {
    let a: [i32; 3] = [1, 2, 3];
    let total = a[0] + a[1] + a[2];
    io::println(&total.to_string());
}
'@ }
)

$results = @()
foreach ($case in $cases) {
    $directory = Join-Path $work $case.id
    New-Item -ItemType Directory -Force -Path (Join-Path $directory 'src') | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $directory 'ravum.toml'),
        "[package]`nname = ""sweep-$($case.id)""`nversion = ""0.1.0""`nedition = ""core-1.0""`n")
    [System.IO.File]::WriteAllText((Join-Path (Join-Path $directory 'src') 'main.rv'), $case.src)

    $check = & $binary check $directory 2>&1
    if ($LASTEXITCODE -ne 0) {
        $results += [pscustomobject]@{ Case = $case.id; Stage = 'check'; Status = 'FAIL'; Detail = ($check | Select-Object -Last 1) }
        continue
    }
    $build = & $binary build $directory 2>&1
    if ($LASTEXITCODE -ne 0) {
        $results += [pscustomobject]@{ Case = $case.id; Stage = 'build'; Status = 'FAIL'; Detail = ($build | Select-Object -Last 1) }
        continue
    }
    if ($case.expect -like 'run:*') {
        $expected = $case.expect.Substring(4)
        $output = & $binary run $directory 2>&1
        if ($LASTEXITCODE -ne 0) {
            $results += [pscustomobject]@{ Case = $case.id; Stage = 'run'; Status = 'FAIL'; Detail = ($output | Select-Object -Last 1) }
            continue
        }
        $text = ($output | Out-String)
        if ($text -notlike "*$expected*") {
            $results += [pscustomobject]@{ Case = $case.id; Stage = 'stdout'; Status = 'FAIL'; Detail = "expected $expected, got $($text.Trim())" }
            continue
        }
    }
    $results += [pscustomobject]@{ Case = $case.id; Stage = 'ok'; Status = 'PASS'; Detail = '' }
}

$results | Format-Table -AutoSize
$failed = @($results | Where-Object { $_.Status -ne 'PASS' })
Write-Host ("RESUME: {0} PASS / {1} FAIL (of {2})" -f ($results.Count - $failed.Count), $failed.Count, $results.Count)

if (-not $Keep) { Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue }
if ($failed.Count -gt 0) { exit 1 }
exit 0
