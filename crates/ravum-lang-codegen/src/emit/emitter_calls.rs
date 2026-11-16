use super::*;

impl<'a> Emitter<'a> {
    pub(super) fn call_args(&self, args: &[Expr], indent: usize) -> Result<String, CodegenError> {
        let mut rendered = Vec::new();
        for argument in args {
            rendered.push(self.expr(argument, indent)?);
        }
        Ok(rendered.join(", "))
    }

    pub(super) fn call_expr(
        &self,
        expr: &Expr,
        callee: &Expr,
        args: &[Expr],
        indent: usize,
    ) -> Result<String, CodegenError> {
        let target = self.facts().calls.get(&expr.id).ok_or_else(|| {
            err(format!(
                "missing call fact for call expression (expression node {})",
                expr.id
            ))
        })?;
        match target {
            CallTarget::UserFn { item } => Ok(format!(
                "{}({})",
                self.item_path(*item)?,
                self.call_args(args, indent)?
            )),
            CallTarget::BuiltinFn { path } => self.builtin_free_call(expr, path, args, indent),
            CallTarget::VariantCtor { enum_item, variant } => {
                let name = self.variant_name_opt(*enum_item, *variant);
                let constructor = match name {
                    Some(name) => format!("{}::{}", self.enum_path(*enum_item)?, name),
                    None => match &callee.kind {
                        ExprKind::Path(path) => self.plain_path_text(path)?,
                        _ => {
                            return Err(err(format!(
                                "unresolved variant constructor for call expression (expression node {})",
                                expr.id
                            )));
                        }
                    },
                };
                let rendered = self.call_args(args, indent)?;
                let is_unit = self.facts().enums.get(enum_item).is_some_and(|info| {
                    matches!(
                        info.variants.get(*variant).map(|variant| &variant.kind),
                        Some(VariantKind::Unit)
                    )
                });
                if rendered.is_empty() && is_unit {
                    Ok(constructor)
                } else {
                    Ok(format!("{constructor}({rendered})"))
                }
            }
            CallTarget::Error => Err(err(format!(
                "call expression (node {}) has an unresolved target",
                expr.id
            ))),
        }
    }

    pub(super) fn builtin_free_call(
        &self,
        expr: &Expr,
        path: &str,
        args: &[Expr],
        indent: usize,
    ) -> Result<String, CodegenError> {
        let spec = builtin_free(path).ok_or_else(|| {
            err(format!(
                "unsupported builtin function `{path}` (call node {})",
                expr.id
            ))
        })?;
        let (file, line) = self.location(expr.span);
        let rendered = self.call_args(args, indent)?;
        let first = args.first();
        match spec {
            FreeBuiltin::Println => Ok(format!("crate::rv_println({})", self.str_arg(first, indent)?)),
            FreeBuiltin::Print => Ok(format!("crate::rv_print({})", self.str_arg(first, indent)?)),
            FreeBuiltin::Eprintln => {
                Ok(format!("crate::rv_eprintln({})", self.str_arg(first, indent)?))
            }
            FreeBuiltin::Eprint => Ok(format!("crate::rv_eprint({})", self.str_arg(first, indent)?)),
            FreeBuiltin::ReadLine => {
                self.expect_arity(path, args, 0)?;
                Ok("crate::rv_read_line()".to_string())
            }
            FreeBuiltin::Args => {
                self.expect_arity(path, args, 0)?;
                Ok("crate::rv_args()".to_string())
            }
            FreeBuiltin::EnvVar => Ok(format!("crate::rv_env_var({})", self.str_arg(first, indent)?)),
            FreeBuiltin::FsReadToString => Ok(format!(
                "crate::rv_fs_read_to_string({})",
                self.str_arg(first, indent)?
            )),
            FreeBuiltin::FsWriteString => {
                let path_arg = self.nth_arg(args, 0, indent)?;
                let contents_arg = self.nth_arg(args, 1, indent)?;
                Ok(format!(
                    "crate::rv_fs_write_string({}, {})",
                    path_arg, contents_arg
                ))
            }
            FreeBuiltin::FsExists => Ok(format!(
                "crate::rv_fs_exists({})",
                self.str_arg(first, indent)?
            )),
            FreeBuiltin::Exit => Ok(format!("crate::rv_exit({rendered})")),
            FreeBuiltin::Panic => {
                self.expect_arity(path, args, 1)?;
                Ok(format!("crate::rv_panic({})", self.str_arg(first, indent)?))
            }
            FreeBuiltin::Assert => {
                self.expect_arity(path, args, 1)?;
                Ok(format!(
                    "crate::rv_assert({}, {}, {})",
                    self.expr(&args[0], indent)?,
                    string_literal(&file),
                    line
                ))
            }
            FreeBuiltin::AssertEq => {
                self.expect_arity(path, args, 2)?;
                // Ravum compares text values of different Rust types (`String`
                // versus `&str`); Rust's cross `PartialEq` impls cover that with
                // the `_cross` helper. Same types keep the simple `Debug` form.
                let heterogeneous = match (
                    self.facts().expr_types.get(&args[0].id),
                    self.facts().expr_types.get(&args[1].id),
                ) {
                    (Some(left), Some(right)) => left != right,
                    _ => false,
                };
                let helper = if heterogeneous {
                    "crate::rv_assert_eq_cross"
                } else {
                    "crate::rv_assert_eq"
                };
                Ok(format!(
                    "{helper}(&({}), &({}), {}, {})",
                    self.expr(&args[0], indent)?,
                    self.expr(&args[1], indent)?,
                    string_literal(&file),
                    line
                ))
            }
            FreeBuiltin::Drop => Ok(format!("drop({rendered})")),
            FreeBuiltin::StringFrom => Ok(format!(
                "String::from({})",
                self.str_arg(first, indent)?
            )),
            FreeBuiltin::ErrorNew => Ok(format!(
                "crate::Error::new({})",
                self.str_arg(first, indent)?
            )),
            FreeBuiltin::RustPath(name) => Ok(format!("{name}({rendered})")),
            FreeBuiltin::ParseI64 => Ok(format!(
                "({}).trim().parse::<i64>().map_err(|rv_error| crate::Error::new(&rv_error.to_string()))",
                self.str_borrow(first, indent)?
            )),
            FreeBuiltin::ParseF64 => Ok(format!(
                "({}).trim().parse::<f64>().map_err(|rv_error| crate::Error::new(&rv_error.to_string()))",
                self.str_borrow(first, indent)?
            )),
            FreeBuiltin::MathUnary(method) => Ok(format!(
                "({}).{method}()",
                self.nth_arg(args, 0, indent)?
            )),
            FreeBuiltin::MathBinary(method) => {
                let a = self.nth_arg(args, 0, indent)?;
                let b = self.nth_arg(args, 1, indent)?;
                // STD-6.5 fixes the observable result on signed zeros:
                // `max(-0.0, +0.0) = +0.0` and `min(-0.0, +0.0) = -0.0`.
                // `f64::max`/`min` lower to an LLVM intrinsic whose answer for
                // that tie depends on the optimisation level (`-O3` and
                // `-C opt-level=0` disagree), so the tie is emitted explicitly.
                // NaN is left to the intrinsic, which already implements
                // "if one operand is NaN, return the other".
                let is_float = matches!(
                    args.first()
                        .and_then(|argument| self.facts().expr_types.get(&argument.id)),
                    Some(Type::F32) | Some(Type::F64)
                );
                if is_float && (method == "max" || method == "min") {
                    let tie = if method == "max" { "0.0" } else { "-0.0" };
                    return Ok(format!(
                        "(if ({a}) == 0.0 && ({b}) == 0.0 {{ {tie} }} else {{ ({a}).{method}({b}) }})"
                    ));
                }
                Ok(format!("({a}).{method}({b})"))
            }
            FreeBuiltin::VariantCtor => {
                let segments: Vec<&str> = path
                    .strip_prefix("std::")
                    .unwrap_or(path)
                    .split("::")
                    .collect();
                let constructor = segments[segments.len() - 1];
                let owner = segments
                    .get(segments.len().wrapping_sub(2))
                    .copied()
                    .filter(|owner| matches!(*owner, "Option" | "Result"));
                match owner {
                    Some(owner) => Ok(format!("{owner}::{constructor}({rendered})")),
                    None => Ok(format!("{constructor}({rendered})")),
                }
            }
        }
    }

    pub(super) fn expect_arity(
        &self,
        path: &str,
        args: &[Expr],
        expected: usize,
    ) -> Result<(), CodegenError> {
        if args.len() == expected {
            Ok(())
        } else {
            Err(err(format!(
                "builtin `{path}` expects {expected} argument(s), got {}",
                args.len()
            )))
        }
    }

    pub(super) fn nth_arg(
        &self,
        args: &[Expr],
        index: usize,
        indent: usize,
    ) -> Result<String, CodegenError> {
        match args.get(index) {
            Some(argument) => self.expr(argument, indent),
            None => Err(err(format!(
                "builtin call is missing argument #{} (call node {})",
                index + 1,
                args.first().map(|_| "?").unwrap_or("?")
            ))),
        }
    }

    /// Emit an argument that must coerce to `&str`.
    pub(super) fn str_arg(
        &self,
        arg: Option<&Expr>,
        indent: usize,
    ) -> Result<String, CodegenError> {
        match arg {
            Some(argument) => {
                let text = self.expr(argument, indent)?;
                match self.facts().expr_types.get(&argument.id) {
                    Some(Type::String) | Some(Type::Str) => Ok(format!("&({text})")),
                    _ => Ok(text),
                }
            }
            None => Err(err("builtin call is missing its text argument")),
        }
    }

    /// Emit an argument that must be usable as a `&str` receiver (`str::parse_*`).
    pub(super) fn str_borrow(
        &self,
        arg: Option<&Expr>,
        indent: usize,
    ) -> Result<String, CodegenError> {
        match arg {
            Some(argument) => {
                let text = self.expr(argument, indent)?;
                match self.facts().expr_types.get(&argument.id) {
                    Some(Type::String) | Some(Type::Str) => Ok(format!("&({text})")),
                    _ => Ok(text),
                }
            }
            None => Err(err("builtin call is missing its text argument")),
        }
    }

    pub(super) fn method_call_expr(
        &self,
        expr: &Expr,
        receiver: &Expr,
        method: &Ident,
        args: &[Expr],
        indent: usize,
    ) -> Result<String, CodegenError> {
        let target = self.facts().methods.get(&expr.id).ok_or_else(|| {
            err(format!(
                "missing method fact for method call (expression node {})",
                expr.id
            ))
        })?;
        let receiver_text = self.operand(receiver, indent)?;
        match target {
            MethodTarget::User { item } => {
                let name = self
                    .index
                    .fns
                    .get(item)
                    .map(|entry| entry.rust_name.clone())
                    .unwrap_or_else(|| mangle_ident(&method.name));
                Ok(format!(
                    "{receiver_text}.{name}({})",
                    self.call_args(args, indent)?
                ))
            }
            MethodTarget::Builtin { path, .. } => {
                self.builtin_method_call(expr, path, &receiver_text, args, indent)
            }
            MethodTarget::Error => Err(err(format!(
                "method call (node {}) has an unresolved target",
                expr.id
            ))),
        }
    }

    pub(super) fn builtin_method_call(
        &self,
        expr: &Expr,
        path: &str,
        receiver: &str,
        args: &[Expr],
        indent: usize,
    ) -> Result<String, CodegenError> {
        let (owner, method) = split_builtin_method(path).ok_or_else(|| {
            err(format!(
                "malformed builtin method path `{path}` (method call node {})",
                expr.id
            ))
        })?;
        let owner = normalize_owner(owner);
        let rendered = self.call_args(args, indent)?;

        if let Some(special) = builtin_method_special(&owner, method, receiver, &rendered) {
            return Ok(special);
        }

        // Direct pass-through only for the methods the Core 1.0 stdlib actually
        // provides on this owner. No hidden clone, no hidden conversion.
        if builtin_method_is_direct(&owner, method) {
            return Ok(format!("{receiver}.{}({rendered})", mangle_ident(method)));
        }
        Err(err(format!(
            "unsupported builtin method `{path}` (owner `{owner}`, method `{method}`) at method call node {}; \
             the Ravum Core 1.0 stdlib has no such method",
            expr.id
        )))
    }

    // -- constants ---------------------------------------------------------

    pub(super) fn const_value(
        &self,
        ty: &Type,
        value: &ConstValue,
    ) -> Result<String, CodegenError> {
        if matches!(value, ConstValue::Str(_)) && matches!(ty, Type::String) {
            return Err(err(
                "a `String` constant cannot be generated: stable Rust cannot build a non-empty `String` in a constant; declare the constant as `&str`",
            ));
        }
        Ok(match value {
            ConstValue::Int(number) => {
                let name = int_type_name(ty).ok_or_else(|| {
                    err(format!(
                        "constant with an integer value has non-integer type `{ty}`"
                    ))
                })?;
                if *number < 0 {
                    format!("-{}{}", number.unsigned_abs(), name)
                } else {
                    format!("{number}{name}")
                }
            }
            ConstValue::Bool(value) => {
                if *value {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            ConstValue::Char(value) => char_literal(*value),
            ConstValue::Str(text) => string_literal(text),
        })
    }

    pub(super) fn literal_const_expr(&self, expr: &Expr) -> Result<String, CodegenError> {
        match &expr.kind {
            ExprKind::Int { value, suffix } => {
                Ok(self.int_literal_text(*value, *suffix, self.facts().expr_types.get(&expr.id)))
            }
            ExprKind::Bool(value) => Ok(if *value { "true" } else { "false" }.to_string()),
            ExprKind::Char(value) => Ok(char_literal(*value)),
            ExprKind::Str(value) => Ok(string_literal(value)),
            _ => Err(err(format!(
                "missing `Facts::consts` entry for constant value (expression node {})",
                expr.id
            ))),
        }
    }

    // -- source markers ----------------------------------------------------
}
