use super::*;

impl<'a> Emitter<'a> {
    pub(super) fn value_path(&self, path: &Path) -> Result<String, CodegenError> {
        match self.facts().resolutions.get(&path.id) {
            Some(Resolution::Local { name, .. }) => Ok(mangle_ident(name)),
            Some(Resolution::Item { item }) => self.item_path(*item),
            Some(Resolution::Variant { enum_item, variant }) => {
                match self.variant_name_opt(*enum_item, *variant) {
                    Some(name) => Ok(format!("{}::{}", self.enum_path(*enum_item)?, name)),
                    None => self.plain_path_text(path),
                }
            }
            Some(Resolution::BuiltinFn { path }) => self.builtin_fn_value(path),
            Some(Resolution::BuiltinConst { path }) => builtin_const(path).ok_or_else(|| {
                err(format!(
                    "unsupported builtin constant `{path}` (path node {})",
                    path
                ))
            }),
            Some(Resolution::Module) | Some(Resolution::BuiltinModule) => Err(err(format!(
                "module path `{}` used as a value (path node {})",
                path.display(),
                path.id
            ))),
            Some(Resolution::Error) => Err(err(format!(
                "path `{}` is unresolved in this checked program (path node {})",
                path.display(),
                path.id
            ))),
            None => Err(err(format!(
                "missing resolution fact for path `{}` (path node {})",
                path.display(),
                path.id
            ))),
        }
    }

    pub(super) fn builtin_fn_value(&self, path: &str) -> Result<String, CodegenError> {
        Ok(match builtin_free(path) {
            Some(FreeBuiltin::Println) => "crate::rv_println".to_string(),
            Some(FreeBuiltin::Print) => "crate::rv_print".to_string(),
            Some(FreeBuiltin::Eprintln) => "crate::rv_eprintln".to_string(),
            Some(FreeBuiltin::Eprint) => "crate::rv_eprint".to_string(),
            Some(FreeBuiltin::ReadLine) => "crate::rv_read_line".to_string(),
            Some(FreeBuiltin::Args) => "crate::rv_args".to_string(),
            Some(FreeBuiltin::EnvVar) => "crate::rv_env_var".to_string(),
            Some(FreeBuiltin::FsReadToString) => "crate::rv_fs_read_to_string".to_string(),
            Some(FreeBuiltin::FsWriteString) => "crate::rv_fs_write_string".to_string(),
            Some(FreeBuiltin::FsExists) => "crate::rv_fs_exists".to_string(),
            Some(FreeBuiltin::Exit) => "crate::rv_exit".to_string(),
            Some(FreeBuiltin::Panic) => "crate::rv_panic".to_string(),
            Some(FreeBuiltin::Assert) => "crate::rv_assert".to_string(),
            Some(FreeBuiltin::AssertEq) => "crate::rv_assert_eq".to_string(),
            Some(FreeBuiltin::Drop) => "drop".to_string(),
            Some(FreeBuiltin::StringFrom) => "String::from".to_string(),
            Some(FreeBuiltin::ErrorNew) => "crate::Error::new".to_string(),
            Some(FreeBuiltin::RustPath(name)) => name.to_string(),
            Some(FreeBuiltin::ParseI64) | Some(FreeBuiltin::ParseF64) => {
                return Err(err(format!(
                    "builtin `{path}` is only supported when called directly"
                )));
            }
            Some(FreeBuiltin::MathUnary(method)) => format!("f64::{method}"),
            Some(FreeBuiltin::MathBinary(method)) => format!("f64::{method}"),
            Some(FreeBuiltin::VariantCtor) => {
                let segments: Vec<&str> = path.split("::").collect();
                segments[segments.len() - 1].to_string()
            }
            None => {
                return Err(err(format!("unsupported builtin function `{path}`")));
            }
        })
    }

    /// Path of a nominal type used as a value (`Point`, `E::V`, `Ok`).
    pub(super) fn plain_path_text(&self, path: &Path) -> Result<String, CodegenError> {
        if path.root != PathRoot::Plain {
            return Err(err(format!(
                "cannot emit rooted path `{}` without a resolved item (path node {})",
                path.display(),
                path.id
            )));
        }
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| mangle_ident(&segment.name.name))
            .collect();
        Ok(segments.join("::"))
    }

    pub(super) fn struct_literal_path(&self, path: &Path) -> Result<String, CodegenError> {
        match self.facts().resolutions.get(&path.id) {
            Some(Resolution::Item { item }) => self.item_path(*item),
            Some(Resolution::Variant { enum_item, variant }) => {
                match self.variant_name_opt(*enum_item, *variant) {
                    Some(name) => Ok(format!("{}::{}", self.enum_path(*enum_item)?, name)),
                    None => self.plain_path_text(path),
                }
            }
            Some(_) => self.plain_path_text(path),
            None => Err(err(format!(
                "missing resolution fact for struct literal path `{}` (path node {})",
                path.display(),
                path.id
            ))),
        }
    }

    pub(super) fn enum_path(&self, enum_item: NodeId) -> Result<String, CodegenError> {
        self.item_path(enum_item)
    }

    pub(super) fn variant_name_opt(&self, enum_item: NodeId, variant: usize) -> Option<String> {
        if let Some(names) = self.index.enum_variants.get(&enum_item) {
            if let Some(name) = names.get(variant) {
                return Some(name.clone());
            }
        }
        self.facts()
            .enums
            .get(&enum_item)
            .and_then(|info| info.variants.get(variant))
            .map(|info| info.rust_name.clone())
    }

    /// Rust path of an item. `Facts::item_paths` is used when it is consistent
    /// with the module tree emitted from the AST; the AST tree wins on conflict
    /// because only it is guaranteed to match the emitted modules.
    pub(super) fn item_path(&self, id: NodeId) -> Result<String, CodegenError> {
        let computed = self
            .index
            .items
            .get(&id)
            .map(|entry| entry.rust_path.clone());
        if let Some(raw) = self.facts().item_paths.get(&id) {
            let rewritten = self.rewrite_fact_path(raw);
            return match computed {
                Some(path) => Ok(path),
                None => Ok(rewritten),
            };
        }
        computed.ok_or_else(|| err(format!("missing Rust path for item node {id}")))
    }

    pub(super) fn rewrite_fact_path(&self, raw: &str) -> String {
        let mut segments: Vec<&str> = raw.split("::").filter(|s| !s.is_empty()).collect();
        if segments.first() == Some(&"crate") {
            segments.remove(0);
        }
        let mut out = String::from("crate::rvm_root");
        let mut consumed = 0usize;
        let mut module_path = String::new();
        while consumed + 1 < segments.len() {
            let candidate = if module_path.is_empty() {
                segments[consumed].to_string()
            } else {
                format!("{module_path}::{}", segments[consumed])
            };
            if self.index.module_paths.contains(&candidate) {
                out.push_str("::rvm_");
                out.push_str(&sanitize_segment(segments[consumed]));
                module_path = candidate;
                consumed += 1;
            } else {
                break;
            }
        }
        for segment in &segments[consumed..] {
            out.push_str("::");
            out.push_str(&mangle_ident(segment));
        }
        out
    }

    // -- types -------------------------------------------------------------
}
