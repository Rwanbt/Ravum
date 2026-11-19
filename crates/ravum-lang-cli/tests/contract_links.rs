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
