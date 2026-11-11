# Lead's probe matrix, round 2 — ownership, moves, exhaustiveness, borrows.
# Frozen baseline binary; check-pass/build-fail = divergence.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path,
    [string]$Toolchain = "",
    [string]$Out = ""
)
$ErrorActionPreference = "Stop"
if (-not $Toolchain) { $Toolchain = Join-Path $RepoRoot "target\baseline-bin\ravum-lang.exe" }
if (-not $Out) { $Out = Join-Path $RepoRoot "target\lead-probe-matrix2.json" }
$scratch = Join-Path $RepoRoot "target\lead-probes2"
if (Test-Path $scratch) { Remove-Item -Recurse -Force $scratch }
New-Item -ItemType Directory -Path $scratch | Out-Null

$cases = [ordered]@{}

$cases["unwrap-or-use-after-move"] = @'
use std::io;
fn main() {
    let o = Some(String::from("x"));
    let s = o.unwrap_or(String::from("y"));
    let t = o;
    io::println(s.as_str());
}
'@

$cases["result-unwrap-use-after-move"] = @'
use std::io;
fn main() {
    let r: Result<i32, Error> = Ok(1);
    let v = r.unwrap();
    let w = r;
    io::println("ok");
}
'@

$cases["struct-partial-then-whole"] = @'
struct D { a: String, b: i32 }
fn main() {
    let d = D { a: String::from("a"), b: 1 };
    let x = d.a;
    let y = d;
}
'@

$cases["struct-partial-then-other"] = @'
struct D { a: String, b: i32 }
fn main() {
    let d = D { a: String::from("a"), b: 1 };
    let x = d.a;
    let y = d.b;
}
'@

$cases["move-then-use-param"] = @'
use std::io;
fn consume(s: String) { }
fn main() {
    let s = String::from("x");
    consume(s);
    io::println(s.as_str());
}
'@

$cases["move-in-loop-from-outer"] = @'
fn main() {
    let s = String::from("x");
    let mut i = 0;
    while i < 2 {
        let t = s;
        i = i + 1;
    }
}
'@

$cases["int-match-nonexhaustive"] = @'
fn f(n: i32) -> i32 {
    match n {
        1 => 1,
        2 => 2,
    }
}
fn main() { }
'@

$cases["option-option-missing-arm"] = @'
fn f(x: Option<Option<i32>>) -> i32 {
    match x {
        Some(Some(n)) => n,
        None => 0,
    }
}
fn main() { }
'@

$cases["option-option-exhaustive"] = @'
fn f(x: Option<Option<i32>>) -> i32 {
    match x {
        Some(Some(n)) => n,
        Some(None) => -1,
        None => 0,
    }
}
fn main() { }
'@

$cases["vec-index-move"] = @'
fn f(v: Vec<String>) -> String {
    return v[0];
}
fn main() { }
'@

$cases["nested-tuple-move"] = @'
fn f(t: ((String, i32), bool)) -> String {
    return t.0.0;
}
fn main() { }
'@

$cases["array-of-tuples-move"] = @'
fn f(a: [(String, i32); 2]) -> String {
    return a[1].0;
}
fn main() { }
'@

$cases["if-value-move-then-reuse"] = @'
use std::io;
fn main() {
    let c = true;
    let x = if c { String::from("a") } else { String::from("b") };
    let y = x;
    let z = x;
    io::println("ok");
}
'@

$cases["for-over-vec-then-use"] = @'
use std::io;
fn main() {
    let v = vec![String::from("a")];
    for x in v {
        let y = x;
    }
    let n = v.len();
    io::println("ok");
}
'@

$cases["mut-ref-param"] = @'
fn bump(x: &mut i32) {
    *x = *x + 1;
}
fn main() {
    let mut n = 1;
    bump(&mut n);
}
'@

$cases["two-mut-borrows"] = @'
fn main() {
    let mut n = 1;
    let a = &mut n;
    let b = &mut n;
}
'@

$cases["borrow-then-mutate"] = @'
fn main() {
    let mut v = Vec::new();
    v.push(1);
    let r = &v;
    v.push(2);
}
'@

$cases["enum-nested-exhaustive"] = @'
enum E { A(Option<i32>), B }
fn f(e: E) -> i32 {
    match e {
        E::A(Some(n)) => n,
        E::A(None) => -1,
        E::B => 0,
    }
}
fn main() { }
'@

$cases["enum-nested-missing"] = @'
enum E { A(Option<i32>), B }
fn f(e: E) -> i32 {
    match e {
        E::A(Some(n)) => n,
        E::B => 0,
    }
}
fn main() { }
'@

$results = New-Object System.Collections.ArrayList
foreach ($name in $cases.Keys) {
    $dir = Join-Path $scratch $name
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $src = Join-Path $dir "main.rv"
    Set-Content -Path $src -Value $cases[$name] -NoNewline
    $checkOut = & $Toolchain check $src 2>&1 | Out-String
    $checkExit = $LASTEXITCODE
    $buildExit = -1
    $buildOut = ""
    if ($checkExit -eq 0) {
        $buildOut = & $Toolchain build $src 2>&1 | Out-String
        $buildExit = $LASTEXITCODE
    }
    $codes = @()
    foreach ($text in @($checkOut, $buildOut)) {
        foreach ($match in [regex]::Matches($text, 'E-[A-Z0-9-]+')) { $codes += $match.Value }
    }
    $codes = @($codes | Sort-Object -Unique)
    $divergence = ($checkExit -eq 0 -and $buildExit -ne 0)
    [void]$results.Add([pscustomobject]@{
        name = $name; check_exit = $checkExit; build_exit = $buildExit
        divergence = $divergence; codes = ($codes -join ",")
        check_tail = (($checkOut -split "`n" | Where-Object { $_ -match 'error\[' } | Select-Object -First 1))
        build_tail = (($buildOut -split "`n" | Where-Object { $_ -match 'error(\[|:)' } | Select-Object -First 2) -join " | ")
    })
    Write-Host ("{0,-32} check={1} build={2}{3} [{4}]" -f $name, $checkExit, $buildExit, $(if ($divergence) { "  <== DIVERGENCE" } else { "" }), ($codes -join ","))
}
$summary = [pscustomobject]@{
    toolchain = $Toolchain; total = $results.Count
    check_pass = ($results | Where-Object { $_.check_exit -eq 0 }).Count
    build_pass = ($results | Where-Object { $_.check_exit -eq 0 -and $_.build_exit -eq 0 }).Count
    divergences = @($results | Where-Object { $_.divergence }).Count
    results = $results
}
$summary | ConvertTo-Json -Depth 5 | Set-Content -Path $Out -Encoding UTF8
Write-Host ""
Write-Host ("SUMMARY: {0} probes; check-pass {1}; build-pass {2}; DIVERGENCES {3}" -f $summary.total, $summary.check_pass, $summary.build_pass, $summary.divergences)
Write-Host ("report: {0}" -f $Out)
