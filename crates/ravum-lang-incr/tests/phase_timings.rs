//! Measurement only (ignored by default): per-phase cost of a whole-package
//! check on a synthetic file, to decide what step 2 of T13 must make per-item.
//! Run: cargo test -p ravum-lang-incr --release --test phase_timings -- --ignored --nocapture

use ravum_lang_sema::syntax::{parser, SourceMap, LANGUAGE_EDITION};
use ravum_lang_sema::{check_with_timings, CheckOptions, Profile};

fn synthetic(fns: usize) -> String {
    let mut s = String::new();
    for i in 0..fns {
        s.push_str(&format!(
            "fn f{i}(x: i64, n: i64) -> i64 {{\n    let mut acc = x;\n    for k in 0..n {{\n        acc = acc + k;\n    }}\n    if acc > 10 {{\n        acc = acc - 1;\n    }}\n    let mut t = String::from(\"k\");\n    t.push_str(\"v\");\n    acc\n}}\n"
        ));
    }
    s.push_str("fn main() {\n    let r = f0(1, 2);\n}\n");
    s
}

#[test]
#[ignore]
fn phase_timings() {
    for fns in [50usize, 500, 2500] {
        let src = synthetic(fns);
        let lines = src.lines().count();
        let mut sources = SourceMap::new();
        let file = sources.add("src/main.rv", src.as_str());
        let started = std::time::Instant::now();
        let parsed = parser::parse_file(file, &src);
        let parse = started.elapsed();
        assert!(parsed.diags.is_empty());
        let options = CheckOptions {
            profile: Profile::Core,
            edition: LANGUAGE_EDITION.to_string(),
        };
        let mut best = None;
        for _ in 0..5 {
            let (r, t) = check_with_timings(&parsed.program, &sources, &options);
            assert!(
                r.checked.is_some(),
                "{:?}",
                r.diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
            );
            best = Some(t);
        }
        eprintln!(
            "lines={lines} fns={fns} parse={parse:?} {:?}",
            best.unwrap()
        );
    }
}
