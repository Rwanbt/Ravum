//! RFC-UI-LANG-2 §4: the real prelude `Arena<T>`, compiled by rustc with a
//! **reduced** generation limit (3) and slot limit (4), so that retirement
//! (STD-21.4 / HDL-3) and `arena is full` (STD-21.9) are reached quickly.
//! A separate test pins the real limits to the values of the spec.

use ravum_lang_codegen::prelude::PRELUDE;

const MAIN: &str = r#"
fn order(cycles: u32) -> String {
    let mut a: Arena<&'static str> = Arena::new();
    let _x = a.insert("A");
    let t = a.insert("T");
    let _z = a.insert("Z");
    assert_eq!(a.remove(t), Some("T"));
    for _ in 0..cycles {
        let h = a.insert("t");
        assert!(a.remove(h).is_some());
    }
    let _c = a.insert("C");
    assert!(!a.contains(t), "an old handle became valid again");
    let mut s = String::new();
    for (_, x) in &a {
        s.push_str(x);
    }
    s
}

fn main() {
    // Generation limit 3: two removals keep slot 1, the third retires it.
    println!("{}", order(1));
    println!("{}", order(2));
    let mut a: Arena<i64> = Arena::new();
    for i in 0..4 {
        let _ = a.insert(i);
    }
    println!("full at {}", a.len());
    let _ = a.insert(9);
}
"#;

fn rustc() -> String {
    std::env::var("RAVUM_RUSTC").unwrap_or_else(|_| "rustc".to_string())
}

#[test]
fn reduced_limits_retire_slots_and_fill_up() {
    let mut src = PRELUDE
        .replace(
            "pub const RV_ARENA_GEN_MAX: u32 = (1 << 24) - 1;",
            "pub const RV_ARENA_GEN_MAX: u32 = 3;",
        )
        .replace(
            "pub const RV_ARENA_SLOT_MAX: u32 = (1 << 20) - 1;",
            "pub const RV_ARENA_SLOT_MAX: u32 = 3;",
        );
    assert!(
        src.contains("RV_ARENA_GEN_MAX: u32 = 3;") && src.contains("RV_ARENA_SLOT_MAX: u32 = 3;")
    );
    src.push_str(MAIN);
    let dir = std::env::temp_dir().join(format!("ravum-arena-prelude-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("p.rs");
    std::fs::write(&file, src).unwrap();
    let bin = dir.join(if cfg!(windows) { "p.exe" } else { "p" });
    let out = std::process::Command::new(rustc())
        .args([
            "--edition",
            "2021",
            "-C",
            "overflow-checks=on",
            "-A",
            "warnings",
            "-o",
        ])
        .arg(&bin)
        .arg(&file)
        .output()
        .expect("rustc");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = std::process::Command::new(&bin).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert_eq!(stdout, "ACZ\nAZC\nfull at 4\n");
    assert!(stderr.contains("arena is full"), "{stderr}");
    assert!(!run.status.success());
}

#[test]
fn the_real_limits_are_the_spec_values() {
    // STD-21.4 and STD-21.9.
    assert!(PRELUDE.contains("pub const RV_ARENA_GEN_MAX: u32 = (1 << 24) - 1;"));
    assert!(PRELUDE.contains("pub const RV_ARENA_SLOT_MAX: u32 = (1 << 20) - 1;"));
    assert!(PRELUDE.contains("pub const RV_ARENA_ID_MAX: u32 = (1 << 20) - 1;"));
}
