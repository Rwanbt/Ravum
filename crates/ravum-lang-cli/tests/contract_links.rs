//! T19 (first slice): `check --json` lists every contract clause with a
//! stable id (no line number, ADR-040), its location, the tests reaching it
//! and an honest verification status (`runtime-checked`, never proved).

use std::process::Command;

const SRC: &str = "fn div(a: i64, b: i64) -> i64 effects()\n    requires b != 0\n    ensures result * b + a % b == a\n{\n    return a / b;\n}\n\nfn half(x: i64) -> i64 effects() {\n    return div(x, 2);\n}\n\nfn lonely(x: i64) -> i64 effects()\n    requires x > 0\n{\n    return x;\n}\n\nfn main() -> () effects() {\n    let _y = lonely(1);\n}\n\ntest fn halves() {\n    assert(half(8) == 4);\n}\n";

fn check_json(src: &str, tag: &str) -> serde_json::Value {
    let dir = std::env::temp_dir().join(format!("rv-contract-links-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"links\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["check", "--json"])
        .arg(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

#[test]
fn check_json_links_each_contract_to_the_tests_reaching_it() {
    let v = check_json(SRC, "a");
    assert_eq!(v["success"], true, "{v}");
    assert_eq!(v["schema_version"], 1, "additive field: schema unchanged");
    let c = v["contracts"].as_array().expect("contracts");
    let ids: Vec<&str> = c.iter().map(|x| x["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        ["div#requires.0", "div#ensures.0", "lonely#requires.0"]
    );
    // `halves` reaches `div` through `half`; nothing tests `lonely`
    // (called by `main` only): an honest empty list, not a claim.
    assert_eq!(c[0]["exercised_by"], serde_json::json!(["halves"]));
    assert_eq!(c[1]["exercised_by"], serde_json::json!(["halves"]));
    assert_eq!(c[2]["exercised_by"], serde_json::json!([]));
    assert_eq!(c[1]["text"], "result * b + a % b == a");
    for x in c {
        assert_eq!(x["verification"], "runtime-checked");
        assert_ne!(x["verification"], "proved");
    }
    assert_eq!(c[0]["span"]["start_line"], 2);

    // Stability: lines inserted above move the location, never the id.
    let moved = check_json(&format!("// header\n\n\n{SRC}"), "b");
    let c2 = moved["contracts"].as_array().unwrap();
    let ids2: Vec<&str> = c2.iter().map(|x| x["id"].as_str().unwrap()).collect();
    assert_eq!(ids2, ids);
    assert_eq!(c2[0]["span"]["start_line"], 5);
}

#[test]
fn a_program_without_contracts_has_no_contracts_field() {
    let v = check_json("fn main() -> () effects() {\n}\n", "c");
    assert!(v.get("contracts").is_none(), "{v}");
}

#[test]
fn test_json_names_the_violated_contract_of_a_failed_test() {
    let src = "fn grow(x: i64) -> i64 effects()\n    ensures result > x\n{\n    if x > 10 {\n        return x;\n    }\n    return x + 1;\n}\n\nfn main() -> () effects() {\n}\n\ntest fn small() {\n    assert(grow(1) == 2);\n}\n\ntest fn big() {\n    assert(grow(20) == 20);\n}\n\ntest fn plain() {\n    assert(1 == 2);\n}\n";
    let dir = std::env::temp_dir().join(format!("rv-contract-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"viol\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["test", "--json"])
        .arg(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let tests = v["summary"]["tests"].as_array().unwrap();
    let by = |n: &str| tests.iter().find(|t| t["name"] == n).unwrap().clone();
    assert_eq!(by("small")["status"], "ok");
    assert!(by("small").get("message").is_none());
    let big = by("big");
    assert_eq!(big["status"], "FAILED");
    assert_eq!(
        big["violated_contracts"],
        serde_json::json!(["grow#ensures.0"])
    );
    // Negative: a failure that is not a contract violation names none.
    let plain = by("plain");
    assert_eq!(plain["status"], "FAILED");
    assert!(plain.get("violated_contracts").is_none(), "{plain}");
}

#[test]
fn check_sarif_is_a_valid_sarif_log_with_results_and_contracts() {
    let src = "fn div(a: i64, b: i64) -> i64 effects()\n    requires b != 0\n{\n    return a / b;\n}\n\nfn bad() -> i64 effects() {\n    return true;\n}\n\nfn main() -> () effects() {\n}\n";
    let dir = std::env::temp_dir().join(format!("rv-sarif-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"sarif\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["check", "--sarif"])
        .arg(&dir)
        .output()
        .unwrap();
    assert!(!out.status.success(), "the program has an error");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["version"], "2.1.0");
    let run = &v["runs"][0];
    assert_eq!(run["tool"]["driver"]["name"], "ravum-lang");
    let results = run["results"].as_array().unwrap();
    let r = results
        .iter()
        .find(|r| r["ruleId"] == "E-TYP-MISMATCH")
        .expect("the type error is a result");
    assert_eq!(r["level"], "error");
    let region = &r["locations"][0]["physicalLocation"]["region"];
    assert_eq!(region["startLine"], 8);
    // Every result's rule is declared in the driver.
    let rules: Vec<&str> = run["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    for r in results {
        assert!(rules.contains(&r["ruleId"].as_str().unwrap()));
    }
    // A program with an error has no checked facts: no contract facts.
    assert!(run.get("properties").is_none(), "{run}");
    // Negative: `--sarif` and `--json` together are a usage error.
    let both = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["check", "--sarif", "--json"])
        .arg(&dir)
        .output()
        .unwrap();
    assert!(!both.status.success());
    // Without errors, the contracts are in the property bag.
    std::fs::write(
        dir.join("src/main.rv"),
        src.replace("    return true;", "    return 1;"),
    )
    .unwrap();
    let ok = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["check", "--sarif"])
        .arg(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let v: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    let c = &v["runs"][0]["properties"]["ravum.contracts"][0];
    assert_eq!(c["id"], "div#requires.0");
    assert_eq!(c["verification"], "runtime-checked");
}

/// RFC-INVARIANTS-1: an `invariant` is a contract fact (`S#invariant.n`)
/// reached by the tests that construct `S` or call a `&mut self` method; a
/// failed test names it.
#[test]
fn invariants_are_contract_facts_and_named_by_failed_tests() {
    let src = "struct Acc {\n    n: i64,\n}\n    invariant self.n >= 0\n\nimpl Acc {\n    fn take(&mut self, k: i64) -> () effects() {\n        self.n = self.n - k;\n    }\n\n    fn get(&self) -> i64 effects() {\n        return self.n;\n    }\n}\n\nfn make() -> Acc effects() {\n    return Acc { n: 3 };\n}\n\nfn main() -> () effects() {\n}\n\ntest fn builds() {\n    assert(make().get() == 3);\n}\n\ntest fn overdraws() {\n    let mut a = make();\n    a.take(5);\n}\n";
    let v = check_json(src, "inv");
    let c = v["contracts"].as_array().expect("contracts");
    assert_eq!(c.len(), 1, "{v}");
    assert_eq!(c[0]["id"], "Acc#invariant.0");
    assert_eq!(c[0]["kind"], "invariant");
    assert_eq!(c[0]["text"], "self.n >= 0");
    assert_eq!(
        c[0]["exercised_by"],
        serde_json::json!(["builds", "overdraws"])
    );
    assert_eq!(c[0]["verification"], "runtime-checked");
    let dir = std::env::temp_dir().join(format!("rv-inv-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"invt\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rv"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["test", "--json"])
        .arg(&dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let tests = v["summary"]["tests"].as_array().unwrap();
    let by = |n: &str| tests.iter().find(|t| t["name"] == n).unwrap().clone();
    assert_eq!(by("builds")["status"], "ok");
    let o = by("overdraws");
    assert_eq!(o["status"], "FAILED");
    assert_eq!(
        o["violated_contracts"],
        serde_json::json!(["Acc#invariant.0"])
    );
    assert!(
        o["message"].as_str().unwrap().contains("(self.n = -2)"),
        "{o}"
    );
}

/// RFC-INVARIANTS-1 INV-9: every write path to a field of a struct with an
/// invariant is refused outside its methods (assignment, compound
/// assignment, `&mut` borrow, mutating method on the field), allowed inside
/// them, and reading stays allowed; a struct without invariant is unchanged.
#[test]
fn field_writes_outside_the_methods_of_an_invariant_struct_are_refused() {
    let base = |inv: &str, tail: &str| {
        format!("struct R {{\n    lo: i64,\n    v: Vec<i64>,\n}}\n{inv}\nimpl R {{\n    fn bump(&mut self) -> () effects(alloc) {{\n        self.lo = self.lo + 1;\n        self.lo += 1;\n        self.v.push(1);\n        set(&mut self.lo);\n    }}\n}}\n\nfn set(x: &mut i64) -> () effects() {{\n    *x = 3;\n}}\n\nfn main() -> () effects(alloc) {{\n    let mut r = R {{ lo: 0, v: Vec::new() }};\n    r.bump();\n    let _k = r.lo + (r.v.len() as i64);\n{tail}}}\n")
    };
    let inv = "    invariant self.lo >= 0\n";
    let codes = |v: &serde_json::Value| -> Vec<String> {
        v["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["severity"] == "error")
            .map(|d| d["code"].as_str().unwrap().to_string())
            .collect()
    };
    let ok = check_json(&base(inv, ""), "fw0");
    assert_eq!(ok["success"], true, "{ok}");
    for (i, tail) in [
        "    r.lo = -1;\n",
        "    r.lo -= 5;\n",
        "    set(&mut r.lo);\n",
        "    r.v.push(2);\n",
    ]
    .iter()
    .enumerate()
    {
        let v = check_json(&base(inv, tail), &format!("fw{}", i + 1));
        assert_eq!(codes(&v), ["E-CON-FIELD-WRITE"], "{tail}: {v}");
        // Without the invariant the same program is accepted.
        let w = check_json(&base("", tail), &format!("fwn{}", i + 1));
        assert_eq!(w["success"], true, "{tail}: {w}");
    }
}
