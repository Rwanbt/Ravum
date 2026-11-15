use super::*;

impl<'p, 'w, 'f> TypeChecker<'p, 'w, 'f> {
    pub(super) fn check_const(&mut self, module: ModuleId, item: NodeId, c: &ConstItem) {
        self.module = module;
        self.generics.clear();
        self.self_ty = None;
        self.fn_ret = Type::Unit;
        self.current_fn = None;
        self.scopes.clear();
        self.push_scope();
        self.in_const = true;
        let declared = self
            .facts
            .type_nodes
            .get(&c.ty.id)
            .cloned()
            .unwrap_or_else(|| self.resolve_type(&c.ty));
        // G-5.11 / TYP-1.6: a constant of type `!` is not constructible.
        if crate::types::contains_never(&declared) {
            self.error(
                "E-TYP-MISMATCH",
                c.ty.span,
                "the never type (`!`) is not a valid constant type in Core 1.0".to_string(),
            );
        }
        let (value_ty, value) = self.check_const_expr(&c.value, Some(&declared));
        if !Self::compatible(&value_ty, &declared) {
            self.error(
                "E-TYP-MISMATCH",
                c.value.span,
                format!("constant value has type `{value_ty}`, declared `{declared}`"),
            );
        }
        if let Some(value) = value {
            let rust_path = self
                .facts
                .item_paths
                .get(&item)
                .cloned()
                .unwrap_or_else(|| {
                    let module_path = self.world.modules[module].path.clone();
                    resolve::join_path(&module_path, &resolve::rust_ident(&c.name.name))
                });
            self.facts.consts.insert(
                item,
                ConstInfo {
                    item,
                    rust_path,
                    name: c.name.name.clone(),
                    ty: declared,
                    value,
                },
            );
        }
        self.in_const = false;
        self.pop_scope();
    }

    pub(super) fn check_const_expr(
        &mut self,
        expr: &Expr,
        expected: Option<&Type>,
    ) -> (Type, Option<ConstValue>) {
        let (ty, value) = match &expr.kind {
            ExprKind::Int { value, suffix } => {
                let ty = self.check_int_literal(expr, *value, *suffix, expected);
                let value = if ty.is_int() && int_value_fits(*value, &ty) {
                    Some(ConstValue::Int(int_literal_to_i128(*value, &ty)))
                } else {
                    None
                };
                (ty, value)
            }
            ExprKind::Bool(value) => (Type::Bool, Some(ConstValue::Bool(*value))),
            ExprKind::Char(value) => (Type::Char, Some(ConstValue::Char(*value))),
            ExprKind::Str(value) => (Type::str_ref(), Some(ConstValue::Str(value.clone()))),
            ExprKind::Float { text, suffix } => {
                let ty = self.check_float_literal(expr, text, *suffix, expected);
                self.error(
                    "E-RES-CONST-NOT-CONST",
                    expr.span,
                    "floating-point constants are not representable in Core 1.0 facts".to_string(),
                );
                (ty, None)
            }
            ExprKind::Unary { op, expr: operand } => match op {
                UnOp::Neg => {
                    // TYP-13.2 / G-6.37: `const MIN: i8 = -128;` is valid even
                    // though the positive magnitude 128 does not fit `i8`.
                    // Recursing first range-checked `128` and then reported a
                    // spurious E-RES-CONST-NOT-CONST ("not a constant"), while
                    // the same expression was accepted in a `let` (`check_unary`
                    // handles the sign before checking the magnitude).
                    if let ExprKind::Int { value, suffix } = &operand.kind {
                        let ty = match suffix {
                            Some(suffix) => int_type(*suffix),
                            None => match expected {
                                Some(t) if t.is_int() => t.clone(),
                                _ => Type::I32,
                            },
                        };
                        self.facts.expr_types.insert(operand.id, ty.clone());
                        if !ty.is_signed_int() {
                            self.error(
                                "E-TYP-MISMATCH",
                                expr.span,
                                format!("cannot negate a value of type `{ty}`"),
                            );
                            return (ty, None);
                        }
                        let (min_magnitude, _, _) = int_bounds(&ty);
                        if *value > min_magnitude {
                            self.error(
                                "E-TYP-LITERAL-RANGE",
                                operand.span,
                                format!("integer literal `-{value}` does not fit in `{ty}`"),
                            );
                            return (ty, None);
                        }
                        let magnitude = *value;
                        let negated = if magnitude == 1u128 << 127 {
                            i128::MIN
                        } else {
                            -(magnitude as i128)
                        };
                        return (ty, Some(ConstValue::Int(negated)));
                    }
                    let (operand_ty, operand_value) = self.check_const_expr(operand, expected);
                    match operand_value {
                        Some(ConstValue::Int(v)) => {
                            let ty = if operand_ty.is_int() {
                                operand_ty.clone()
                            } else {
                                Type::I32
                            };
                            match v.checked_neg() {
                                Some(neg) if int_i128_fits(neg, &ty) => {
                                    (ty, Some(ConstValue::Int(neg)))
                                }
                                _ => {
                                    self.error(
                                        "E-TYP-CONST",
                                        expr.span,
                                        "constant overflow".to_string(),
                                    );
                                    (ty, None)
                                }
                            }
                        }
                        _ => {
                            self.error(
                                "E-RES-CONST-NOT-CONST",
                                expr.span,
                                "unary minus requires an integer constant".to_string(),
                            );
                            (operand_ty, None)
                        }
                    }
                }
                UnOp::Not => {
                    let (ty, value) = self.check_const_expr(operand, expected);
                    let out = match value {
                        Some(ConstValue::Bool(v)) => Some(ConstValue::Bool(!v)),
                        Some(ConstValue::Int(v)) => Some(ConstValue::Int(!v)),
                        _ => None,
                    };
                    (ty, out)
                }
                _ => {
                    // G-5.11 (MUST): `const_expr` has no variable, no call, no
                    // field access, no index, no `?`, no block; those forms are
                    // `E-TYP-CONST`. `E-RES-CONST-NOT-CONST` is reserved for a
                    // value the checker's facts cannot represent (e.g. a float).
                    self.error(
                        "E-TYP-CONST",
                        expr.span,
                        "references are not constant expressions".to_string(),
                    );
                    (Type::Error, None)
                }
            },
            ExprKind::Binary { op, lhs, rhs } => {
                let (l_ty, l_val) = self.check_const_expr(lhs, expected);
                let (r_ty, r_val) = self.check_const_expr(rhs, Some(&l_ty));
                let _ = r_ty;
                self.const_binary(expr, *op, &l_ty, l_val, r_val)
            }
            ExprKind::Cast { expr: operand, ty } => {
                let (source_ty, source_val) = self.check_const_expr(operand, None);
                let target = self.resolve_type(ty);
                if !cast_allowed(&source_ty, &target) {
                    self.error(
                        "E-TYP-CAST",
                        operand.span,
                        format!("cast from `{source_ty}` to `{target}` is not allowed in Core 1.0"),
                    );
                    return (Type::Error, None);
                }
                let value = source_val.and_then(|v| cast_const_value(v, &source_ty, &target));
                (target, value)
            }
            ExprKind::Block(_) => {
                // G-5.11 (MUST): a block is not a `const_expr`.
                self.error(
                    "E-TYP-CONST",
                    expr.span,
                    "blocks are not constant expressions".to_string(),
                );
                (Type::Error, None)
            }
            _ => {
                // G-5.11 / G-5.13 (MUST): variable, call, field, index, `?` and
                // a reference to another `const` are `E-TYP-CONST`.
                self.error(
                    "E-TYP-CONST",
                    expr.span,
                    "this expression is not allowed in a constant".to_string(),
                );
                (Type::Error, None)
            }
        };
        self.record_expr_type(expr.id, &ty);
        (ty, value)
    }

    pub(super) fn const_binary(
        &mut self,
        expr: &Expr,
        op: ast::BinOp,
        ty: &Type,
        lhs: Option<ConstValue>,
        rhs: Option<ConstValue>,
    ) -> (Type, Option<ConstValue>) {
        use ast::BinOp as B;
        match op {
            B::Add | B::Sub | B::Mul | B::Div | B::Rem => {
                if let (Some(ConstValue::Int(a)), Some(ConstValue::Int(b))) = (&lhs, &rhs) {
                    let result = match op {
                        B::Add => a.checked_add(*b),
                        B::Sub => a.checked_sub(*b),
                        B::Mul => a.checked_mul(*b),
                        B::Div => {
                            if *b == 0 {
                                self.error(
                                    "E-TYP-CONST",
                                    expr.span,
                                    "division by zero in constant".to_string(),
                                );
                                None
                            } else {
                                a.checked_div(*b)
                            }
                        }
                        B::Rem => {
                            if *b == 0 {
                                self.error(
                                    "E-TYP-CONST",
                                    expr.span,
                                    "remainder by zero in constant".to_string(),
                                );
                                None
                            } else {
                                a.checked_rem(*b)
                            }
                        }
                        _ => None,
                    };
                    match result {
                        Some(v) if int_i128_fits(v, ty) => (ty.clone(), Some(ConstValue::Int(v))),
                        Some(_) => {
                            self.error("E-TYP-CONST", expr.span, "constant overflow".to_string());
                            (ty.clone(), None)
                        }
                        None => {
                            self.error(
                                "E-TYP-CONST",
                                expr.span,
                                "constant evaluation overflow".to_string(),
                            );
                            (ty.clone(), None)
                        }
                    }
                } else {
                    (ty.clone(), None)
                }
            }
            B::Eq | B::Ne | B::Lt | B::Le | B::Gt | B::Ge | B::And | B::Or => {
                let value = match (op, &lhs, &rhs) {
                    (B::And, Some(ConstValue::Bool(a)), Some(ConstValue::Bool(b))) => {
                        Some(ConstValue::Bool(*a && *b))
                    }
                    (B::Or, Some(ConstValue::Bool(a)), Some(ConstValue::Bool(b))) => {
                        Some(ConstValue::Bool(*a || *b))
                    }
                    (_, Some(ConstValue::Int(a)), Some(ConstValue::Int(b))) => {
                        let b = match op {
                            B::Eq => a == b,
                            B::Ne => a != b,
                            B::Lt => a < b,
                            B::Le => a <= b,
                            B::Gt => a > b,
                            B::Ge => a >= b,
                            _ => false,
                        };
                        Some(ConstValue::Bool(b))
                    }
                    _ => None,
                };
                (Type::Bool, value)
            }
            B::BitAnd | B::BitOr | B::BitXor | B::Shl | B::Shr => {
                if let (Some(ConstValue::Int(a)), Some(ConstValue::Int(b))) = (&lhs, &rhs) {
                    let result = match op {
                        B::BitAnd => Some(a & b),
                        B::BitOr => Some(a | b),
                        B::BitXor => Some(a ^ b),
                        B::Shl => {
                            if *b < 0 || *b >= 128 {
                                None
                            } else {
                                a.checked_shl(*b as u32)
                            }
                        }
                        B::Shr => {
                            if *b < 0 || *b >= 128 {
                                None
                            } else {
                                a.checked_shr(*b as u32)
                            }
                        }
                        _ => None,
                    };
                    match result {
                        Some(v) if int_i128_fits(v, ty) => (ty.clone(), Some(ConstValue::Int(v))),
                        _ => {
                            self.error(
                                "E-TYP-CONST",
                                expr.span,
                                "constant evaluation overflow".to_string(),
                            );
                            (ty.clone(), None)
                        }
                    }
                } else {
                    (ty.clone(), None)
                }
            }
        }
    }
}
