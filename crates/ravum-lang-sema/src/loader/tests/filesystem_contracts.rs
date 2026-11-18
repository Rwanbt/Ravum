use super::*;

#[test]
fn dependency_path_policy_refuses_empty_nul_and_unbounded_roots() {
    let temp = TempDir::new("dependency-path-policy");
    for (start, path, code, message) in [
        (temp.path(), "  ", "E-MOD-DEPENDENCY", "empty path"),
        (temp.path(), "ar\0g", "E-MOD-PATH-ESCAPE", "NUL byte"),
        (Path::new(""), "util", "E-MOD-PATH-ESCAPE", "has no parent"),
    ] {
        let mut diags = DiagnosticBag::new();
        let dep = DependencyDecl {
            name: "util".into(),
            path: path.into(),
            span: Span::dummy(),
        };
        assert_eq!(resolve_dependency_dir(start, &dep, &mut diags), None);
        assert_eq!(codes(&diags), [code]);
        assert!(diags.iter().any(|diag| diag.message.contains(message)));
    }
}

#[cfg(unix)]
#[test]
fn standalone_module_discovery_refuses_direct_and_intermediate_symlinks() {
    use std::os::unix::fs::symlink;

    for direct in [true, false] {
        let temp = TempDir::new("module-symlink");
        let root = temp.file("main.rv", "mod util; fn main() {}");
        let target = temp.file("real/mod.rv", "pub fn value() -> i32 { 1 }");
        if direct {
            symlink(&target, temp.path().join("util.rv")).unwrap();
        } else {
            symlink(target.parent().unwrap(), temp.path().join("util")).unwrap();
        }
        let mut cache = ParseCache::new();
        let loaded = load_single_file(&root, &LoadOptions::default(), &mut cache);
        assert!(loaded.program.is_none());
        assert_eq!(codes(&loaded.diags), ["E-MOD-PATH"]);
        assert!(loaded
            .diags
            .iter()
            .any(|diag| diag.message.contains("symbolic link")));
    }
}

#[test]
fn explicit_crate_loading_refuses_a_missing_root_or_one_outside_src() {
    let temp = TempDir::new("crate-root-boundary");
    let file = temp.file("outside.rv", "fn main() {}");
    let directory = temp.path().join("src");
    fs::create_dir(&directory).unwrap();
    let src_root = directory.canonicalize().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let manifest = parse_manifest(&manifest("app", ""), 0).0.unwrap();
    let graph = DepGraph::new(&root, manifest);

    for (file, code) in [
        (file.canonicalize().unwrap(), "E-MOD-PATH-ESCAPE"),
        (src_root.join("missing.rv"), "E-IO"),
    ] {
        let mut sources = SourceMap::new();
        let mut diags = DiagnosticBag::new();
        let mut parse_time = Duration::ZERO;
        let loaded = load_crate_program(
            &file,
            &root,
            &src_root,
            &graph,
            &mut sources,
            &mut diags,
            &mut BTreeMap::new(),
            &mut 0,
            &mut ((u64::from(SYNTHETIC_FILE) << 32) | 1),
            &mut parse_time,
            &mut ParseCache::new(),
            &LoadOptions::default(),
        );
        assert!(loaded.is_none());
        assert_eq!(codes(&diags), [code]);
    }
}

#[test]
fn dependency_cannot_displace_a_local_module() {
    let temp = TempDir::new("module-dependency-collision");
    temp.file(
        "app/ravum.toml",
        &manifest("app", "\n[dependencies]\nutil = { path = \"../util\" }\n"),
    );
    temp.file("app/src/main.rv", "mod util {} fn main() {}");
    temp.file("util/ravum.toml", &manifest("util", ""));
    temp.file("util/src/lib.rv", "pub fn value() -> i32 { 1 }");

    let loaded = load_package(&temp.path().join("app"));
    assert!(loaded.program.is_none());
    assert_eq!(codes(&loaded.diags), ["E-MOD-DUPLICATE"]);
    assert!(loaded
        .diags
        .iter()
        .any(|diag| diag.message.contains("collides with a local module")));
}

#[cfg(unix)]
#[test]
fn package_module_discovery_refuses_each_symlink_layout() {
    use std::os::unix::fs::symlink;

    for (link, target, contents) in [
        ("src/util.rv", "real/lib.rv", "mod util; fn main() {}"),
        ("src/util/mod.rv", "real/lib.rv", "mod util; fn main() {}"),
        ("src/outer", "real", "mod outer { mod lib; } fn main() {}"),
    ] {
        let temp = TempDir::new("package-module-symlink");
        temp.file("ravum.toml", &manifest("app", ""));
        temp.file("src/main.rv", contents);
        temp.file("real/lib.rv", "pub fn value() -> i32 { 1 }");
        let link = temp.path().join(link);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(temp.path().join(target), &link).unwrap();

        let loaded = load_package(temp.path());
        assert!(loaded.program.is_none());
        assert_eq!(codes(&loaded.diags), ["E-MOD-PATH"], "{}", link.display());
    }
}

#[test]
fn source_registration_refuses_an_occupied_allocation_slot() {
    let mut sources = SourceMap::new();
    let mut ids = BTreeMap::new();
    let mut diags = DiagnosticBag::new();
    let mut next = 4;
    assert!(sources.add_at(next, "old.rv", "old").is_some());
    assert_eq!(
        register_source(
            &mut sources,
            &mut ids,
            &mut next,
            Path::new("new.rv"),
            "new.rv".into(),
            "new".into(),
            &mut diags,
        ),
        None
    );
    assert!(ids.is_empty());
    assert_eq!(sources.file(4).unwrap().text, "old");
    assert_eq!(codes(&diags), ["E-IO"]);
}
