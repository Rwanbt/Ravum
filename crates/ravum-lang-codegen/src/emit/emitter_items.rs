use super::*;

impl<'a> Emitter<'a> {
    pub(super) fn emit_item(&self, buf: &mut Buf, item: &Item) -> Result<(), CodegenError> {
        match &item.kind {
            ItemKind::Fn(fun) => self.emit_fn(buf, item, fun),
            ItemKind::Struct(structure) => self.emit_struct(buf, item, structure),
            ItemKind::Enum(enumeration) => self.emit_enum(buf, item, enumeration),
            ItemKind::Impl(implementation) => self.emit_impl(buf, implementation),
            ItemKind::Const(constant) => self.emit_const(buf, item, constant),
            ItemKind::Mod(module) => self.emit_module(buf, module),
            ItemKind::Extern(block) => self.emit_extern_block(buf, block),
            ItemKind::Use(_) => Ok(()),
            ItemKind::ModDecl(declaration) => Err(err(format!(
                "unresolved `mod {};` declaration: the loader must expand file modules before code generation",
                declaration.name.name
            ))),
        }
    }

    pub(super) fn entry(&self, id: NodeId) -> Result<&ItemEntry, CodegenError> {
        self.index
            .items
            .get(&id)
            .ok_or_else(|| err(format!("item node {id} is missing from the module index")))
    }

    pub(super) fn emit_fn(
        &self,
        buf: &mut Buf,
        item: &Item,
        fun: &FnItem,
    ) -> Result<(), CodegenError> {
        self.marker(buf, item.span);
        // `Index::build` walks the same AST and records every function.
        let entry = &self.index.fns[&item.id];

        let mut signature = String::new();
        // `main` and test functions are called from the generated crate-root
        // entry point, so they must be visible outside their Ravum module.
        let is_entry_point = Some(item.id) == self.index.user_main || fun.is_test;
        if item.vis.is_public() || is_entry_point {
            signature.push_str("pub ");
        }
        signature.push_str("fn ");
        signature.push_str(&entry.rust_name);
        signature.push_str(&self.generics(&fun.generics));
        signature.push('(');
        let mut params = Vec::new();
        for param in &fun.params {
            match &param.kind {
                ParamKind::Receiver(receiver) => params.push(match receiver {
                    Receiver::Shared => "&self".to_string(),
                    Receiver::Exclusive => "&mut self".to_string(),
                    Receiver::ByValue => "self".to_string(),
                }),
                ParamKind::Binding { mutable, name, ty } => {
                    // A `mut` parameter binding is part of the checked program
                    // (the checker only accepts an assignment when the binding
                    // is mutable), so the emitted Rust parameter must carry
                    // `mut` too — otherwise `fn bump(mut x: i32) { x = x + 1; }`
                    // is a check-pass/build-fail (`E0384`).
                    params.push(format!(
                        "{}: {}",
                        if *mutable {
                            format!("mut {}", self.local_ident(&name.name))
                        } else {
                            self.local_ident(&name.name)
                        },
                        self.type_node_pos(ty, TypePos::Param)?
                    ));
                }
            }
        }
        signature.push_str(&params.join(", "));
        signature.push(')');
        let ret = self.type_node(&fun.ret)?;
        if ret != "()" {
            signature.push_str(&format!(" -> {ret}"));
        }
        signature.push(' ');

        // Plan the array-element paths of this body (OWN-5.4) before emitting it:
        // `expr` consults the plan when it reaches a projection or an index.
        *self.array_moves.borrow_mut() = self.plan_array_moves(&fun.body);
        let body = self.block_text(buf.indent, &fun.body)?;
        buf.line(&format!("{signature}{body}"));
        Ok(())
    }

    pub(super) fn emit_struct(
        &self,
        buf: &mut Buf,
        item: &Item,
        structure: &StructItem,
    ) -> Result<(), CodegenError> {
        self.marker(buf, item.span);
        let entry = self.entry(item.id)?;
        buf.line(&self.derive_attribute(item.id));
        let visibility = if item.vis.is_public() { "pub " } else { "" };
        let generics = self.generics(&structure.generics);
        let head = format!("{visibility}struct {}{generics}", entry.rust_name);
        if structure.fields.is_empty() {
            buf.line(&format!("{head} {{}}"));
            return Ok(());
        }
        buf.line(&format!("{head} {{"));
        buf.indent += 1;
        for field in &structure.fields {
            let field_visibility = if field.vis.is_public() { "pub " } else { "" };
            buf.line(&format!(
                "{field_visibility}{}: {},",
                mangle_ident(&field.name.name),
                self.type_node(&field.ty)?
            ));
        }
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    pub(super) fn emit_enum(
        &self,
        buf: &mut Buf,
        item: &Item,
        enumeration: &EnumItem,
    ) -> Result<(), CodegenError> {
        self.marker(buf, item.span);
        let entry = self.entry(item.id)?;
        buf.line(&self.derive_attribute(item.id));
        let visibility = if item.vis.is_public() { "pub " } else { "" };
        let generics = self.generics(&enumeration.generics);
        buf.line(&format!(
            "{visibility}enum {}{generics} {{",
            entry.rust_name
        ));
        buf.indent += 1;
        for variant in &enumeration.variants {
            let name = mangle_ident(&variant.name.name);
            match &variant.fields {
                VariantDefFields::Unit => buf.line(&format!("{name},")),
                VariantDefFields::Tuple(types) => {
                    let mut rendered = Vec::new();
                    for ty in types {
                        rendered.push(self.type_node(ty)?);
                    }
                    buf.line(&format!("{name}({}),", rendered.join(", ")));
                }
                VariantDefFields::Struct(fields) => {
                    let mut rendered = Vec::new();
                    for field in fields {
                        rendered.push(format!(
                            "{}: {}",
                            mangle_ident(&field.name.name),
                            self.type_node(&field.ty)?
                        ));
                    }
                    buf.line(&format!("{name} {{ {} }},", rendered.join(", ")));
                }
            }
        }
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    pub(super) fn emit_impl(
        &self,
        buf: &mut Buf,
        implementation: &ImplItem,
    ) -> Result<(), CodegenError> {
        let self_type = self.type_node(&implementation.self_ty)?;
        let generics = self.generics(&implementation.generics);
        buf.line(&format!("impl{generics} {self_type} {{"));
        buf.indent += 1;
        // The checker accepts only functions inside an `impl`
        // (E-RES-IMPL-UNKNOWN); anything else is refused here too.
        if let Some(item) = implementation.non_methods().next() {
            return Err(err(format!(
                "unsupported item inside `impl` (item node {})",
                item.id
            )));
        }
        for (item, fun) in implementation.methods() {
            self.emit_fn(buf, item, fun)?;
        }
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    pub(super) fn emit_const(
        &self,
        buf: &mut Buf,
        item: &Item,
        constant: &ravum_lang_syntax::ast::ConstItem,
    ) -> Result<(), CodegenError> {
        self.marker(buf, item.span);
        let entry = self.entry(item.id)?;
        let visibility = if item.vis.is_public() { "pub " } else { "" };
        let ty = self.type_node(&constant.ty)?;
        let value = match self.facts().consts.get(&item.id) {
            Some(info) => self.const_value(&info.ty, &info.value)?,
            None => self.literal_const_expr(&constant.value)?,
        };
        buf.line(&format!(
            "{visibility}const {}: {ty} = {value};",
            entry.rust_name
        ));
        Ok(())
    }

    pub(super) fn derive_attribute(&self, id: NodeId) -> String {
        let mut derives = vec!["Debug", "Clone", "PartialEq"];
        let is_copy = self
            .facts()
            .structs
            .get(&id)
            .map(|info| info.is_copy)
            .or_else(|| self.facts().enums.get(&id).map(|info| info.is_copy))
            .unwrap_or(false);
        if is_copy {
            derives.push("Copy");
        }
        format!("#[derive({})]", derives.join(", "))
    }

    pub(super) fn generics(&self, params: &[GenericParam]) -> String {
        if params.is_empty() {
            return String::new();
        }
        let names: Vec<String> = params
            .iter()
            .map(|param| mangle_ident(&param.name.name))
            .collect();
        format!("<{}>", names.join(", "))
    }

    // -- statements and blocks ---------------------------------------------

    pub(super) fn block_text(&self, indent: usize, block: &Block) -> Result<String, CodegenError> {
        let mut buf = Buf::new(indent);
        buf.out.push_str("{\n");
        buf.indent += 1;
        for statement in &block.stmts {
            self.emit_stmt(&mut buf, statement)?;
        }
        if let Some(tail) = &block.tail {
            let text = self.expr(tail, buf.indent)?;
            buf.line(text.trim_end_matches('\n'));
        }
        buf.indent -= 1;
        buf.line("}");
        // The caller embeds the block inline (`fn f() {`, `if c {`, `=> {`), so
        // the closing brace must not carry a trailing newline.
        Ok(buf.finish().trim_end_matches('\n').to_string())
    }

    pub(super) fn emit_stmt(&self, buf: &mut Buf, statement: &Stmt) -> Result<(), CodegenError> {
        self.marker(buf, statement.span);
        match &statement.kind {
            StmtKind::Let(declaration) => {
                let pattern = self.pattern(&declaration.pat)?;
                let annotation = match &declaration.ty {
                    Some(ty) => Some(self.type_node(ty)?),
                    None => match self.facts().pat_types.get(&declaration.pat.id) {
                        Some(resolved) if !matches!(resolved, Type::Never | Type::Error) => {
                            Some(self.fmt_type(resolved)?)
                        }
                        _ => None,
                    },
                };
                let suffix = match &annotation {
                    Some(ty) => format!(": {ty}"),
                    None => String::new(),
                };
                match &declaration.init {
                    Some(init) => {
                        let value = self.expr(init, buf.indent)?;
                        buf.line(&format!(
                            "let {pattern}{suffix} = {};",
                            value.trim_end_matches('\n')
                        ));
                    }
                    None => {
                        if annotation.is_none() {
                            return Err(err(format!(
                                "`let` without initializer requires a type annotation (statement node {})",
                                statement.id
                            )));
                        }
                        buf.line(&format!("let {pattern}{suffix};"));
                    }
                }
                Ok(())
            }
            StmtKind::Expr { expr, semi } => {
                let text = self.expr(expr, buf.indent)?;
                let text = text.trim_end_matches('\n');
                if *semi {
                    buf.line(&format!("{text};"));
                } else {
                    buf.line(text);
                }
                Ok(())
            }
        }
    }

    // -- array-element moves (OWN-5.4 / P0-A.2) ----------------------------

    // -- expressions -------------------------------------------------------
}
