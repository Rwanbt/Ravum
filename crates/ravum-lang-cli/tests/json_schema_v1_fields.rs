//! DIAG-7.3 / DIAG-9.3: a producer never removes or renames a field without
//! incrementing `schema_version`. The fields of schema version 1 are frozen
//! here; while the CLI reports `schema_version: 1`, every one of them must be
//! present with its type. Adding a field is allowed (it is not checked).

use std::fs;
use std::process::Command;

const ENVELOPE_V1: &[(&str, &str)] = &[
    ("schema_version", "number"),
    ("tool", "string"),
    ("tool_version", "string"),
    ("spec_version", "string"),
    ("command", "string"),
    ("status", "string"),
    ("exit_code", "number"),
    ("success", "bool"),
    ("target", "object"),
    ("profile", "string"),
    ("dependencies_digest", "string"),
    ("summary", "object"),
    ("diagnostics", "array"),
];

const DIAGNOSTIC_V1: &[(&str, &str)] = &[
    ("code", "string"),
    ("severity", "string"),
    ("phase", "string"),
    ("message", "string"),
    ("file", "string"),
    ("line", "number"),
    ("column", "number"),
    ("end_line", "number"),
    ("end_column", "number"),
    ("primary", "object"),
    ("labels", "array"),
    ("notes", "array"),
];

fn kind(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn missing(obj: &serde_json::Value, fields: &[(&str, &str)]) -> Vec<String> {
    fields
        .iter()
        .filter(|(name, ty)| obj.get(*name).map(kind) != Some(*ty))
        .map(|(name, ty)| format!("{name}: expected {ty}, got {:?}", obj.get(*name).map(kind)))
        .collect()
}

#[test]
fn schema_version_1_keeps_every_field_it_published() {
    let dir = std::env::temp_dir().join(format!("ravum-json-v1-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("ravum.toml"),
        "[package]\nname = \"jsonv1\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/main.rv"),
        "fn main() {\n    let x: i32 = true;\n}\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ravum-lang"))
        .args(["check", "--json", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("one JSON object");
    assert_eq!(
        report["schema_version"], 1,
        "schema_version changed: update the frozen field lists of the new version"
    );
    let mut problems = missing(&report, ENVELOPE_V1);
    let diags = report["diagnostics"].as_array().expect("diagnostics array");
    assert!(!diags.is_empty(), "the fixture must produce a diagnostic");
    for d in diags {
        problems.extend(missing(d, DIAGNOSTIC_V1));
    }
    assert!(
        problems.is_empty(),
        "schema 1 lost fields:\n{}",
        problems.join("\n")
    );
}
