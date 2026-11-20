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
        self.emit_fn_as(buf, item, fun, None)
    }

    /// RFC-INVARIANTS-1: the `invariant` checks of struct `def` on the value
    /// `var` (an expression of type `S` or `&S`): one `if` per clause, in
    /// order, the message naming the clause and the shown field values.
    pub(super) fn invariant_checks(&self, def: NodeId, var: &str) -> String {
        use ravum_lang_sema::contracts::{shown, Shown};
        let Some(info) = self.facts().structs.get(&def) else {
            return String::new();
        };
        let mut parts = Vec::new();
        let mut args = String::new();
        for f in &info.fields {
            match shown(&f.ty, true) {
                Some(Shown::Scalar) => parts.push(format!("self.{} = {{}}", f.name)),
                Some(Shown::Text) => parts.push(format!("self.{} = \\\"{{}}\\\"", f.name)),
                None => continue,
            }
            args.push_str(&format!(", {var}.{}", f.rust_name));
        }
        let fmt = if parts.is_empty() {
            String::new()
        } else {
            format!(" ({})", parts.join(", "))
        };
        let mut out = String::new();
        for (method, text) in &info.invariants {
            let msg = format!("contract violated: invariant {text}");
            out.push_str(&format!(
                "if !{var}.{method}() {{ panic!(\"{{}}{fmt}\", {msg:?}{args}); }} "
            ));
        }
        out
    }

    /// The struct whose invariants a `&mut self` method of `implementation`
    /// re-establishes on exit.
    fn invariant_owner(&self, implementation: &ImplItem) -> Option<NodeId> {
        match self.facts().type_nodes.get(&implementation.self_ty.id) {
            Some(ravum_lang_sema::types::Type::Nominal { def, .. })
                if self
                    .facts()
                    .structs
                    .get(def)
                    .is_some_and(|s| !s.invariants.is_empty()) =>
            {
                Some(*def)
            }
            _ => None,
        }
    }

    /// [`Self::emit_fn`] under another Rust name (`rename`), private.
    fn emit_fn_as(
        &self,
        buf: &mut Buf,
        item: &Item,
        fun: &FnItem,
        rename: Option<&str>,
    ) -> Result<(), CodegenError> {
        self.marker(buf, item.span);
        // `Index::build` walks the same AST and records every function.
        let entry = &self.index.fns[&item.id];

        let mut signature = String::new();
        // `main` and test functions are called from the generated crate-root
        // entry point, so they must be visible outside their Ravum module.
        let is_entry_point = Some(item.id) == self.index.user_main || fun.is_test;
        if (item.vis.is_public() || is_entry_point) && rename.is_none() {
            signature.push_str("pub ");
        }
        signature.push_str("fn ");
        signature.push_str(rename.unwrap_or(&entry.rust_name));
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
        if fun.is_async {
            // CONC-6.1: an `async fn` returns a boxed future of its result; the
            // body runs, unchanged, inside `async move` (its parameters own no
            // reference, CONC-6.3, so the future is `'static`).
            signature.push_str(&format!(" -> crate::Future<{ret}>"));
        } else if ret != "()" {
            signature.push_str(&format!(" -> {ret}"));
        }
        signature.push(' ');

        // Plan the array-element paths of this body (OWN-5.4) before emitting it:
        // `expr` consults the plan when it reaches a projection or an index.
        *self.array_moves.borrow_mut() = self.plan_array_moves(&fun.body);
        let mut body = self.block_text(buf.indent, &fun.body)?;
        if !fun.contracts.is_empty() {
            body = self.contract_body(buf.indent, item.id, fun, &ret, body)?;
        }
        if fun.is_async {
            buf.line(&format!("{signature}{{ Box::pin(async move {body}) }}"));
        } else {
            buf.line(&format!("{signature}{body}"));
        }
        Ok(())
    }

    /// RFC-CONTRACTS-1: `requires` checked on entry; with `ensures`, the body
    /// runs in an immediately called `move` closure (a `return` leaves the
    /// closure; `Copy` parameters keep their entry value outside it), then
    /// every `ensures` is checked with `result` bound.
    fn contract_body(
        &self,
        indent: usize,
        id: ravum_lang_syntax::NodeId,
        fun: &FnItem,
        ret: &str,
        body: String,
    ) -> Result<String, CodegenError> {
        use ravum_lang_sema::contracts::{shown, Shown};
        let mut out = String::from("{ ");
        let mut ensures = Vec::new();
        // Amendment 1: the values shown after the clause, by declared type
        // (the IR lowering applies the same rule: identical messages).
        let sig = self.checked.facts.fn_sigs.get(&id);
        let note = |ensures: bool| -> (String, Vec<String>) {
            let mut parts = Vec::new();
            let mut args = Vec::new();
            let mut push = |name: &str, ident: String, s: Shown| match s {
                Shown::Scalar => {
                    parts.push(format!("{name} = {{}}"));
                    args.push(ident);
                }
                Shown::Text => {
                    parts.push(format!("{name} = \\\"{{}}\\\""));
                    args.push(ident);
                }
            };
            if let Some(sig) = sig {
                for p in sig.params.iter().filter(|p| !p.is_receiver) {
                    let owned_ok = !ensures;
                    if let Some(s) = shown(&p.ty, owned_ok) {
                        push(&p.name, self.local_ident(&p.name), s);
                    }
                }
                if ensures {
                    if let Some(s) = shown(&sig.ret, true) {
                        push("result", self.local_ident("result"), s);
                    }
                }
            }
            if parts.is_empty() {
                (String::new(), args)
            } else {
                (format!(" ({})", parts.join(", ")), args)
            }
        };
        for c in &fun.contracts {
            let cond = self.expr(&c.expr, indent + 1)?;
            let kind = match c.kind {
                ravum_lang_syntax::ast::ContractKind::Requires => "requires",
                ravum_lang_syntax::ast::ContractKind::Ensures => "ensures",
            };
            let msg = format!("contract violated: {kind} {}", c.text);
            let (fmt, args) = note(c.kind == ravum_lang_syntax::ast::ContractKind::Ensures);
            let args: String = args.iter().map(|a| format!(", {a}")).collect();
            let check = format!("if !({cond}) {{ panic!(\"{{}}{fmt}\", {msg:?}{args}); }} ");
            match c.kind {
                ravum_lang_syntax::ast::ContractKind::Requires => out.push_str(&check),
                ravum_lang_syntax::ast::ContractKind::Ensures => ensures.push(check),
            }
        }
        if ensures.is_empty() {
            out.push_str(&body);
        } else {
            let result = self.local_ident("result");
            out.push_str(&format!(
                "let {result}: {ret} = (move || -> {ret} {body})(); "
            ));
            for e in ensures {
                out.push_str(&e);
            }
            out.push_str(&result);
        }
        out.push_str(" }");
        Ok(out)
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
        // RFC-DSP-ABI-2 DSP3-2: an ABI 3 block's state is a `repr(C)` layout,
        // so the layout is a function of the schema digest. Every non-generic
        // struct gets it (nested state structs included; harmless elsewhere).
        if structure.generics.is_empty() && self.options.dsp.as_ref().is_some_and(|c| c.abi == 3) {
            buf.line("#[repr(C)]");
        }
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
        let owner = self.invariant_owner(implementation);
        for (item, fun) in implementation.methods() {
            let exclusive = fun.params.iter().any(|p| {
                matches!(
                    p.kind,
                    ParamKind::Receiver(ravum_lang_syntax::ast::Receiver::Exclusive)
                )
            });
            match owner {
                Some(def) if exclusive => self.emit_invariant_method(buf, item, fun, def)?,
                _ => self.emit_fn(buf, item, fun)?,
            }
        }
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    /// RFC-INVARIANTS-1 (INV-3): a `&mut self` method of a struct with
    /// invariants. The body becomes a private inner method; the method calls
    /// it with a reborrow of `self` and its arguments, then checks every
    /// invariant before returning (a panic of the body checks nothing).
    fn emit_invariant_method(
        &self,
        buf: &mut Buf,
        item: &Item,
        fun: &FnItem,
        def: NodeId,
    ) -> Result<(), CodegenError> {
        let entry = &self.index.fns[&item.id];
        let inner = format!("rv_inv_body_{}", entry.rust_name.trim_start_matches("r#"));
        self.emit_fn_as(buf, item, fun, Some(&inner))?;
        let mut params = vec!["&mut self".to_string()];
        let mut args = vec!["&mut *self".to_string()];
        for param in &fun.params {
            if let ParamKind::Binding { name, ty, .. } = &param.kind {
                let ident = self.local_ident(&name.name);
                params.push(format!(
                    "{ident}: {}",
                    self.type_node_pos(ty, TypePos::Param)?
                ));
                args.push(ident);
            }
        }
        let ret = self.type_node(&fun.ret)?;
        let vis = if item.vis.is_public() { "pub " } else { "" };
        let generics = self.generics(&fun.generics);
        let checks = self.invariant_checks(def, "self");
        buf.line(&format!(
            "{vis}fn {}{generics}({}) -> {ret} {{ let rv_inv_r = Self::{inner}({}); {checks}rv_inv_r }}",
            entry.rust_name,
            params.join(", "),
            args.join(", ")
        ));
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
