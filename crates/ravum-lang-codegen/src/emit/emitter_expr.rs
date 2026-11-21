use super::*;

impl<'a> Emitter<'a> {
    pub(super) fn is_atomic(kind: &ExprKind) -> bool {
        matches!(
            kind,
            ExprKind::Unit
                | ExprKind::Int { .. }
                | ExprKind::Float { .. }
                | ExprKind::Bool(_)
                | ExprKind::Char(_)
                | ExprKind::Str(_)
                | ExprKind::SelfValue
                | ExprKind::Path(_)
                | ExprKind::Call { .. }
                | ExprKind::MethodCall { .. }
                | ExprKind::Field { .. }
                | ExprKind::TupleIndex { .. }
                | ExprKind::Index { .. }
                | ExprKind::Tuple(_)
                | ExprKind::Array(_)
                | ExprKind::ArrayRepeat { .. }
                | ExprKind::Try(_)
                | ExprKind::Await(_)
        )
    }

    /// Wrap in parentheses when the expression cannot be used as a prefix or
    /// postfix operand as-is. Extra parentheses are always semantically neutral.
    pub(super) fn operand(&self, expr: &Expr, indent: usize) -> Result<String, CodegenError> {
        let text = self.expr(expr, indent)?;
        if Self::is_atomic(&expr.kind) {
            Ok(text)
        } else {
            Ok(format!("({text})"))
        }
    }

    pub(super) fn expr(&self, expr: &Expr, indent: usize) -> Result<String, CodegenError> {
        match &expr.kind {
            ExprKind::Unit => Ok("()".to_string()),
            ExprKind::Int { value, suffix } => {
                Ok(self.int_literal_text(*value, *suffix, self.facts().expr_types.get(&expr.id)))
            }
            ExprKind::Float { text, suffix } => {
                Ok(self.float_literal(*suffix, text, self.facts().expr_types.get(&expr.id)))
            }
            ExprKind::Bool(value) => Ok(if *value { "true" } else { "false" }.to_string()),
            ExprKind::Char(value) => Ok(char_literal(*value)),
            ExprKind::Str(value) => Ok(string_literal(value)),
            ExprKind::SelfValue => Ok("self".to_string()),
            ExprKind::Path(path) => self.value_path(path),
            ExprKind::Call { args, .. } => self.call_expr(expr, args, indent),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => self.method_call_expr(expr, receiver, method, args, indent),
            ExprKind::Field { base, name } => {
                if let Some(text) = self.element_path_expr(expr, indent)? {
                    return Ok(text);
                }
                Ok(format!(
                    "{}.{}",
                    self.operand(base, indent)?,
                    mangle_ident(&name.name)
                ))
            }
            ExprKind::TupleIndex { base, index } => {
                if let Some(text) = self.element_path_expr(expr, indent)? {
                    return Ok(text);
                }
                Ok(format!("{}.{}", self.operand(base, indent)?, index))
            }
            ExprKind::Index { base, index } => {
                if let Some(text) = self.element_path_expr(expr, indent)? {
                    return Ok(text);
                }
                Ok(format!(
                    "{}[{}]",
                    self.operand(base, indent)?,
                    self.expr(index, indent)?
                ))
            }
            ExprKind::Unary { op, expr: inner } => {
                let operand = self.operand(inner, indent)?;
                Ok(match op {
                    UnOp::Neg => format!("-{operand}"),
                    UnOp::Not => format!("!{operand}"),
                    UnOp::Ref => format!("&{operand}"),
                    UnOp::RefMut => format!("&mut {operand}"),
                    UnOp::Deref => format!("*{operand}"),
                })
            }
            ExprKind::Binary { op, lhs, rhs } => Ok(format!(
                "{} {} {}",
                self.operand(lhs, indent)?,
                op.as_str(),
                self.operand(rhs, indent)?
            )),
            ExprKind::Assign { target, value } => {
                if self.path_mode(target.id) == Some(PathMode::Reinit) {
                    return self.element_reinit_expr(target, value, indent);
                }
                Ok(format!(
                    "{} = {}",
                    self.operand(target, indent)?,
                    self.expr(value, indent)?
                ))
            }
            ExprKind::AssignOp { op, target, value } => Ok(format!(
                "{} {}= {}",
                self.operand(target, indent)?,
                op.as_str(),
                self.expr(value, indent)?
            )),
            ExprKind::Cast { expr: inner, ty } => Ok(format!(
                "{} as {}",
                self.operand(inner, indent)?,
                self.type_node(ty)?
            )),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => self.if_text(cond, then_block, else_branch, indent),
            ExprKind::Match { scrutinee, arms } => self.match_text(scrutinee, arms, indent),
            ExprKind::While { cond, body } => Ok(format!(
                "while {} {}",
                self.operand(cond, indent)?,
                self.block_text(indent, body)?
            )),
            ExprKind::Loop { body } => Ok(format!("loop {}", self.block_text(indent, body)?)),
            ExprKind::For { pat, iter, body } => Ok(format!(
                "for {} in {} {}",
                self.pattern(pat)?,
                self.expr(iter, indent)?,
                self.block_text(indent, body)?
            )),
            ExprKind::Block(block) => self.block_text(indent, block),
            ExprKind::Unsafe(block) => self.block_text(indent, block),
            ExprKind::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.expr(item, indent)?);
                }
                if rendered.len() == 1 {
                    Ok(format!("({},)", rendered[0]))
                } else {
                    Ok(format!("({})", rendered.join(", ")))
                }
            }
            ExprKind::Array(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.expr(item, indent)?);
                }
                Ok(format!("[{}]", rendered.join(", ")))
            }
            ExprKind::ArrayRepeat { value, len } => Ok(format!(
                "[{}; {}]",
                self.expr(value, indent)?,
                self.expr(len, indent)?
            )),
            ExprKind::StructLit { path, fields } => {
                let base = self.struct_literal_path(path)?;
                let lit = if fields.is_empty() {
                    format!("{base} {{}}")
                } else {
                    let mut rendered = Vec::new();
                    for field in fields {
                        rendered.push(format!(
                            "{}: {}",
                            mangle_ident(&field.name.name),
                            self.expr(&field.value, indent)?
                        ));
                    }
                    format!("{base} {{ {} }}", rendered.join(", "))
                };
                // RFC-INVARIANTS-1 (INV-2): a constructed value is checked.
                if let Some(Resolution::Item { item }) = self.facts().resolutions.get(&path.id) {
                    let checks = self.invariant_checks(*item, "rv_inv_v");
                    if !checks.is_empty() {
                        return Ok(format!("{{ let rv_inv_v = {lit}; {checks}rv_inv_v }}"));
                    }
                }
                Ok(lit)
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                // Only closed `for` ranges reach the backend; an absent bound
                // would be written as nothing.
                let mut text = String::new();
                for first in start.iter() {
                    text.push_str(&self.operand(first, indent)?);
                }
                text.push_str(if *inclusive { "..=" } else { ".." });
                for last in end.iter() {
                    text.push_str(&self.operand(last, indent)?);
                }
                Ok(text)
            }
            ExprKind::Try(inner) => Ok(format!("{}?", self.operand(inner, indent)?)),
            ExprKind::Await(inner) => Ok(format!("{}.await", self.operand(inner, indent)?)),
            ExprKind::Return(value) => match value {
                Some(value) => Ok(format!("return {}", self.expr(value, indent)?)),
                None => Ok("return".to_string()),
            },
            ExprKind::Break => Ok("break".to_string()),
            ExprKind::Continue => Ok("continue".to_string()),
        }
    }

    pub(super) fn if_text(
        &self,
        cond: &Expr,
        then_block: &Block,
        else_branch: &Option<Box<Expr>>,
        indent: usize,
    ) -> Result<String, CodegenError> {
        let mut text = format!(
            "if {} {}",
            self.operand(cond, indent)?,
            self.block_text(indent, then_block)?
        );
        if let Some(otherwise) = else_branch {
            match &otherwise.kind {
                ExprKind::Block(block) => {
                    text.push_str(" else ");
                    text.push_str(&self.block_text(indent, block)?);
                }
                ExprKind::If {
                    cond,
                    then_block,
                    else_branch,
                } => {
                    text.push_str(" else ");
                    text.push_str(&self.if_text(cond, then_block, else_branch, indent)?);
                }
                _ => {
                    text.push_str(" else { ");
                    text.push_str(&self.expr(otherwise, indent)?);
                    text.push_str(" }");
                }
            }
        }
        Ok(text)
    }

    pub(super) fn match_text(
        &self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        indent: usize,
    ) -> Result<String, CodegenError> {
        // G-6.36: Rust has no float patterns. When an arm mentions a float
        // literal, bind the scrutinee once and lower every float sub-pattern to
        // a guard (`Some(__ravum_flt_0) if __ravum_flt_0 == 1.5f64`). The
        // checker requires a `_` arm for any float pattern, so the guards cannot
        // make the emitted match non-exhaustive.
        if !arms.iter().any(|arm| pattern_contains_float(&arm.pat)) {
            return self.plain_match_text(scrutinee, arms, indent);
        }
        // Generated bindings must neither shadow locals used by the arms
        // nor be interpreted as named constant patterns by Rust.
        let temp = self.match_temporary_name("__ravum_match_scrutinee".into());
        let mut text = "{\n".to_string();
        text.push_str(&indent_text(indent + 1));
        text.push_str("let ");
        text.push_str(&temp);
        text.push_str(" = ");
        text.push_str(&self.operand(scrutinee, indent + 1)?);
        text.push_str(";\n");
        text.push_str(&indent_text(indent + 1));
        text.push_str("match ");
        text.push_str(&temp);
        text.push_str(" {\n");
        for arm in arms {
            let mut counter = 0usize;
            let mut conditions: Vec<String> = Vec::new();
            let pattern =
                self.pattern_with_float_guards(&arm.pat, &mut counter, &mut conditions)?;
            let body = self.expr(&arm.body, indent + 2)?;
            text.push_str(&indent_text(indent + 2));
            text.push_str(&pattern);
            if !conditions.is_empty() {
                text.push_str(" if ");
                text.push_str(&conditions.join(" && "));
            }
            text.push_str(" => ");
            text.push_str(&body);
            text.push_str(",\n");
        }
        text.push_str(&indent_text(indent + 1));
        text.push_str("}\n");
        text.push_str(&indent_text(indent));
        text.push('}');
        Ok(text)
    }

    pub(super) fn plain_match_text(
        &self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        indent: usize,
    ) -> Result<String, CodegenError> {
        let mut text = format!("match {} {{\n", self.operand(scrutinee, indent)?);
        for arm in arms {
            let pattern = self.pattern(&arm.pat)?;
            let body = self.expr(&arm.body, indent + 1)?;
            text.push_str(&indent_text(indent + 1));
            text.push_str(&pattern);
            text.push_str(" => ");
            text.push_str(&body);
            text.push_str(",\n");
        }
        text.push_str(&indent_text(indent));
        text.push('}');
        Ok(text)
    }

    /// Renders `pat` replacing every float literal by a fresh binding and
    /// recording `binding == literal` in `conditions` (G-6.36).
    pub(super) fn pattern_with_float_guards(
        &self,
        pat: &Pat,
        counter: &mut usize,
        conditions: &mut Vec<String>,
    ) -> Result<String, CodegenError> {
        match &pat.kind {
            PatKind::Lit(LitPat::Float { text, suffix }) => {
                let name = self.match_temporary_name(format!("__ravum_flt_{counter}"));
                *counter += 1;
                let literal =
                    self.float_literal(*suffix, text, self.facts().pat_types.get(&pat.id));
                conditions.push(format!("{name} == {literal}"));
                Ok(name)
            }
            PatKind::Tuple(items) => {
                let mut rendered = Vec::new();
                for item in items {
                    rendered.push(self.pattern_with_float_guards(item, counter, conditions)?);
                }
                if rendered.len() == 1 {
                    Ok(format!("({},)", rendered[0]))
                } else {
                    Ok(format!("({})", rendered.join(", ")))
                }
            }
            PatKind::Variant { path, fields } => {
                let base = self.variant_pattern_base(path)?;
                match fields {
                    VariantPatFields::Tuple(items) => {
                        let mut rendered = Vec::new();
                        for item in items {
                            rendered
                                .push(self.pattern_with_float_guards(item, counter, conditions)?);
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
                                self.pattern_with_float_guards(&field.pat, counter, conditions)?
                            ));
                        }
                        Ok(format!("{base} {{ {} }}", rendered.join(", ")))
                    }
                }
            }
            _ => self.pattern(pat),
        }
    }

    // -- literals ----------------------------------------------------------

    pub(super) fn int_literal_text(
        &self,
        value: u128,
        suffix: Option<IntTy>,
        ty: Option<&Type>,
    ) -> String {
        let name = match suffix {
            Some(suffix) => Some(suffix.name().to_string()),
            None => ty.and_then(int_type_name).map(str::to_string),
        };
        match name {
            Some(name) => format!("{value}{name}"),
            None => format!("{value}"),
        }
    }

    pub(super) fn float_literal(
        &self,
        suffix: Option<FloatTy>,
        text: &str,
        ty: Option<&Type>,
    ) -> String {
        // The lexer builds a float token only after a fraction or an exponent
        // (lexer.rs `is_float`), so the text always holds `.`, `e` or `E`.
        let mut out = text.trim().to_string();
        let is_f32 = match suffix {
            Some(FloatTy::F32) => true,
            Some(FloatTy::F64) => false,
            None => matches!(ty, Some(Type::F32)),
        };
        // Always annotate the literal. Without a suffix, `2.0` reaches `rustc` as
        // `{float}` and any method call on it fails with E0689
        // (`math::sqrt(2.0)` was accepted by `check` and refused by `rustc`).
        // The checker's recorded type decides f32 vs f64; the default is f64.
        out.push_str(if is_f32 { "f32" } else { "f64" });
        out
    }

    // -- paths -------------------------------------------------------------
    fn match_temporary_name(&self, mut name: String) -> String {
        for _ in &self.index.float_match_names {
            if !self.index.float_match_names.contains(&name) {
                break;
            }
            name.push('_');
        }
        name
    }
}
