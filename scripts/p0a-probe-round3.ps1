# P0-A probe round 3 — systematic `check` => `build` sweep over axes not covered
# by rounds 1-2.
#
# Each program is a complete, spec-valid Ravum Core 1.0 program written by
# construction (one file, no network, no fixtures). The probe runs the working
# tree toolchain (`check` then `build`). A program whose `check` exits 0 but
# whose `build` exits non-zero is a `check` => `build` DIVERGENCE: it is printed,
# written to the JSON report and makes the script exit 1.
#
# A program rejected by `check` (exit != 0) is reported as `check-rejected`: it
# is NOT a build divergence, but it is not silently dropped either. Programs
# were validated with the same toolchain before being kept in the matrix; a
# non-zero `check_rejected` count means the matrix itself needs a fix.
#
# The four D12 shapes (nested index chain, array behind field/tuple projection,
# shadowed array binding) are measured in a separate `known_open_controls`
# block: they are known open divergences owned by codegen-pipeline, so they do
# not fail this script; a control that starts passing is reported as a notice.
#
# Usage:
#   pwsh -File scripts/p0a-probe-round3.ps1
#   pwsh -File scripts/p0a-probe-round3.ps1 -Toolchain target\baseline-bin\ravum-lang.exe
#   pwsh -File scripts/p0a-probe-round3.ps1 -Out target\my-report.json
#
# Exit codes: 0 = no divergence in the new matrix; 1 = at least one divergence;
# 2 = the matrix contains a program the checker rejects (matrix defect: fix the
# probe, not the compiler). `known_open` and `rejected_controls` are measured
# separately and never change the exit code.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path,
    [string]$Toolchain = "",
    [string]$Out = ""
)
$ErrorActionPreference = "Stop"

if (-not $Toolchain) { $Toolchain = Join-Path $RepoRoot "target\debug\ravum-lang.exe" }
if (-not (Test-Path $Toolchain)) { throw "toolchain not found: $Toolchain" }
if (-not $Out) { $Out = Join-Path $RepoRoot "target\p0a-probe-round3.json" }

$scratch = Join-Path $RepoRoot "target\p0a-probes3"
if (Test-Path $scratch) { Remove-Item -Recurse -Force $scratch }
New-Item -ItemType Directory -Path $scratch | Out-Null

$cases = [ordered]@{}

# ---------------------------------------------------------------------------
# Axis: match-move — a `match` arm owns the payload and moves out of it.
# ---------------------------------------------------------------------------
$cases["match-option-string-move"] = @{ axis = "match-move"; source = @'
fn f(o: Option<String>) -> String {
    match o {
        Some(s) => {
            let t = s;
            return t;
        }
        None => {
            return String::from("none");
        }
    }
}

fn main() {
}
'@ }

$cases["match-option-string-move-arm-block"] = @{ axis = "match-move"; source = @'
fn f(o: Option<String>) -> usize {
    match o {
        Some(s) => {
            let t = s;
            return t.len();
        }
        None => {
            return 0;
        }
    }
}

fn main() {
}
'@ }

$cases["match-tuple-payload-branch-move"] = @{ axis = "match-move"; source = @'
fn f(x: Option<(String, bool)>) -> String {
    match x {
        Some((s, true)) => {
            return s;
        }
        Some((s, false)) => {
            return s;
        }
        None => {
            return String::from("none");
        }
    }
}

fn main() {
}
'@ }

$cases["match-enum-struct-payload-move"] = @{ axis = "match-move"; source = @'
struct P {
    s: String,
    n: i32,
}

enum E {
    V(P),
    Empty,
}

fn f(e: E) -> String {
    match e {
        E::V(p) => {
            return p.s;
        }
        E::Empty => {
            return String::from("empty");
        }
    }
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Axis: enum-array-payload — an enum variant owns an array, moved then indexed.
# ---------------------------------------------------------------------------
$cases["enum-array-payload-index-one"] = @{ axis = "enum-array-payload"; source = @'
enum E {
    Items([String; 2]),
    Empty,
}

fn f(e: E) -> String {
    match e {
        E::Items(a) => {
            return a[1];
        }
        E::Empty => {
            return String::from("empty");
        }
    }
}

fn main() {
}
'@ }

$cases["enum-array-payload-both-elements"] = @{ axis = "enum-array-payload"; source = @'
enum E {
    Items([String; 2]),
    Empty,
}

fn f(e: E) -> usize {
    match e {
        E::Items(a) => {
            return a[0].len() + a[1].len();
        }
        E::Empty => {
            return 0;
        }
    }
}

fn main() {
}
'@ }

$cases["enum-array-payload-move-into-call"] = @{ axis = "enum-array-payload"; source = @'
enum E {
    Items([String; 2]),
    Empty,
}

fn take(s: String) -> usize {
    return s.len();
}

fn f(e: E) -> usize {
    match e {
        E::Items(a) => {
            return take(a[0]);
        }
        E::Empty => {
            return 0;
        }
    }
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Axis: nested-aggregate — moves through struct-in-struct, struct-in-tuple,
# tuple-in-struct, struct-in-array and parenthesised nested tuples.
# ---------------------------------------------------------------------------
$cases["struct-in-struct-field-move"] = @{ axis = "nested-aggregate"; source = @'
struct Inner {
    s: String,
    n: i32,
}

struct Outer {
    inner: Inner,
    tag: i32,
}

fn f(o: Outer) -> String {
    return o.inner.s;
}

fn main() {
}
'@ }

$cases["struct-in-tuple-field-move"] = @{ axis = "nested-aggregate"; source = @'
struct Inner {
    s: String,
    n: i32,
}

fn f(t: (Inner, i32)) -> String {
    return t.0.s;
}

fn main() {
}
'@ }

$cases["tuple-in-struct-field-move"] = @{ axis = "nested-aggregate"; source = @'
struct Holder {
    pair: (String, i32),
}

fn f(h: Holder) -> String {
    return h.pair.0;
}

fn main() {
}
'@ }

$cases["struct-in-array-element-field-move"] = @{ axis = "nested-aggregate"; source = @'
struct P {
    s: String,
    n: i32,
}

fn f(a: [P; 2]) -> String {
    return a[1].s;
}

fn main() {
}
'@ }

$cases["nested-tuple-parenthesised-field"] = @{ axis = "nested-aggregate"; source = @'
fn f(t: ((String, i32), bool)) -> String {
    return (t.0).0;
}

fn main() {
}
'@ }

$cases["struct-in-struct-whole-inner-move"] = @{ axis = "nested-aggregate"; source = @'
struct Inner {
    s: String,
    n: i32,
}

struct Outer {
    inner: Inner,
    tag: i32,
}

fn take(inner: Inner) -> String {
    return inner.s;
}

fn f(o: Outer) -> String {
    return take(o.inner);
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Axis: array-borrow — constant/dynamic index in `&` / `&mut` position.
# ---------------------------------------------------------------------------
$cases["array-element-shared-borrow-argument"] = @{ axis = "array-borrow"; source = @'
fn g(s: &String) -> usize {
    return s.len();
}

fn f(a: [String; 2]) -> usize {
    return g(&a[0]);
}

fn main() {
}
'@ }

$cases["array-element-mut-borrow-argument"] = @{ axis = "array-borrow"; source = @'
fn h(s: &mut String) {
    s.push_str("!");
}

fn f(mut a: [String; 2]) -> usize {
    h(&mut a[0]);
    return a[0].len();
}

fn main() {
}
'@ }

$cases["array-element-borrow-in-condition"] = @{ axis = "array-borrow"; source = @'
fn empty(s: &String) -> bool {
    return s.is_empty();
}

fn f(a: [String; 2]) -> bool {
    if empty(&a[1]) {
        return true;
    }
    return false;
}

fn main() {
}
'@ }

$cases["array-element-dynamic-mut-borrow"] = @{ axis = "array-borrow"; source = @'
fn h(s: &mut String) {
    s.push_str("!");
}

fn f(mut a: [String; 2], i: usize) {
    h(&mut a[i]);
}

fn main() {
}
'@ }

$cases["array-two-shared-borrows-argument"] = @{ axis = "array-borrow"; source = @'
fn g(x: &String, y: &String) -> usize {
    return x.len() + y.len();
}

fn f(a: [String; 2]) -> usize {
    return g(&a[0], &a[1]);
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Axis: reinit-after-move — partial move followed by a field/element assignment.
# ---------------------------------------------------------------------------
$cases["struct-field-reassign-then-whole"] = @{ axis = "reinit-after-move"; source = @'
struct D {
    a: String,
    b: i32,
}

fn f(mut d: D) -> D {
    let x = d.a;
    d.a = String::from("new");
    let y = d.b;
    return d;
}

fn main() {
}
'@ }

$cases["struct-field-reassign-then-read"] = @{ axis = "reinit-after-move"; source = @'
struct D {
    a: String,
    b: i32,
}

fn f(mut d: D) -> usize {
    let x = d.a;
    d.a = String::from("new");
    return d.a.len() + x.len() + (d.b as usize);
}

fn main() {
}
'@ }

$cases["array-element-reassign-after-move"] = @{ axis = "reinit-after-move"; source = @'
fn f(mut a: [String; 2]) -> usize {
    let x = a[0];
    a[0] = String::from("new");
    return a[0].len() + x.len();
}

fn main() {
}
'@ }

$cases["tuple-element-reassign-after-move"] = @{ axis = "reinit-after-move"; source = @'
fn f(mut t: (String, i32)) -> usize {
    let x = t.0;
    t.0 = String::from("new");
    return t.0.len() + x.len() + (t.1 as usize);
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Axis: loop-move — moving a value declared in the loop body (fresh per
# iteration) versus moving an outer binding (invalid and not probed here).
# ---------------------------------------------------------------------------
$cases["while-body-local-move"] = @{ axis = "loop-move"; source = @'
fn f(n: usize) -> usize {
    let mut i: usize = 0;
    let mut total: usize = 0;
    while i < n {
        let s = String::from("x");
        let t = s;
        total += t.len();
        i += 1;
    }
    return total;
}

fn main() {
}
'@ }

$cases["loop-body-local-move-with-break"] = @{ axis = "loop-move"; source = @'
fn f() -> String {
    let mut i: i32 = 0;
    loop {
        let s = String::from("x");
        if i > 2 {
            break;
        }
        let t = s;
        i += 1;
    }
    return String::from("done");
}

fn main() {
}
'@ }

$cases["for-range-body-local-move"] = @{ axis = "loop-move"; source = @'
fn f(n: usize) -> usize {
    let mut total: usize = 0;
    for i in 0..n {
        let s = String::from("x");
        let t = s;
        total += t.len();
    }
    return total;
}

fn main() {
}
'@ }

# `for x in array` is NOT expressible in Core 1.0 (`for` iterates a range or an
# owned Vec<T>, EXPR-6.4): the checker rejects it, so the closest valid axis is
# an owned Vec<T> whose elements are moved in the body.
$cases["for-vec-body-element-move"] = @{ axis = "loop-move"; source = @'
fn f(v: Vec<String>) -> usize {
    let mut total: usize = 0;
    for x in v {
        let y = x;
        total += y.len();
    }
    return total;
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Axis: if-move — branch returns moving the same or different elements.
# ---------------------------------------------------------------------------
$cases["if-returns-different-array-elements"] = @{ axis = "if-move"; source = @'
fn f(c: bool, a: [String; 2]) -> String {
    if c {
        return a[0];
    }
    return a[1];
}

fn main() {
}
'@ }

$cases["if-else-returns-same-array-element"] = @{ axis = "if-move"; source = @'
fn f(c: bool, a: [String; 2]) -> String {
    if c {
        return a[0];
    } else {
        return a[0];
    }
}

fn main() {
}
'@ }

$cases["if-value-then-move"] = @{ axis = "if-move"; source = @'
fn f(c: bool) -> String {
    let x = if c {
        String::from("a")
    } else {
        String::from("b")
    };
    return x;
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Axis: method-receiver — self / &self / &mut self receiving moved elements.
# ---------------------------------------------------------------------------
$cases["self-method-consumes-struct"] = @{ axis = "method-receiver"; source = @'
struct Holder {
    text: String,
}

impl Holder {
    fn into_text(self) -> String {
        return self.text;
    }
}

fn f(h: Holder) -> String {
    return h.into_text();
}

fn main() {
}
'@ }

$cases["mut-self-method-receives-element"] = @{ axis = "method-receiver"; source = @'
struct Holder {
    text: String,
}

impl Holder {
    fn push(&mut self, s: String) {
        self.text.push_str(s.as_str());
    }
}

fn f(mut h: Holder, a: [String; 2]) -> String {
    h.push(a[0]);
    return h.text;
}

fn main() {
}
'@ }

$cases["mut-self-method-on-array-element"] = @{ axis = "method-receiver"; source = @'
struct Counter {
    n: i32,
}

impl Counter {
    fn bump(&mut self) {
        self.n = self.n + 1;
    }
}

fn f(mut a: [Counter; 2]) -> i32 {
    a[0].bump();
    return a[0].n;
}

fn main() {
}
'@ }

$cases["self-method-on-array-element"] = @{ axis = "method-receiver"; source = @'
struct Holder {
    text: String,
}

impl Holder {
    fn into_text(self) -> String {
        return self.text;
    }
}

fn f(a: [Holder; 2]) -> String {
    return a[1].into_text();
}

fn main() {
}
'@ }

$cases["self-method-on-array-element-dynamic-index"] = @{ axis = "method-receiver"; source = @'
struct Holder {
    text: String,
}

impl Holder {
    fn into_text(self) -> String {
        return self.text;
    }
}

fn f(a: [Holder; 2], i: usize) -> String {
    return a[i].into_text();
}

fn main() {
}
'@ }

$cases["self-method-on-tuple-element"] = @{ axis = "method-receiver"; source = @'
struct Holder {
    text: String,
}

impl Holder {
    fn into_text(self) -> String {
        return self.text;
    }
}

fn f(t: (Holder, i32)) -> String {
    return t.0.into_text();
}

fn main() {
}
'@ }

$cases["shared-self-method-on-nested-field"] = @{ axis = "method-receiver"; source = @'
struct Inner {
    s: String,
}

struct Outer {
    inner: Inner,
}

impl Inner {
    fn len_of(&self) -> usize {
        return self.s.len();
    }
}

fn f(o: &Outer) -> usize {
    return o.inner.len_of();
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Axis: generic-array — non-`Copy` `T` with arrays and generic wrappers.
# ---------------------------------------------------------------------------
$cases["generic-array-second-element"] = @{ axis = "generic-array"; source = @'
fn second<T>(a: [T; 2]) -> T {
    return a[1];
}

fn main() {
}
'@ }

$cases["generic-array-instantiated-string"] = @{ axis = "generic-array"; source = @'
fn first<T>(a: [T; 2]) -> T {
    return a[0];
}

fn main() {
    let a = [String::from("a"), String::from("b")];
    let s = first(a);
}
'@ }

$cases["generic-wrapper-option-move"] = @{ axis = "generic-array"; source = @'
fn wrap<T>(x: T) -> Option<T> {
    return Some(x);
}

fn f() -> Option<String> {
    return wrap(String::from("x"));
}

fn main() {
}
'@ }

$cases["generic-array-identity-return"] = @{ axis = "generic-array"; source = @'
fn identity<T>(a: [T; 2]) -> [T; 2] {
    return a;
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Axis: static-str-array — `&'static str` as an array element.
#
# Note: `let s = a[1];` and `fn f(a: [&str; 2]) -> &str { return a[1]; }` are
# rejected by the checker with E-OWN-REF-ESCAPE (a sema finding, reported
# separately: OWN-6.2/MUST says a `&'static str` is Copy, storable and
# returnable). They are therefore NOT part of this valid-program matrix; the
# instruction-position forms below are.
# ---------------------------------------------------------------------------
$cases["array-static-str-element-compare"] = @{ axis = "static-str-array"; source = @'
fn f(a: [&str; 2]) -> bool {
    return a[0] == "zero";
}

fn main() {
}
'@ }

$cases["array-static-str-elements-compare"] = @{ axis = "static-str-array"; source = @'
fn f(a: [&str; 2]) -> bool {
    return a[0] == a[1];
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Axis: nested-unwrap — `Option`/`Result` payloads moved then unwrapped.
# ---------------------------------------------------------------------------
$cases["option-vec-unwrap"] = @{ axis = "nested-unwrap"; source = @'
fn f(o: Option<Vec<String>>) -> usize {
    let v = o.unwrap();
    return v.len();
}

fn main() {
}
'@ }

$cases["option-option-unwrap"] = @{ axis = "nested-unwrap"; source = @'
fn f(o: Option<Option<String>>) -> String {
    let inner = o.unwrap();
    return inner.unwrap();
}

fn main() {
}
'@ }

$cases["option-string-unwrap-chain"] = @{ axis = "nested-unwrap"; source = @'
fn f(o: Option<String>) -> usize {
    return o.unwrap().len();
}

fn main() {
}
'@ }

$cases["result-vec-unwrap"] = @{ axis = "nested-unwrap"; source = @'
fn f(r: Result<Vec<String>, Error>) -> usize {
    let v = r.unwrap();
    return v.len();
}

fn main() {
}
'@ }

$cases["option-tuple-unwrap-project"] = @{ axis = "nested-unwrap"; source = @'
fn f(o: Option<(String, i32)>) -> String {
    let t = o.unwrap();
    return t.0;
}

fn main() {
}
'@ }

# ---------------------------------------------------------------------------
# Known-open D12 controls (owned by codegen-pipeline). These are NOT part of
# the new matrix and do not fail the script; a control that now builds is a
# notice that the corpus can gain the corresponding accepted-mode case.
# ---------------------------------------------------------------------------
$knownOpen = [ordered]@{}
$knownOpen["d12-nested-index-chain"] = @'
fn f(a: [[String; 2]; 2]) -> String {
    return a[0][1];
}

fn main() {
}
'@
$knownOpen["d12-field-projection"] = @'
struct H {
    items: [String; 2],
}

fn f(h: H) -> String {
    return h.items[0];
}

fn main() {
}
'@
$knownOpen["d12-tuple-projection"] = @'
fn f(t: ([String; 2], i32)) -> String {
    return t.0[0];
}

fn main() {
}
'@
$knownOpen["d12-shadowed-binding"] = @'
fn f(a: [String; 2]) -> String {
    let x = a[0];
    {
        let a = 2;
        let b = a;
    }
    return x;
}

fn main() {
}
'@

# ---------------------------------------------------------------------------
# Rejected controls: programs kept OUT of the matrix because `check` rejects
# them. They are measured and reported for the record, but they cannot be a
# `check` => `build` divergence. Two of them are spec-vs-checker findings
# (OWN-6.2 says a `&'static str` is Copy/storable/returnable); the `for` one is
# a correct rejection (EXPR-6.4: `for` iterates a range or an owned Vec).
# ---------------------------------------------------------------------------
$rejectedControls = [ordered]@{}
$rejectedControls["rejected-for-over-array"] = @'
fn f(a: [String; 2]) -> usize {
    let mut total: usize = 0;
    for x in a {
        total += x.len();
    }
    return total;
}

fn main() {
}
'@
$rejectedControls["rejected-array-static-str-element-store"] = @'
fn main() {
    let a = ["zero", "one"];
    let s = a[1];
}
'@
$rejectedControls["rejected-array-static-str-element-return"] = @'
fn f(a: [&str; 2]) -> &str {
    return a[1];
}

fn main() {
}
'@

function Invoke-Probe([string]$name, [string]$axis, [string]$source) {
    $dir = Join-Path $scratch $name
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $src = Join-Path $dir "main.rv"
    Set-Content -Path $src -Value $source -NoNewline
    $checkOut = & $Toolchain check $src 2>&1 | Out-String
    $checkExit = $LASTEXITCODE
    $buildOut = ""
    $buildExit = -1
    if ($checkExit -eq 0) {
        $buildOut = & $Toolchain build $src 2>&1 | Out-String
        $buildExit = $LASTEXITCODE
    }
    $ravumCodes = @()
    $rustcCodes = @()
    foreach ($text in @($checkOut, $buildOut)) {
        foreach ($match in [regex]::Matches($text, 'E-[A-Z0-9]+(?:-[A-Z0-9]+)*')) { $ravumCodes += $match.Value }
        foreach ($match in [regex]::Matches($text, 'error\[(E[0-9]{4})\]')) { $rustcCodes += $match.Groups[1].Value }
    }
    $ravumCodes = @($ravumCodes | Sort-Object -Unique)
    $rustcCodes = @($rustcCodes | Sort-Object -Unique)
    $divergence = ($checkExit -eq 0 -and $buildExit -ne 0)
    $verdict = if ($checkExit -ne 0) { "check-rejected" } elseif ($divergence) { "divergence" } else { "build-pass" }
    return [pscustomobject]@{
        name         = $name
        axis         = $axis
        verdict      = $verdict
        divergence   = $divergence
        check_exit   = $checkExit
        build_exit   = $buildExit
        ravum_codes  = ($ravumCodes -join ",")
        rustc_codes  = ($rustcCodes -join ",")
        check_tail   = (($checkOut -split "`n" | Where-Object { $_ -match 'error' } | Select-Object -First 1))
        build_tail   = (($buildOut -split "`n" | Where-Object { $_ -match 'error(\[|:)' } | Select-Object -First 2) -join " | ")
        program      = $source
    }
}

$results = New-Object System.Collections.ArrayList
foreach ($name in $cases.Keys) {
    $entry = $cases[$name]
    $result = Invoke-Probe -name $name -axis $entry.axis -source $entry.source
    [void]$results.Add($result)
    $marker = if ($result.divergence) { "  <== DIVERGENCE" } elseif ($result.verdict -eq "check-rejected") { "  (check rejected)" } else { "" }
    Write-Host ("{0,-44} {1,-18} check={2} build={3}{4}" -f $name, $result.axis, $result.check_exit, $result.build_exit, $marker)
}

$knownOpenResults = New-Object System.Collections.ArrayList
foreach ($name in $knownOpen.Keys) {
    $result = Invoke-Probe -name $name -axis "known-open-d12" -source $knownOpen[$name]
    [void]$knownOpenResults.Add($result)
    $state = if ($result.divergence) { "still open (check 0 / build != 0)" } elseif ($result.verdict -eq "build-pass") { "NOW BUILDS — corpus can gain it" } else { "check rejected (matrix needs review)" }
    Write-Host ("{0,-44} {1,-18} check={2} build={3}  {4}" -f $name, "known-open-d12", $result.check_exit, $result.build_exit, $state)
}

$rejectedResults = New-Object System.Collections.ArrayList
foreach ($name in $rejectedControls.Keys) {
    $result = Invoke-Probe -name $name -axis "rejected-control" -source $rejectedControls[$name]
    [void]$rejectedResults.Add($result)
    Write-Host ("{0,-44} {1,-18} check={2} build={3}  (expected check-rejected: not a divergence)" -f $name, "rejected-control", $result.check_exit, $result.build_exit)
}

$axisCounts = @($results | Group-Object axis | ForEach-Object { [pscustomobject]@{ axis = $_.Name; programs = $_.Count } })
$summary = [pscustomobject]@{
    toolchain        = $Toolchain
    toolchain_version = ((& $Toolchain --version 2>&1 | Select-Object -First 1) -join "")
    toolchain_sha256 = (Get-FileHash -Path $Toolchain -Algorithm SHA256).Hash
    generated_at     = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    total            = $results.Count
    axes             = $axisCounts
    axis_count       = $axisCounts.Count
    check_pass       = @($results | Where-Object { $_.check_exit -eq 0 }).Count
    build_pass       = @($results | Where-Object { $_.verdict -eq "build-pass" }).Count
    check_rejected   = @($results | Where-Object { $_.verdict -eq "check-rejected" }).Count
    divergences      = @($results | Where-Object { $_.divergence }).Count
    results          = $results
    known_open       = $knownOpenResults
    rejected_controls = $rejectedResults
}
$summary | ConvertTo-Json -Depth 6 | Set-Content -Path $Out -Encoding UTF8

$divergentNames = @($results | Where-Object { $_.divergence } | ForEach-Object { $_.name })
Write-Host ""
Write-Host ("MATRIX: {0} programs across {1} axes; check-pass {2}; build-pass {3}; check-rejected {4}; DIVERGENCES {5}" -f `
    $summary.total, $summary.axis_count, $summary.check_pass, $summary.build_pass, $summary.check_rejected, $summary.divergences)
Write-Host ("axes: " + (($axisCounts | ForEach-Object { "$($_.axis)=$($_.programs)" }) -join ", "))
Write-Host ("report: {0}" -f $Out)
if ($summary.check_rejected -gt 0) {
    # The matrix is meant to contain only programs the checker accepts; a
    # rejection means the matrix (not the compiler) must be fixed before any
    # build result can be interpreted.
    Write-Host "MATRIX INVALID: a program expected to be accepted was rejected by check."
    exit 2
}
if ($divergentNames.Count -gt 0) {
    Write-Host "DIVERGENCES:"
    foreach ($name in $divergentNames) { Write-Host ("  - {0}" -f $name) }
    exit 1
}
exit 0
