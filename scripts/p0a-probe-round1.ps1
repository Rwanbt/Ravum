# Lead's independent probe matrix for the P0-A / P0-F families.
# Uses the frozen baseline binary so concurrent agent edits cannot change the
# result. Each program is `check`ed then `build`ed; a check-pass/build-fail pair
# is a divergence and is printed as such.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path,
    [string]$Toolchain = "",
    [string]$Out = ""
)
$ErrorActionPreference = "Stop"
if (-not $Toolchain) { $Toolchain = Join-Path $RepoRoot "target\baseline-bin\ravum-lang.exe" }
if (-not $Out) { $Out = Join-Path $RepoRoot "target\lead-probe-matrix.json" }
$scratch = Join-Path $RepoRoot "target\lead-probes"
if (Test-Path $scratch) { Remove-Item -Recurse -Force $scratch }
New-Item -ItemType Directory -Path $scratch | Out-Null

$cases = [ordered]@{}
$cases["str-param-receives-as-str"] = @'
use std::io;
use std::string;
fn g(s: &str) -> usize {
    return s.len();
}
fn main() {
    let trimmed = " a ".trim();
    let n = g(trimmed.as_str());
    io::println("ok");
}
'@
$cases["str-struct-field"] = @'
struct S {
    text: &str,
}
fn main() {
    let s = S { text: "a" };
}
'@
$cases["str-return-literal"] = @'
fn lit() -> &str {
    return "a";
}
fn main() {
    let x = lit();
}
'@
$cases["array-element-move"] = @'
fn second(a: [String; 2]) -> String {
    return a[1];
}
fn main() {
}
'@
$cases["tuple-element-move"] = @'
fn first(t: (String, i32)) -> String {
    return t.0;
}
fn main() {
}
'@
$cases["vec-element-move-must-fail"] = @'
fn second(v: Vec<String>) -> String {
    return v[1];
}
fn main() {
}
'@
$cases["move-after-unwrap"] = @'
use std::io;
fn main() {
    let o = Some(String::from("x"));
    let s = o.unwrap();
    let t = o;
    io::println(s.as_str());
}
'@
$cases["generic-with-str"] = @'
fn id<T>(x: T) -> T {
    return x;
}
fn main() {
    let r = id("a");
}
'@
$cases["option-str-stored-must-fail"] = @'
fn main() {
    let o = Some("a");
    let r = o;
}
'@
$cases["vec-str-stored-must-fail"] = @'
fn main() {
    let mut v = Vec::new();
    v.push("a");
}
'@
$cases["param-mut-binding"] = @'
fn bump(mut x: i32) -> i32 {
    x = x + 1;
    return x;
}
fn main() {
    let n = bump(1);
}
'@
$cases["param-assign-without-mut"] = @'
fn bump(x: i32) -> i32 {
    x = x + 1;
    return x;
}
fn main() {
}
'@
$cases["array-annotated-len-mismatch"] = @'
fn main() {
    let a: [i32; 3] = [1, 2];
}
'@
$cases["array-annotated-elem-mismatch"] = @'
fn main() {
    let a: [i32; 2] = [1, true];
}
'@
$cases["nested-match-exhaustive"] = @'
fn f(x: Option<(i32, bool)>) -> i32 {
    match x {
        Some((n, true)) => n,
        Some((n, false)) => n,
        None => 0,
    }
}
fn main() {
}
'@
$cases["nested-match-missing-arm"] = @'
fn f(x: Option<(i32, bool)>) -> i32 {
    match x {
        Some((n, true)) => n,
        None => 0,
    }
}
fn main() {
}
'@
$cases["block-arg-borrow"] = @'
fn take(x: &i32) -> i32 {
    return *x;
}
fn main() {
    let v = 1;
    let n = take({ &v });
}
'@
$cases["borrow-through-if-arg"] = @'
fn take(x: &i32) -> i32 {
    return *x;
}
fn main() {
    let a = 1;
    let b = 2;
    let c = true;
    let n = take(if c { &a } else { &b });
}
'@
$cases["static-str-in-tuple"] = @'
fn main() {
    let t: (&str, i32) = ("a", 1);
}
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
    Write-Host ("{0,-34} check={1} build={2}{3} [{4}]" -f $name, $checkExit, $buildExit, $(if ($divergence) { "  <== DIVERGENCE" } else { "" }), ($codes -join ","))
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
