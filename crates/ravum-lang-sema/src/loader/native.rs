//! Native libraries (RFC-E11-SQLITE NAT-1 to NAT-3): the `[native]` section
//! of the manifest, the resolution of each library file on disk, its digest,
//! and the check that every `extern "C" from "<lib>"` block names one.
//!
//! Nothing here compiles or downloads code: a native library is provided
//! already built (NAT-1.5). The toolchain only checks that the file is the one
//! the manifest pins.

use super::*;
use ravum_lang_syntax::ast::{Item, ItemKind};
use std::collections::BTreeSet;

fn native_err(diags: &mut DiagnosticBag, code: &str, span: Span, message: impl Into<String>) {
    diags.push(Diagnostic::error(code, span, message));
}

/// The host platform as written in `{os}-{arch}` and `sha256-<os>-<arch>`.
pub(super) fn host_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// One `name = { kind = "...", path = "...", lib = "...", sha256 = "..." }`.
pub(super) fn parse_native_entry(
    name: &str,
    value: &TomlValue,
    span: Span,
    diags: &mut DiagnosticBag,
) -> Option<NativeDecl> {
    if !is_valid_package_name(name) {
        native_err(
            diags,
            "E-MOD-NATIVE",
            span,
            format!("invalid native library name `{name}`: expected `[a-z][a-z0-9_-]*`"),
        );
        return None;
    }
    let TomlValue::Table(entries) = value else {
        native_err(
            diags,
            "E-MOD-NATIVE",
            span,
            format!(
                "native library `{name}` must be `{name} = {{ kind = \"static\", path = \"...\", lib = \"...\", sha256 = \"...\" }}`"
            ),
        );
        return None;
    };
    let (mut kind, mut path, mut lib) = (None, None, None);
    let mut sha256 = Vec::new();
    for (key, text) in entries {
        match key.as_str() {
            "kind" => {
                kind = match text.as_str() {
                    "static" => Some(NativeKind::Static),
                    "dylib" => Some(NativeKind::Dylib),
                    other => {
                        native_err(
                            diags,
                            "E-MOD-NATIVE",
                            span,
                            format!("native library `{name}`: kind `{other}` is neither `static` nor `dylib`"),
                        );
                        return None;
                    }
                }
            }
            "path" => path = Some(text.clone()),
            "lib" => lib = Some(text.clone()),
            "sha256" => sha256.push((String::new(), text.clone())),
            other if other.starts_with("sha256-") => {
                sha256.push((other["sha256-".len()..].to_string(), text.clone()))
            }
            other => {
                native_err(
                    diags,
                    "E-MOD-NATIVE",
                    span,
                    format!("native library `{name}`: unknown key `{other}` (expected kind, path, lib, sha256)"),
                );
                return None;
            }
        }
    }
    let (Some(kind), Some(path), Some(lib)) = (kind, path, lib) else {
        native_err(
            diags,
            "E-MOD-NATIVE",
            span,
            format!("native library `{name}` needs `kind`, `path` and `lib`"),
        );
        return None;
    };
    if sha256.is_empty() {
        native_err(
            diags,
            "E-MOD-NATIVE",
            span,
            format!("native library `{name}` needs a `sha256` digest (NAT-1.3)"),
        );
        return None;
    }
    if let Some((_, bad)) = sha256.iter().find(|(_, hex)| !is_sha256_hex(hex)) {
        native_err(
            diags,
            "E-MOD-NATIVE",
            span,
            format!("native library `{name}`: `{bad}` is not 64 lowercase hexadecimal digits"),
        );
        return None;
    }
    if lib.is_empty()
        || !lib
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        native_err(
            diags,
            "E-MOD-NATIVE",
            span,
            format!("native library `{name}`: invalid `lib` name `{lib}`"),
        );
        return None;
    }
    let escapes = path.starts_with('/')
        || path.starts_with('\\')
        || path.contains(':')
        || path.split(['/', '\\']).any(|part| part == "..");
    if path.is_empty() || escapes {
        native_err(
            diags,
            "E-MOD-NATIVE-PATH",
            span,
            format!("native library `{name}`: `path = \"{path}\"` must stay inside the package (relative, no `..`) (NAT-1.2)"),
        );
        return None;
    }
    Some(NativeDecl {
        name: name.to_string(),
        kind,
        path,
        lib,
        sha256,
        span,
    })
}

/// The library file name `rustc`/the linker will look for.
fn library_file_name(kind: NativeKind, lib: &str) -> String {
    match (kind, std::env::consts::OS) {
        (_, "windows") => format!("{lib}.lib"),
        (NativeKind::Static, _) => format!("lib{lib}.a"),
        (NativeKind::Dylib, "macos") => format!("lib{lib}.dylib"),
        (NativeKind::Dylib, _) => format!("lib{lib}.so"),
    }
}

/// Names of the libraries named by `extern "C" from "<lib>"` in `items`,
/// with the span of each first use. Modules named in `skip` (the merged
/// dependencies, at the root) are not entered.
fn used_libraries(items: &[Item], skip: &BTreeSet<String>, out: &mut Vec<(String, Span)>) {
    for item in items {
        match &item.kind {
            ItemKind::Extern(block) => {
                if let Some(lib) = &block.lib {
                    if !out.iter().any(|(name, _)| *name == lib.name) {
                        out.push((lib.name.clone(), lib.span));
                    }
                }
            }
            ItemKind::Mod(module) => {
                let merged = module
                    .name
                    .as_ref()
                    .is_some_and(|name| skip.contains(&name.name));
                if !merged {
                    used_libraries(&module.items, &BTreeSet::new(), out);
                }
            }
            _ => {}
        }
    }
}

/// Resolve and check the native libraries a loaded program uses (MOD-5.14 to
/// MOD-5.20). Each package resolves the libraries its own `extern` blocks
/// name against its own `[native]` section, relative to its own directory; a
/// dependency's code is the module named after it at the program root.
pub(super) fn resolve_natives(
    graph: &DepGraph,
    program: &Program,
    diags: &mut DiagnosticBag,
) -> Vec<NativeLib> {
    let dependency_names: BTreeSet<String> = graph
        .packages
        .iter()
        .skip(1)
        .map(|package| package.manifest.name.clone())
        .collect();
    let mut out: Vec<NativeLib> = Vec::new();
    for (index, package) in graph.packages.iter().enumerate() {
        let mut used = Vec::new();
        if index == 0 {
            used_libraries(&program.root.items, &dependency_names, &mut used);
        } else {
            let module = program.root.items.iter().find_map(|item| match &item.kind {
                ItemKind::Mod(module)
                    if module
                        .name
                        .as_ref()
                        .is_some_and(|name| name.name == package.manifest.name) =>
                {
                    Some(module)
                }
                _ => None,
            });
            if let Some(module) = module {
                used_libraries(&module.items, &BTreeSet::new(), &mut used);
            }
        }
        for native in resolve_package_natives(&package.dir, &package.manifest, used, diags) {
            if let Some(other) = out.iter().find(|other| other.name == native.name) {
                let span = package
                    .manifest
                    .native
                    .iter()
                    .find(|decl| decl.name == native.name)
                    .map(|decl| decl.span)
                    .unwrap_or_else(Span::dummy);
                native_err(
                    diags,
                    "E-MOD-NATIVE",
                    span,
                    format!(
                        "native library `{}` is declared by two packages (`{}` and `{}`); a program links one library per name",
                        native.name, other.package, native.package
                    ),
                );
                continue;
            }
            out.push(native);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn resolve_package_natives(
    root: &Path,
    manifest: &PackageManifest,
    used: Vec<(String, Span)>,
    diags: &mut DiagnosticBag,
) -> Vec<NativeLib> {
    let platform = host_platform();
    let mut out = Vec::new();
    for (name, use_span) in used {
        let Some(decl) = manifest.native.iter().find(|decl| decl.name == name) else {
            native_err(
                diags,
                "E-FFI-UNKNOWN-LIB",
                use_span,
                format!("`extern \"C\" from \"{name}\"`: no `{name}` in the `[native]` section of the `ravum.toml` of package `{}` (NAT-2.1)", manifest.name),
            );
            continue;
        };
        let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
        let relative = decl.path.replace("{os}", os).replace("{arch}", arch);
        let dir = root.join(&relative);
        let file = dir.join(library_file_name(decl.kind, &decl.lib));
        let bytes = match fs::read(&file) {
            Ok(bytes) => bytes,
            Err(error) => {
                native_err(
                    diags,
                    "E-MOD-NATIVE-MISSING",
                    decl.span,
                    format!(
                        "native library `{name}`: cannot read `{}` for platform `{platform}`: {error}",
                        file.display()
                    ),
                );
                continue;
            }
        };
        let pinned = decl
            .sha256
            .iter()
            .find(|(p, _)| *p == platform)
            .or_else(|| decl.sha256.iter().find(|(p, _)| p.is_empty()));
        let Some((_, pinned)) = pinned else {
            native_err(
                diags,
                "E-MOD-NATIVE-MISSING",
                decl.span,
                format!("native library `{name}`: no `sha256` or `sha256-{platform}` digest for platform `{platform}`"),
            );
            continue;
        };
        let actual = sha256_hex(&bytes);
        if actual != *pinned {
            native_err(
                diags,
                "E-MOD-NATIVE-DIGEST",
                decl.span,
                format!(
                    "native library `{name}`: `{}` has digest {actual}, the manifest pins {pinned}; nothing is linked (NAT-1.3)",
                    file.display()
                ),
            );
            continue;
        }
        out.push(NativeLib {
            name: name.clone(),
            package: manifest.name.clone(),
            kind: decl.kind,
            dir,
            lib: decl.lib.clone(),
            digest: format!("sha256:{actual}"),
        });
    }
    out
}

/// Fold the native libraries into the dependency digest (NAT-2.2): a changed
/// library changes the build identity. Without natives the digest is
/// unchanged.
pub(super) fn fold_native_digest(digest: &str, natives: &[NativeLib]) -> String {
    if natives.is_empty() {
        return digest.to_string();
    }
    let mut hasher = Sha256::new();
    hasher.update(digest.as_bytes());
    for native in natives {
        hasher.update(b"\nnative\0");
        hasher.update(native.name.as_bytes());
        hasher.update(&[0]);
        hasher.update(native.kind.name().as_bytes());
        hasher.update(&[0]);
        hasher.update(native.lib.as_bytes());
        hasher.update(&[0]);
        hasher.update(native.digest.as_bytes());
    }
    format!("sha256:{}", hasher.finish_hex())
}
