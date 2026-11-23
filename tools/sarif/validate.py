#!/usr/bin/env python3
"""T19: validate `ravum-lang check --sarif` against the official SARIF 2.1.0
schema (OASIS, vendored: tools/sarif/sarif-schema-2.1.0.json).

Usage: python3 tools/sarif/validate.py [path/to/ravum-lang]
Needs the `jsonschema` package. Checks two generated packages (with and
without errors) and a negative control (a mutated log must be rejected).
Exit 0 only if every check holds."""
import json, os, subprocess, sys, tempfile, pathlib, copy
import jsonschema

here = pathlib.Path(__file__).resolve().parent
binary = sys.argv[1] if len(sys.argv) > 1 else str(here.parents[1] / "target/debug/ravum-lang")
schema = json.load(open(here / "sarif-schema-2.1.0.json"))
validator = jsonschema.Draft4Validator(schema)

SRC = """struct R {
    lo: i64,
}
    invariant self.lo >= 0

fn div(a: i64, b: i64) -> i64 effects()
    requires b != 0
{
    return a / b;
}

fn main() -> () effects() {
    let x: i64 = BAD;
    let unused = 1;
}
"""

def sarif(src):
    with tempfile.TemporaryDirectory() as d:
        os.makedirs(f"{d}/src")
        open(f"{d}/ravum.toml", "w").write('[package]\nname = "sarifv"\nversion = "0.1.0"\nedition = "core-1.0"\n')
        open(f"{d}/src/main.rv", "w").write(src)
        out = subprocess.run([binary, "check", "--sarif", d], capture_output=True, text=True)
        return json.loads(out.stdout)

ok = True
for name, src in [("with errors", SRC.replace("BAD", "true")), ("without errors", SRC.replace("BAD", "1"))]:
    log = sarif(src)
    errs = list(validator.iter_errors(log))
    n = len(log["runs"][0]["results"])
    print(f"{name}: {len(errs)} schema error(s), {n} result(s)")
    ok &= not errs and n > 0
    if name == "without errors":
        ok &= "ravum.contracts" in log["runs"][0].get("properties", {})
        bad = copy.deepcopy(log)
        bad["runs"][0]["results"][0]["level"] = "fatal"  # not a SARIF level
        rejected = any(True for _ in validator.iter_errors(bad))
        print(f"negative control rejected: {rejected}")
        ok &= rejected
print("PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
