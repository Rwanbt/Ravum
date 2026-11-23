//! T14 measure, run by hand: which single-file `run-pass` conformance
//! programs the IR lowers, and why the others are refused.
//! `cargo test -p ravum-lang-jit --test ir_refusal_census -- --ignored --nocapture`

use std::collections::BTreeMap;
#[test]
#[ignore]
fn census() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/cases");
    let mut tally: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let (mut ok, mut total) = (0, 0);
    for group in std::fs::read_dir(&root).unwrap() {
        for case in std::fs::read_dir(group.unwrap().path()).unwrap() {
            let dir = case.unwrap().path();
            let cj = std::fs::read_to_string(dir.join("case.json")).unwrap_or_default();
            if !cj.contains("\"run-pass\"") {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(dir.join("src/main.rv")) else {
                continue;
            };
            if std::fs::read_dir(dir.join("src")).unwrap().count() > 1 {
                continue;
            }
            total += 1;
            match ravum_lang_ir::compile_source(&src) {
                Ok(_) => ok += 1,
                Err(e) => {
                    let k = format!("{e:?}");
                    let k = k.chars().take(90).collect::<String>();
                    tally
                        .entry(k)
                        .or_default()
                        .push(dir.file_name().unwrap().to_string_lossy().into());
                }
            }
        }
    }
    let mut v: Vec<_> = tally.into_iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(c.len()));
    println!("lowered {ok}/{total}");
    for (k, c) in v {
        println!("{:3} {k}  e.g. {}", c.len(), c[0]);
    }
}
