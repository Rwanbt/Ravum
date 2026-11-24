//! T14 oracle: every single-file `run-pass` conformance program that the IR
//! lowers must print, in the interpreter **and** the JIT, the stdout the
//! conformance case expects (the output the rustc path is checked against),
//! and exit like it. A program the IR refuses is skipped here (it is counted
//! by `ir_refusal_census`), never reported as agreeing.

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

fn field(json: &str, key: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get(key)?.as_str().map(str::to_string)
}

#[test]
fn lowered_run_pass_programs_print_what_the_conformance_case_expects() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/cases");
    let mut checked = 0;
    let mut failures = Vec::new();
    let mut groups: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|g| g.unwrap().path())
        .collect();
    groups.sort();
    for group in groups {
        let mut cases: Vec<_> = std::fs::read_dir(&group)
            .unwrap()
            .map(|c| c.unwrap().path())
            .collect();
        cases.sort();
        for dir in cases {
            let Ok(cj) = std::fs::read_to_string(dir.join("case.json")) else {
                continue;
            };
            let v: serde_json::Value = serde_json::from_str(&cj).unwrap();
            if v["mode"] != "run-pass" || v.get("stdin").is_some() || v.get("args").is_some() {
                continue;
            }
            if std::fs::read_dir(dir.join("src"))
                .map(|d| d.count())
                .unwrap_or(0)
                != 1
            {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(dir.join("src/main.rv")) else {
                continue;
            };
            let Ok(ir) = compile_source(&src) else {
                continue;
            };
            let expected = field(&cj, "stdout_contains").unwrap_or_default();
            let (interp, iout) = run_with_output(&ir, "main", DEFAULT_STEPS);
            let (native, nout) = match compile(&ir) {
                Ok(m) => m.run_with_output("main", DEFAULT_FUEL),
                Err(e) => {
                    failures.push(format!(
                        "{}: lowered but the JIT refused it: {e:?}",
                        dir.display()
                    ));
                    continue;
                }
            };
            checked += 1;
            let name = dir.strip_prefix(&root).unwrap().display().to_string();
            if native.render() != interp.render() || nout != iout {
                failures.push(format!(
                    "{name}: interpreter {} != JIT {}",
                    interp.render(),
                    native.render()
                ));
            } else if !iout.stdout.contains(&expected) {
                failures.push(format!(
                    "{name}: stdout {:?} lacks {:?}",
                    iout.stdout, expected
                ));
            } else if interp.render().contains("panic") || interp.render().contains("fault") {
                failures.push(format!("{name}: stopped with {}", interp.render()));
            }
        }
    }
    assert!(checked >= 90, "only {checked} programs reached the oracle");
    assert!(
        failures.is_empty(),
        "{} of {checked}:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
