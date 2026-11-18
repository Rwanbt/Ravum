use super::*;

impl<'a> Emitter<'a> {
    pub(super) fn value_path(&self, path: &Path) -> Result<String, CodegenError> {
        match self.facts().resolutions.get(&path.id) {
            Some(Resolution::Local { name, .. }) => Ok(self.local_ident(name)),
            Some(Resolution::Item { item }) => self.item_path(*item),
            Some(Resolution::Variant { enum_item, variant }) => Ok(format!(
                "{}::{}",
                self.enum_path(*enum_item)?,
                self.variant_name(*enum_item, *variant)?
            )),
            Some(Resolution::BuiltinFn { path: builtin }) => Err(err(format!(
                "builtin function `{builtin}` must be called (path node {})",
                path.id
            ))),
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
            Some(Resolution::Variant { enum_item, variant }) => Ok(format!(
                "{}::{}",
                self.enum_path(*enum_item)?,
                self.variant_name(*enum_item, *variant)?
            )),
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

    /// Rust name of a variant. `Index::build` records the variants of every
    /// enum item of the program, and the checker resolves a variant only to an
    /// index of its enum, so a miss is a defect upstream: refuse it.
    pub(super) fn variant_name(
        &self,
        enum_item: NodeId,
        variant: usize,
    ) -> Result<String, CodegenError> {
        self.index
            .enum_variants
            .get(&enum_item)
            .and_then(|names| names.get(variant))
            .cloned()
            .ok_or_else(|| {
                err(format!(
                    "unknown variant {variant} of enum node {enum_item}"
                ))
            })
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
