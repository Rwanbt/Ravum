//! MACH-1.3, MACH-8.1, MACH-9.3: the machine reuses neither the parser, the
//! checker nor the code generator, and has no dependency (hence no network).

use std::fs;
use std::path::Path;

#[test]
fn the_crate_has_no_dependency() {
    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let deps = manifest
        .split("[dependencies]")
        .nth(1)
        .expect("a [dependencies] table");
    let lines: Vec<&str> = deps
        .lines()
        .map(str::trim)
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert!(lines.is_empty(), "dependencies: {lines:?}");
    assert!(!manifest.contains("[dev-dependencies]"));
}

#[test]
fn no_source_names_a_compiler_crate() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        for forbidden in [
            "ravum_lang",
            "ravum-lang-sema",
            "ravum-lang-syntax",
            "ravum-lang-codegen",
        ] {
            let code: String = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                !code.contains(forbidden),
                "{} uses {forbidden}",
                path.display()
            );
        }
    }
}
