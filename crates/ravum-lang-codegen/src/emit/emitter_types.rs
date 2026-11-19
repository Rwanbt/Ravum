use super::*;

impl<'a> Emitter<'a> {
    pub(super) fn type_node(&self, ty: &AstType) -> Result<String, CodegenError> {
        self.type_node_pos(ty, TypePos::Value)
    }

    /// `type_node` for an explicit position (see [`TypePos`]).
    pub(super) fn type_node_pos(&self, ty: &AstType, pos: TypePos) -> Result<String, CodegenError> {
        if let Some(resolved) = self.facts().type_nodes.get(&ty.id) {
            if !resolved.is_error() {
                return self.fmt_type_pos(resolved, pos);
            }
        }
        self.syntactic_type_pos(ty, pos)
    }

    pub(super) fn syntactic_type_pos(
        &self,
        ty: &AstType,
        pos: TypePos,
    ) -> Result<String, CodegenError> {
        match &ty.kind {
            TypeKind::Unit => Ok("()".to_string()),
            TypeKind::Never => Ok("!".to_string()),
            TypeKind::SelfType => Ok("Self".to_string()),
            TypeKind::Ref { mutable, inner } => {
                if !*mutable && self.is_str_syntax(inner) {
                    // Unresolved `&str`: apply the same position rule as
                    // `fmt_type_pos` so a hand-built program without type facts
                    // still emits valid Rust.
                    return Ok(match pos {
                        TypePos::Value => "&'static str".to_string(),
                        TypePos::Param => "&str".to_string(),
                    });
                }
                Ok(format!(
                    "&{}{}",
                    if *mutable { "mut " } else { "" },
                    // See `fmt_type_pos`: nested text types are static (D13).
                    self.syntactic_type_pos(inner, TypePos::Value)?
                ))
            }
            TypeKind::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.syntactic_type_pos(item, TypePos::Value)?);
                }
                if rendered.len() == 1 {
                    Ok(format!("({},)", rendered[0]))
                } else {
                    Ok(format!("({})", rendered.join(", ")))
                }
            }
            TypeKind::Array { elem, len } => Ok(format!(
                "[{}; {}]",
                self.syntactic_type_pos(elem, TypePos::Value)?,
                self.array_length(len)?
            )),
            TypeKind::Slice(inner) => Ok(format!(
                "[{}]",
                self.syntactic_type_pos(inner, TypePos::Value)?
            )),
            TypeKind::Path(path) => self.syntactic_path_type(path),
            TypeKind::Infer => Err(err(format!(
                "missing resolved type for inferred type node {}",
                ty.id
            ))),
        }
    }

    /// `true` when this type syntax is the bare `str` keyword (`&str` inner).
    pub(super) fn is_str_syntax(&self, ty: &AstType) -> bool {
        match &ty.kind {
            TypeKind::Path(path) => {
                path.root == PathRoot::Plain
                    && path.segments.len() == 1
                    && path.segments[0].name.name == "str"
            }
            _ => false,
        }
    }

    pub(super) fn syntactic_path_type(&self, path: &Path) -> Result<String, CodegenError> {
        if path.root != PathRoot::Plain {
            return Err(err(format!(
                "missing resolved type for rooted type path `{}`",
                path.display()
            )));
        }
        if path.segments.len() != 1 {
            return Err(err(format!(
                "missing resolved type for qualified type path `{}`",
                path.display()
            )));
        }
        let segment = &path.segments[0];
        let name = segment.name.name.as_str();
        match builtin_type_name(name) {
            Some(builtin) => {
                if segment.args.is_empty() {
                    Ok(builtin.to_string())
                } else {
                    let mut rendered = Vec::new();
                    for argument in &segment.args {
                        rendered.push(self.syntactic_type_pos(argument, TypePos::Value)?);
                    }
                    Ok(format!("{builtin}<{}>", rendered.join(", ")))
                }
            }
            None => Err(err(format!(
                "missing resolved type for nominal type `{name}` (type node {})",
                path.id
            ))),
        }
    }

    pub(super) fn array_length(&self, expr: &Expr) -> Result<String, CodegenError> {
        match &expr.kind {
            ExprKind::Int { value, .. } => Ok(value.to_string()),
            ExprKind::Path(path) => self.value_path(path),
            _ => Err(err(format!(
                "array length must be a literal integer or a constant (expression node {})",
                expr.id
            ))),
        }
    }

    pub(super) fn fmt_type(&self, ty: &Type) -> Result<String, CodegenError> {
        self.fmt_type_pos(ty, TypePos::Value)
    }

    /// Emit a semantic type in an explicit position; see [`TypePos`] for why the
    /// borrowed text type is spelled `&'static str` when owned and `&str` when
    /// passed as a parameter.
    pub(super) fn fmt_type_pos(&self, ty: &Type, pos: TypePos) -> Result<String, CodegenError> {
        Ok(match ty {
            Type::I8 => "i8".to_string(),
            Type::I16 => "i16".to_string(),
            Type::I32 => "i32".to_string(),
            Type::I64 => "i64".to_string(),
            Type::I128 => "i128".to_string(),
            Type::U8 => "u8".to_string(),
            Type::U16 => "u16".to_string(),
            Type::U32 => "u32".to_string(),
            Type::U64 => "u64".to_string(),
            Type::U128 => "u128".to_string(),
            Type::Isize => "isize".to_string(),
            Type::Usize => "usize".to_string(),
            Type::F32 => "f32".to_string(),
            Type::F64 => "f64".to_string(),
            Type::Bool => "bool".to_string(),
            Type::Char => "char".to_string(),
            Type::Unit => "()".to_string(),
            Type::Never => "!".to_string(),
            // `str` is unsized: it is legal in Core 1.0 media only behind the
            // `&str` view, which `Type::Ref` handles below.
            Type::Str => "str".to_string(),
            Type::String => "String".to_string(),
            Type::Ref { mutable, inner } => {
                if matches!(**inner, Type::Str) {
                    if *mutable {
                        // resolve.rs rejects `&mut str` with E-RES-UNKNOWN, so a
                        // checked program can never contain it; fail closed
                        // instead of guessing a Rust spelling.
                        return Err(err(
                            "internal error: `&mut str` has no Core 1.0 representation",
                        ));
                    }
                    return Ok(match pos {
                        // OWN-6.2: the only storable/returnable reference.
                        TypePos::Value => "&'static str".to_string(),
                        // STD-9.3 / TYP-13.1: a parameter accepts the
                        // instruction-scoped view from `String::as_str`.
                        TypePos::Param => "&str".to_string(),
                    });
                }
                format!(
                    "&{}{}",
                    if *mutable { "mut " } else { "" },
                    // A text type nested under another type constructor is
                    // always static (D13); only the parameter's own `&str` may
                    // be the elided, instruction-scoped view.
                    self.fmt_type_pos(inner, TypePos::Value)?
                )
            }
            Type::Array { elem, len } => {
                format!("[{}; {len}]", self.fmt_type_pos(elem, TypePos::Value)?)
            }
            Type::Slice(inner) => format!("[{}]", self.fmt_type_pos(inner, TypePos::Value)?),
            Type::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.fmt_type_pos(item, TypePos::Value)?);
                }
                if rendered.len() == 1 {
                    format!("({},)", rendered[0])
                } else {
                    format!("({})", rendered.join(", "))
                }
            }
            Type::Vec(inner) => format!("Vec<{}>", self.fmt_type_pos(inner, TypePos::Value)?),
            Type::Option(inner) => format!("Option<{}>", self.fmt_type_pos(inner, TypePos::Value)?),
            Type::Result(ok, error) => {
                format!(
                    "Result<{}, {}>",
                    self.fmt_type_pos(ok, TypePos::Value)?,
                    self.fmt_type_pos(error, TypePos::Value)?
                )
            }
            Type::Nominal { def, name, args } => {
                let base = self.nominal_path(*def, name)?;
                if args.is_empty() {
                    base
                } else {
                    let mut rendered = Vec::new();
                    for argument in args {
                        rendered.push(self.fmt_type_pos(argument, TypePos::Value)?);
                    }
                    format!("{base}<{}>", rendered.join(", "))
                }
            }
            Type::Param { name, .. } => mangle_ident(name),
            Type::Error => {
                return Err(err(
                    "internal error: attempted to generate the recovered error type",
                ));
            }
        })
    }

    pub(super) fn nominal_path(&self, def: NodeId, name: &str) -> Result<String, CodegenError> {
        if let Some(entry) = self.index.items.get(&def) {
            return Ok(entry.rust_path.clone());
        }
        if let Some(raw) = self.facts().item_paths.get(&def) {
            return Ok(self.rewrite_fact_path(raw));
        }
        if name == "Error" {
            return Ok("crate::Error".to_string());
        }
        // Opaque prelude types of `std::net`, `std::thread` and `std::sync`
        // (RFC-E11-NET-HTTP §3, RFC-E11-CONCURRENCY §3).
        if let Some(opaque) = ravum_lang_sema::builtins::prelude_opaque_name(def) {
            return Ok(format!("crate::{opaque}"));
        }
        Err(err(format!(
            "cannot resolve nominal type `{name}` (definition node {def}) to a Rust path"
        )))
    }

    // -- patterns ----------------------------------------------------------
}
