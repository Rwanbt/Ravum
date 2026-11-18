use super::*;

impl<'a> Emitter<'a> {
    pub(super) fn pattern(&self, pat: &Pat) -> Result<String, CodegenError> {
        match &pat.kind {
            PatKind::Wild => Ok("_".to_string()),
            PatKind::Binding { mutable, name } => Ok(if *mutable {
                format!("mut {}", self.local_ident(&name.name))
            } else {
                self.local_ident(&name.name)
            }),
            PatKind::Lit(literal) => self.literal_pattern(pat, literal),
            PatKind::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.pattern(item)?);
                }
                if rendered.len() == 1 {
                    Ok(format!("({},)", rendered[0]))
                } else {
                    Ok(format!("({})", rendered.join(", ")))
                }
            }
            PatKind::Path(path) => match self.facts().resolutions.get(&path.id) {
                Some(Resolution::Variant { enum_item, variant }) => Ok(format!(
                    "{}::{}",
                    self.enum_path(*enum_item)?,
                    self.variant_name(*enum_item, *variant)?
                )),
                Some(Resolution::Item { item }) => self.item_path(*item),
                Some(_) => self.plain_path_text(path),
                None => Err(err(format!(
                    "missing resolution fact for pattern path `{}` (path node {})",
                    path.display(),
                    path.id
                ))),
            },
            PatKind::Variant { path, fields } => {
                let base = self.variant_pattern_base(path)?;
                match fields {
                    VariantPatFields::Tuple(items) => {
                        let mut rendered = Vec::new();
                        for item in items {
                            rendered.push(self.pattern(item)?);
                        }
                        Ok(format!("{base}({})", rendered.join(", ")))
                    }
                    VariantPatFields::Struct(fields) => {
                        if fields.is_empty() {
                            return Ok(format!("{base} {{}}"));
                        }
                        let mut rendered = Vec::new();
                        for field in fields {
                            rendered.push(format!(
                                "{}: {}",
                                mangle_ident(&field.name.name),
                                self.pattern(&field.pat)?
                            ));
                        }
                        Ok(format!("{base} {{ {} }}", rendered.join(", ")))
                    }
                }
            }
        }
    }

    pub(super) fn literal_pattern(
        &self,
        pat: &Pat,
        literal: &LitPat,
    ) -> Result<String, CodegenError> {
        Ok(match literal {
            LitPat::Int { value, suffix, neg } => {
                let text =
                    self.int_literal_text(*value, *suffix, self.facts().pat_types.get(&pat.id));
                if *neg {
                    format!("-{text}")
                } else {
                    text
                }
            }
            // G-6.36: Rust has no float literal patterns. `match_text` lowers
            // them to guards; reaching this point means a float pattern sits in
            // a position that cannot be lowered (`let`, parameter, `for`), and
            // emitting the literal here would produce Rust that `rustc` rejects.
            LitPat::Float { .. } => {
                return Err(err(
                    "float literal patterns are only supported in `match` arms",
                ));
            }
            LitPat::Bool(value) => {
                if *value {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            LitPat::Char(value) => char_literal(*value),
            LitPat::Str(value) => string_literal(value),
        })
    }

    /// Resolves the `Enum::Variant` prefix shared by the two pattern renderers.
    pub(super) fn variant_pattern_base(&self, path: &Path) -> Result<String, CodegenError> {
        match self.facts().resolutions.get(&path.id) {
            Some(Resolution::Variant { enum_item, variant }) => Ok(format!(
                "{}::{}",
                self.enum_path(*enum_item)?,
                self.variant_name(*enum_item, *variant)?
            )),
            // A variant pattern resolves to a variant or a prelude
            // constructor; the latter is written as its path.
            Some(_) => self.plain_path_text(path),
            None => Err(err(format!(
                "missing resolution fact for pattern path `{}` (path node {})",
                path.display(),
                path.id
            ))),
        }
    }

    // -- calls -------------------------------------------------------------
}
