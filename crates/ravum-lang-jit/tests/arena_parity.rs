//! RFC-UI-LANG-2 P2 (STD-21): the `arena-*` run-pass conformance programs on
//! path C. The IR interpreter and the JIT must print exactly what the case
//! expects (computed by path B, rustc) and agree with each other.

use ravum_lang_ir::compile_source;
use ravum_lang_ir::interp::{run_with_output, DEFAULT_STEPS};
use ravum_lang_jit::{compile, DEFAULT_FUEL};

fn field(json: &str, key: &str) -> Option<String> {
    let start = json.find(&format!("\"{key}\""))?;
    let rest = &json[start + key.len() + 2..];
    let open = rest.find('"')? + 1;
    let mut out = String::new();
    let mut chars = rest[open..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

#[test]
fn arena_programs_agree_on_path_c() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/cases/run");
    let mut seen = 0;
    for case in std::fs::read_dir(root).unwrap() {
        let dir = case.unwrap().path();
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        if !name.starts_with("arena-") {
            continue;
        }
        let json = std::fs::read_to_string(dir.join("case.json")).unwrap();
        let src = std::fs::read_to_string(dir.join("src/main.rv")).unwrap();
        let ir = match compile_source(&src) {
            Ok(ir) => ir,
            // Threads are outside the IR (every `std::thread` program is
            // refused today); this case stays on path B.
            Err(_) if name == "arena-handle-in-message" => continue,
            Err(e) => panic!("{name}: {e:?}"),
        };
        let (i, io) = run_with_output(&ir, "main", 100 * DEFAULT_STEPS);
        let (n, no) = compile(&ir)
            .expect("JIT")
            .run_with_output("main", DEFAULT_FUEL);
        assert_eq!(n.render(), i.render(), "{name}");
        assert_eq!(no, io, "{name}");
        match field(&json, "mode").as_deref() {
            Some("run-pass") => {
                let want = field(&json, "stdout_contains").unwrap();
                assert!(
                    io.stdout.contains(&want),
                    "{name}: {:?} / {}",
                    io.stdout,
                    i.render()
                );
            }
            Some("run-fail") => {
                let want = field(&json, "stderr_contains").unwrap();
                let want = want.trim_start_matches("panic: ");
                assert!(i.render().contains(want), "{name}: {}", i.render());
            }
            m => panic!("{name}: mode {m:?}"),
        }
        seen += 1;
    }
    assert!(seen >= 2, "{seen}");
}

/// STD-21.10 / INV-05: an arena touches the heap; a module with a
/// real-time entry that uses one is refused, so no arena can live in a
/// migrated real-time state (RFC-UI-LANG-2 §2.2: the migration is refused,
/// fail-closed, never a silently emptied arena).
#[test]
fn a_real_time_module_with_an_arena_is_refused() {
    use ravum_lang_jit::hot::{HotEngine, Refusal};
    let src = "fn count() -> usize effects(alloc) {\n    let mut a: Arena<i64> = Arena::new();\n    let _h = a.insert(1i64);\n    a.len()\n}\nfn gain(x: f32) -> f32 effects() {\n    return x;\n}\n";
    let ir = compile_source(src).unwrap();
    assert!(ir
        .functions
        .iter()
        .flat_map(|f| f.blocks.iter().flat_map(|b| b.insts.iter()))
        .any(|i| i.touches_heap_or_io()));
    match HotEngine::start(&ir, "gain") {
        Err(Refusal::Invalid(m)) => assert!(m.contains("INV-05"), "{m}"),
        Err(other) => panic!("expected Invalid, got {other:?}"),
        Ok(_) => panic!("expected a refusal"),
    }
}

/// HDL-3 on path C at the heap level: the limits are those of the spec and
/// of the rustc prelude, and the packed handle orders as (arena, slot,
/// generation).
#[test]
fn heap_limits_and_handle_order() {
    use ravum_lang_ir::heap::{
        pack_handle, unpack_handle, ARENA_GEN_MAX, ARENA_ID_MAX, ARENA_SLOT_MAX,
    };
    assert_eq!(
        (ARENA_ID_MAX, ARENA_SLOT_MAX, ARENA_GEN_MAX),
        ((1 << 20) - 1, (1 << 20) - 1, (1 << 24) - 1)
    );
    let h = pack_handle(ARENA_ID_MAX, ARENA_SLOT_MAX, ARENA_GEN_MAX);
    assert_eq!(
        unpack_handle(h),
        (ARENA_ID_MAX, ARENA_SLOT_MAX, ARENA_GEN_MAX)
    );
    assert!(pack_handle(1, 9, 9) < pack_handle(2, 0, 0));
    assert!(pack_handle(1, 1, 9) < pack_handle(1, 2, 0));
    assert!(pack_handle(1, 1, 1) < pack_handle(1, 1, 2));
}
