use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_item(&mut self) -> Option<Item> {
        if self.fatal {
            return None;
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return None;
        }
        self.descend();
        let result = self.parse_item_inner();
        self.depth -= 1;
        result
    }

    pub(super) fn parse_item_inner(&mut self) -> Option<Item> {
        let start = self.cur_span();
        self.skip_attributes();
        if self.fatal {
            return None;
        }
        let vis = self.parse_vis();
        self.skip_attributes();
        if self.fatal {
            return None;
        }

        let kind: Option<ItemKind> = match self.kind().clone() {
            TokenKind::Kw(Kw::Mod) => self.parse_mod_item(),
            TokenKind::Kw(Kw::Use) => self.parse_use_item(&vis),
            TokenKind::Kw(Kw::Fn) => self.parse_fn_item(false),
            TokenKind::Kw(Kw::Test) => self.parse_fn_item(true),
            TokenKind::Kw(Kw::Struct) => self.parse_struct_item(),
            TokenKind::Kw(Kw::Enum) => self.parse_enum_item(),
            TokenKind::Kw(Kw::Impl) => self.parse_impl_item(),
            TokenKind::Kw(Kw::Const) => self.parse_const_item(),
            TokenKind::Kw(Kw::Trait) => self.unsupported_item("E-UNSUPPORTED-TRAIT", "`trait`"),
            TokenKind::Kw(Kw::Where) => self.unsupported_item("E-UNSUPPORTED-WHERE", "`where`"),
            TokenKind::Kw(Kw::Extern) => self.parse_extern_block(),
            TokenKind::Kw(Kw::Async) if self.peek_kw(1, Kw::Fn) => {
                self.bump(); // `async`
                match self.parse_fn_item(false) {
                    Some(ItemKind::Fn(mut f)) => {
                        f.is_async = true;
                        Some(ItemKind::Fn(f))
                    }
                    other => other,
                }
            }
            TokenKind::Kw(Kw::Async) => self.unsupported_item("E-UNSUPPORTED-ASYNC", "`async`"),
            TokenKind::Kw(Kw::Unsafe) => self.unsupported_item("E-UNSUPPORTED-UNSAFE", "`unsafe`"),
            TokenKind::Kw(Kw::Move) => self.unsupported_item("E-UNSUPPORTED-MOVE", "`move`"),
            TokenKind::Kw(Kw::Type) => {
                self.unsupported_item("E-UNSUPPORTED-TYPE-ALIAS", "`type` aliases")
            }
            TokenKind::Kw(Kw::Static) => {
                self.unsupported_item("E-UNSUPPORTED-STATIC", "`static` items")
            }
            TokenKind::Ident(_) if self.peek_punct(1, Punct::Bang) => {
                let span = self.cur_span();
                self.bump();
                self.bump();
                self.skip_balanced();
                let span = self.span_from(span);
                self.diags.error(
                    "E-UNSUPPORTED-MACRO",
                    span,
                    "macro definitions and invocations are not supported in Ravum Core 1.0",
                );
                self.recover_item();
                None
            }
            _ => {
                // G-2.2, G-2.3: a token that cannot start an item (a stray `;`
                // included) is E-PARSE-EXPECTED: an item was expected.
                self.expected("an item");
                self.bump();
                self.recover_item();
                None
            }
        };

        kind.map(|kind| Item {
            id: self.id(),
            vis,
            kind,
            span: self.span_from(start),
        })
    }

    pub(super) fn unsupported_item(&mut self, code: &str, what: &str) -> Option<ItemKind> {
        let span = self.cur_span();
        self.bump();
        self.diags.error(
            code,
            span,
            format!("{what} is not supported in Ravum Core 1.0"),
        );
        self.recover_item();
        None
    }

    pub(super) fn parse_vis(&mut self) -> Vis {
        if !self.at_kw(Kw::Pub) {
            return Vis::Private;
        }
        let pub_span = self.cur_span();
        self.bump();
        if self.at(Punct::LParen) {
            let open = self.cur_span();
            self.skip_balanced();
            let span = pub_span.join(self.span_from(open));
            self.diags.error(
                "E-UNSUPPORTED-VIS",
                span,
                "restricted visibility `pub(...)` is not supported in Ravum Core 1.0",
            );
            return Vis::Public(pub_span);
        }
        // G-2.4: `pub` applies to fn, struct, enum, const, mod, use and to
        // fields and variants; `pub impl` and `pub extern` are refused.
        if self.at_kw(Kw::Impl) || self.at_kw(Kw::Extern) {
            let span = pub_span.join(self.cur_span());
            self.diags.error(
                "E-PARSE-EXPECTED",
                span,
                "`pub` is not valid here: an `impl` or `extern` block has no visibility",
            );
        }
        if self.at_kw(Kw::Test) {
            let span = pub_span.join(self.cur_span());
            self.diags.error(
                "E-PARSE-EXPECTED",
                span,
                "`test fn` is private in Ravum Core 1.0; `pub test fn` is not a valid item",
            );
        }
        Vis::Public(pub_span)
    }

    /// Detect `#[...]` even though `#` is not a token: the lexer reports it as
    /// `E-LEX-CHAR` and drops it, leaving `[` at the same position. Look back for
    /// `#` before the `[` so the parser can report the intended
    /// `E-UNSUPPORTED-ATTR` and skip the group.
    pub(super) fn attribute_hash_pos(&self) -> Option<u32> {
        if !self.at(Punct::LBracket) {
            return None;
        }
        let start = self.cur_span().start as usize;
        let bytes = self.src.as_bytes();
        let mut i = start;
        while i > 0 {
            i -= 1;
            match bytes[i] {
                b' ' | b'\t' | b'\r' | b'\n' => continue,
                b'#' => return Some(i as u32),
                _ => return None,
            }
        }
        None
    }

    pub(super) fn skip_attributes(&mut self) {
        while let Some(hash) = self.attribute_hash_pos() {
            self.report_attribute(hash);
        }
    }

    /// Consumes one `#[...]`/`#![...]` group (the current token is the opening
    /// `[`) and reports the canonical single diagnostic.
    pub(super) fn report_attribute(&mut self, hash: u32) {
        self.skip_balanced();
        let span = Span::new(self.file, hash, self.prev_end().max(hash));
        self.diags.error(
            "E-UNSUPPORTED-ATTR",
            span,
            "attributes (`#[...]`) are not supported in Ravum Core 1.0",
        );
    }

    pub(super) fn parse_ident(&mut self, what: &str) -> Ident {
        match self.kind().clone() {
            TokenKind::Ident(name) => {
                let span = self.cur_span();
                self.bump();
                Ident::new(name, span)
            }
            _ => {
                self.expected(what);
                Ident::new("<error>", self.cur_span())
            }
        }
    }

    pub(super) fn parse_mod_item(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `mod`
        let name = self.parse_ident("module name");
        if self.eat(Punct::Semi) {
            return Some(ItemKind::ModDecl(ModDecl { name }));
        }
        if self.at(Punct::LBrace) {
            let module = self.parse_module_body(Some(name), start);
            return Some(ItemKind::Mod(module));
        }
        self.expected("`;` or `{` after the module name");
        self.recover_item();
        None
    }

    pub(super) fn parse_module_body(&mut self, name: Option<Ident>, start: Span) -> Module {
        let open = self.cur_span();
        if !self.eat(Punct::LBrace) {
            self.expected("`{`");
            return Module {
                id: self.id(),
                name,
                items: Vec::new(),
                span: start,
            };
        }
        let items = self.parse_module_items(true);
        self.expect_close(Punct::RBrace, open, "`}`");
        Module {
            id: self.id(),
            name,
            items,
            span: self.span_from(start),
        }
    }

    pub(super) fn parse_use_item(&mut self, vis: &Vis) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `use`
        if vis.is_public() {
            self.diags.error(
                "E-UNSUPPORTED-REEXPORT",
                vis_span(vis),
                "`pub use` re-exports are not supported in Ravum Core 1.0",
            );
        }
        let path = self.parse_path(false, false);
        if self.at(Punct::ColonColon) && self.peek_punct(1, Punct::Star) {
            let star = self.peek_at(1).span;
            self.bump_n(2);
            self.diags.error(
                "E-UNSUPPORTED-TOKEN",
                star,
                "glob imports (`use foo::*;`) are not supported in Ravum Core 1.0",
            );
        } else if self.at(Punct::Star) {
            let star = self.cur_span();
            self.bump();
            self.diags.error(
                "E-UNSUPPORTED-TOKEN",
                star,
                "glob imports (`use foo::*;`) are not supported in Ravum Core 1.0",
            );
        }
        if self.at_kw(Kw::As) {
            let span = self.cur_span();
            self.bump();
            self.parse_ident("import alias");
            self.diags.error(
                "E-UNSUPPORTED-ALIAS",
                span,
                "`use ... as ...` aliases are not supported in Ravum Core 1.0",
            );
        }
        if !self.eat(Punct::Semi) {
            self.expected("`;`");
        }
        Some(ItemKind::Use(UseItem {
            path,
            span: self.span_from(start),
        }))
    }

    pub(super) fn parse_generics(&mut self) -> Vec<GenericParam> {
        let mut out = Vec::new();
        if !self.at(Punct::Lt) {
            return out;
        }
        let opener = self.cur_span();
        self.bump(); // `<`
                     // `<'a>` is a lifetime parameter list: already refused by the lexer with
                     // E-UNSUPPORTED-TOKEN, consumed here so `fn f<'a>()` does not cascade.
        let mut lifetimes = 0;
        while matches!(self.kind(), TokenKind::Lifetime(_)) {
            self.bump();
            lifetimes += 1;
            self.eat(Punct::Comma);
        }
        if lifetimes > 0 && self.at(Punct::Gt) {
            self.bump();
            return out;
        }
        if self.at(Punct::Gt) {
            self.expected("generic parameter name");
            self.bump();
            return out;
        }
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::Gt) {
                break;
            }
            let before = self.pos;
            match self.kind().clone() {
                TokenKind::Ident(name) => {
                    let span = self.cur_span();
                    self.bump();
                    out.push(GenericParam {
                        id: self.id(),
                        name: Ident::new(name, span),
                    });
                }
                _ => {
                    self.expected("generic parameter name");
                    if !self.at(Punct::Gt) && !self.at(Punct::Comma) {
                        self.bump();
                    }
                }
            }
            if self.at(Punct::Colon) {
                let span = self.cur_span();
                self.diags.error(
                    "E-UNSUPPORTED-BOUNDS",
                    span,
                    "generic bounds are not supported in Ravum Core 1.0",
                );
                self.skip_bound();
            }
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.pos == before && !self.at(Punct::Gt) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_gt(opener, "unclosed generic parameter list");
        out
    }

    pub(super) fn skip_bound(&mut self) {
        // `:` was not consumed by the caller; skip it and the bound tokens.
        self.bump();
        let mut angles = 0i32;
        while !self.at_eof() {
            match self.kind() {
                TokenKind::Punct(Punct::Lt) => {
                    angles += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::Gt) if angles > 0 => {
                    angles -= 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::Comma | Punct::Gt) if angles == 0 => break,
                TokenKind::Punct(Punct::LBrace | Punct::RBrace | Punct::Semi) => break,
                TokenKind::Punct(Punct::LParen | Punct::LBracket) => {
                    self.skip_balanced();
                }
                _ => self.bump(),
            }
        }
    }

    pub(super) fn expect_gt(&mut self, opener: Span, message: &str) {
        if self.at(Punct::Gt) {
            self.bump();
            return;
        }
        let at = self.cur_span();
        self.diags.push(
            Diagnostic::error("E-PARSE-UNCLOSED", opener, message.to_string()).with_note(
                format!(
                    "reached {} before the generic list was closed",
                    self.describe_token()
                ),
                Some(at),
            ),
        );
    }

    /// `extern "C" { fn f(x: i32) -> i32; ... }`.
    ///
    /// Core 1.0 accepts only the `"C"` ABI and declarations ending in `;`.
    /// `extern "C" fn f() { ... }` (a definition) remains
    /// `E-UNSUPPORTED-EXTERN`.
    pub(super) fn parse_extern_block(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `extern`
        let abi = if let TokenKind::Str(value) = self.kind().clone() {
            let span = self.cur_span();
            self.bump();
            Ident::new(value, span)
        } else {
            self.expected("ABI string after `extern` (only \"C\")");
            Ident::new("C".to_string(), self.zero_span())
        };
        if abi.name != "C" {
            self.diags.error(
                "E-UNSUPPORTED-ABI",
                abi.span,
                format!(
                    "`extern \"{}\"` is not supported in Ravum Core 1.0 (only `extern \"C\"`)",
                    abi.name
                ),
            );
        }
        if self.at_kw(Kw::Fn) {
            self.diags.error(
                "E-UNSUPPORTED-EXTERN",
                start,
                "`extern` function definitions are not supported in Ravum Core 1.0; declare them in an `extern \"C\" { ... }` block",
            );
            self.recover_item();
            return None;
        }
        // `from "<lib>"`: `from` is a contextual word, not a keyword.
        let lib = if matches!(self.kind(), TokenKind::Ident(word) if word == "from") {
            self.bump();
            if let TokenKind::Str(value) = self.kind().clone() {
                let span = self.cur_span();
                self.bump();
                Some(Ident::new(value, span))
            } else {
                self.expected("library name string after `from`");
                None
            }
        } else {
            None
        };
        if !self.at(Punct::LBrace) {
            self.expected("`{`");
            self.recover_item();
            return None;
        }
        self.bump(); // `{`
        let mut fns = Vec::new();
        while !self.at(Punct::RBrace) && !self.at_eof() && !self.fatal {
            if !self.at_kw(Kw::Fn) {
                let found = self.describe_token();
                self.unexpected(format!(
                    "expected `fn` or `}}` in `extern` block, found {found}"
                ));
                self.bump();
                continue;
            }
            let fn_start = self.cur_span();
            self.bump(); // `fn`
            let name = self.parse_ident("function name");
            let mut params = Vec::new();
            if self.at(Punct::LParen) {
                params = self.parse_params();
            } else {
                self.expected("`(`");
            }
            let ret = if self.eat(Punct::Arrow) {
                self.parse_type()
            } else {
                let span = self.zero_span();
                self.unit_type(span)
            };
            if !self.eat(Punct::Semi) {
                self.expected("`;` after `extern` function declaration");
            }
            fns.push(ExternFn {
                id: self.id(),
                name,
                params,
                ret,
                span: self.span_from(fn_start),
            });
        }
        if self.at(Punct::RBrace) {
            self.bump();
        } else {
            self.diags
                .error("E-PARSE-UNCLOSED", start, "unclosed `extern` block");
        }
        Some(ItemKind::Extern(ExternBlock {
            abi,
            lib,
            fns,
            span: self.span_from(start),
        }))
    }

    pub(super) fn parse_fn_item(&mut self, is_test: bool) -> Option<ItemKind> {
        let start = self.cur_span();
        if is_test {
            self.bump(); // `test`
            if !self.at_kw(Kw::Fn) {
                self.expected("`fn` after `test`");
                self.recover_item();
                return None;
            }
        }
        self.bump(); // `fn`
        let name = self.parse_ident("function name");
        let generics = self.parse_generics();
        let mut params = Vec::new();
        if self.at(Punct::LParen) {
            params = self.parse_params();
        } else {
            self.expected("`(`");
        }
        let ret = if self.eat(Punct::Arrow) {
            self.parse_type()
        } else {
            let span = self.zero_span();
            self.unit_type(span)
        };
        let effects = self.parse_effects();
        if effects.is_some() && self.at(Punct::Arrow) {
            // `effects(...)` must come after the return type (G-3.2). It is
            // valid after `)` only when the return type is omitted, so a `->`
            // here means the clause was misplaced: one precise diagnostic, then
            // skip the return type so the body still parses.
            let span = self.cur_span();
            self.diags.error(
                "E-PARSE-EXPECTED",
                span,
                "`effects(...)` must follow the return type; write `-> Ty effects(...)`",
            );
            self.bump(); // `->`
            let _ = self.parse_type();
        }
        if self.at_kw(Kw::Where) {
            let span = self.cur_span();
            self.bump();
            self.diags.error(
                "E-UNSUPPORTED-WHERE",
                span,
                "`where` clauses are not supported in Ravum Core 1.0",
            );
            self.skip_where_clause();
        }
        let contracts = self.parse_contracts();
        let body = if self.at(Punct::LBrace) {
            self.parse_block()
        } else {
            self.expected("`{`");
            if self.at(Punct::Semi) {
                self.bump();
            }
            let span = self.zero_span();
            self.empty_block(span)
        };
        Some(ItemKind::Fn(FnItem {
            name,
            generics,
            params,
            ret,
            effects,
            body,
            is_test,
            is_async: false,
            contracts,
            span: self.span_from(start),
        }))
    }

    /// Source text of `span`, whitespace runs collapsed to one space.
    /// The clause's normalized text (RFC-CONTRACTS-1 CON-5): its tokens
    /// with canonical spacing (one space around binary operators, none
    /// around `.`/`::`, inside brackets, before `,`/`?`, after a prefix
    /// operator), so that `ravum-lang fmt` never changes a message.
    fn source_text(&self, span: Span) -> String {
        let mut out = String::new();
        let mut prev: Option<&TokenKind> = None;
        // The previous token ends an operand: a following `-`/`&`/`*` is binary.
        let mut operand = false;
        let mut prefix = false;
        let mut prev_end = 0;
        for t in &self.tokens {
            if t.span.start < span.start || t.span.end > span.end || t.is_eof() {
                continue;
            }
            let text = self
                .src
                .get(t.span.start as usize..t.span.end as usize)
                .unwrap_or("");
            let k = &t.kind;
            let tight_before =
                matches!(
                    k,
                    TokenKind::Punct(
                        Punct::RParen
                            | Punct::RBracket
                            | Punct::Comma
                            | Punct::Dot
                            | Punct::ColonColon
                            | Punct::Question
                    )
                ) || (matches!(k, TokenKind::Punct(Punct::LParen | Punct::LBracket)) && operand);
            let tight_after_prev = matches!(
                prev,
                Some(TokenKind::Punct(
                    Punct::LParen | Punct::LBracket | Punct::Dot | Punct::ColonColon
                ))
            ) || prefix;
            // `>=`/`>>` reach the parser split (`>` `=`) for generics: glue
            // the pieces that touch in the source.
            let glued = matches!(prev, Some(TokenKind::Punct(Punct::Gt)))
                && prev_end == t.span.start
                && matches!(
                    k,
                    TokenKind::Punct(Punct::Eq | Punct::Gt | Punct::Ge | Punct::Shr | Punct::ShrEq)
                );
            if prev.is_some() && !tight_before && !tight_after_prev && !glued {
                out.push(' ');
            }
            out.push_str(text);
            prefix = matches!(k, TokenKind::Punct(Punct::Bang))
                || (matches!(k, TokenKind::Punct(Punct::Minus | Punct::Amp | Punct::Star))
                    && !operand);
            operand = matches!(
                k,
                TokenKind::Ident(_)
                    | TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::Question)
            ) || matches!(
                k,
                TokenKind::Kw(Kw::SelfValue | Kw::SelfType | Kw::True | Kw::False)
            ) || !matches!(
                k,
                TokenKind::Punct(_) | TokenKind::Kw(_) | TokenKind::Ident(_)
            );
            prev = Some(k);
            prev_end = t.span.end;
        }
        out
    }

    /// `requires EXPR` / `ensures EXPR` clauses before a body
    /// (RFC-CONTRACTS-1). Contextual words: recognized only here.
    fn parse_contracts(&mut self) -> Vec<Contract> {
        let mut out = Vec::new();
        loop {
            let kind = match self.kind() {
                TokenKind::Ident(w) if w == "requires" => ContractKind::Requires,
                TokenKind::Ident(w) if w == "ensures" => ContractKind::Ensures,
                _ => break,
            };
            let start = self.cur_span();
            self.bump();
            let expr = self.parse_condition();
            let text = self.source_text(expr.span);
            out.push(Contract {
                kind,
                expr,
                text,
                span: self.span_from(start),
            });
        }
        out
    }

    pub(super) fn skip_where_clause(&mut self) {
        let mut angles = 0i32;
        while !self.at_eof() {
            match self.kind() {
                TokenKind::Punct(Punct::LBrace) if angles == 0 => break,
                TokenKind::Punct(Punct::Semi) if angles == 0 => break,
                TokenKind::Punct(Punct::Lt) => {
                    angles += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::Gt) if angles > 0 => {
                    angles -= 1;
                    self.bump();
                }
                _ if angles == 0 && self.is_item_start() => break,
                _ => self.bump(),
            }
        }
    }

    pub(super) fn parse_params(&mut self) -> Vec<Param> {
        let opener = self.cur_span();
        self.bump(); // `(`
        let mut params = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                break;
            }
            if self.is_item_start() {
                break;
            }
            let before = self.pos;
            let p_start = self.cur_span();
            let kind = if self.at(Punct::Amp) {
                self.bump();
                let mutable = self.eat_kw(Kw::Mut);
                if self.at_kw(Kw::SelfValue) {
                    self.bump();
                    ParamKind::Receiver(if mutable {
                        Receiver::Exclusive
                    } else {
                        Receiver::Shared
                    })
                } else {
                    self.expected("`self`");
                    ParamKind::Receiver(Receiver::Shared)
                }
            } else if self.at_kw(Kw::SelfValue) {
                self.bump();
                ParamKind::Receiver(Receiver::ByValue)
            } else if self.at_kw(Kw::Mut) {
                // `mut name: Ty`: the mutability of the binding is now part of
                // the AST, so `push_str`/`push` on the parameter are accepted.
                let mutable = true;
                self.bump();
                let name = self.parse_ident("parameter name");
                if self.eat(Punct::Colon) {
                    let ty = self.parse_type();
                    ParamKind::Binding { mutable, name, ty }
                } else {
                    self.expected("`:` and a parameter type");
                    let span = self.zero_span();
                    let ty = self.infer_type(span);
                    ParamKind::Binding { mutable, name, ty }
                }
            } else if matches!(self.kind(), TokenKind::Ident(_)) {
                let mutable = false;
                let name = self.parse_ident("parameter name");
                if self.eat(Punct::Colon) {
                    let ty = self.parse_type();
                    ParamKind::Binding { mutable, name, ty }
                } else {
                    self.expected("`:` and a parameter type");
                    let span = self.zero_span();
                    let ty = self.infer_type(span);
                    ParamKind::Binding { mutable, name, ty }
                }
            } else {
                self.expected("a parameter or `)`");
                if !self.at(Punct::RParen) {
                    self.bump();
                }
                continue;
            };
            params.push(Param {
                id: self.id(),
                kind,
                span: self.span_from(p_start),
            });
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.at(Punct::RParen) {
                break;
            }
            self.expected("`,` or `)`");
            if self.pos == before {
                self.bump();
            }
        }
        self.expect_close(Punct::RParen, opener, "`)`");
        params
    }

    pub(super) fn parse_effects(&mut self) -> Option<Vec<Ident>> {
        if !self.at_kw(Kw::Effects) {
            return None;
        }
        self.bump(); // `effects`
        let opener = self.cur_span();
        if !self.eat(Punct::LParen) {
            self.expected("`(` after `effects`");
            return Some(Vec::new());
        }
        let mut out = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                break;
            }
            let before = self.pos;
            let effect = self.parse_effect_name();
            if let Some(effect) = effect {
                out.push(effect);
            }
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.at(Punct::RParen) {
                break;
            }
            self.expected("`,` or `)`");
            if self.pos == before {
                self.bump();
            }
        }
        self.expect_close(Punct::RParen, opener, "`)`");
        Some(out)
    }

    pub(super) fn parse_effect_name(&mut self) -> Option<Ident> {
        match self.kind().clone() {
            TokenKind::Ident(name) => {
                let span = self.cur_span();
                self.bump();
                Some(Ident::new(name, span))
            }
            TokenKind::Kw(kw @ (Kw::Unsafe | Kw::Async)) => {
                let span = self.cur_span();
                self.bump();
                Some(Ident::new(kw.as_str(), span))
            }
            _ => {
                self.expected("an effect name");
                None
            }
        }
    }

    pub(super) fn parse_struct_item(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `struct`
        let name = self.parse_ident("struct name");
        let generics = self.parse_generics();
        if self.eat(Punct::Semi) {
            return Some(ItemKind::Struct(StructItem {
                name,
                generics,
                fields: Vec::new(),
                invariants: Vec::new(),
                span: self.span_from(start),
            }));
        }
        if self.at(Punct::LParen) {
            let span = self.cur_span();
            self.diags.error(
                "E-UNSUPPORTED-TUPLE-STRUCT",
                span,
                "tuple structs (`struct S(...)`) are not supported in Ravum Core 1.0; use named fields",
            );
            self.skip_balanced();
            if self.at(Punct::Semi) {
                self.bump();
            }
            self.recover_item();
            return None;
        }
        if !self.at(Punct::LBrace) {
            self.expected("`{`, `(` or `;`");
            self.recover_item();
            return None;
        }
        let fields = self.parse_fields("struct");
        let invariants = self.parse_invariants(&name, &generics);
        Some(ItemKind::Struct(StructItem {
            name,
            generics,
            fields,
            invariants,
            span: self.span_from(start),
        }))
    }

    /// RFC-INVARIANTS-1: `invariant <expr>` clauses after the fields. `invariant`
    /// is contextual: it cannot start an item, so no program changes meaning.
    /// Each expression becomes the body of a synthetic pure method
    /// `pub fn __rv_invariant_<n>(&self) -> bool effects()` of a synthetic
    /// `impl` placed right after the struct (the checker, the ownership
    /// rules and both back ends then treat it as ordinary code).
    fn parse_invariants(&mut self, name: &Ident, generics: &[GenericParam]) -> Vec<Invariant> {
        let mut out = Vec::new();
        let mut methods = Vec::new();
        while matches!(self.kind(), TokenKind::Ident(w) if w == "invariant") {
            let start = self.cur_span();
            self.bump();
            let expr = self.parse_condition();
            let text = self.source_text(expr.span);
            let span = self.span_from(start);
            if !generics.is_empty() {
                self.diags.error(
                    "E-CON-UNSUPPORTED",
                    span,
                    "an `invariant` on a generic struct is not supported (RFC-INVARIANTS-1)",
                );
                continue;
            }
            let method = format!("{INVARIANT_METHOD_PREFIX}{}", out.len());
            methods.push(self.invariant_method(&method, expr, span));
            out.push(Invariant { text, method, span });
        }
        if !methods.is_empty() {
            let path = self.synthetic_path(&name.name, name.span);
            let self_ty = Type {
                id: self.id(),
                kind: TypeKind::Path(path),
                span: name.span,
            };
            let span = out[0].span;
            let imp = ItemKind::Impl(ImplItem {
                generics: Vec::new(),
                self_ty,
                items: methods,
                span,
            });
            let item = Item {
                id: self.id(),
                vis: Vis::Private,
                kind: imp,
                span,
            };
            self.pending_items.push(item);
        }
        out
    }

    fn synthetic_path(&mut self, name: &str, span: Span) -> Path {
        Path {
            id: self.id(),
            root: PathRoot::Plain,
            segments: vec![PathSegment {
                name: Ident::new(name, span),
                args: Vec::new(),
            }],
            span,
        }
    }

    fn invariant_method(&mut self, method: &str, expr: Expr, span: Span) -> Item {
        let param = Param {
            id: self.id(),
            kind: ParamKind::Receiver(Receiver::Shared),
            span,
        };
        let bool_path = self.synthetic_path("bool", span);
        let ret = Type {
            id: self.id(),
            kind: TypeKind::Path(bool_path),
            span,
        };
        let body = Block {
            id: self.id(),
            stmts: Vec::new(),
            tail: Some(Box::new(expr)),
            span,
        };
        let f = FnItem {
            name: Ident::new(method, span),
            generics: Vec::new(),
            params: vec![param],
            ret,
            effects: Some(Vec::new()),
            body,
            is_test: false,
            is_async: false,
            contracts: Vec::new(),
            span,
        };
        Item {
            id: self.id(),
            vis: Vis::Public(span),
            kind: ItemKind::Fn(f),
            span,
        }
    }

    pub(super) fn parse_fields(&mut self, what: &str) -> Vec<FieldDef> {
        let opener = self.cur_span();
        self.bump(); // `{`
        let mut fields = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                break;
            }
            let before = self.pos;
            let f_start = self.cur_span();
            let vis = self.parse_vis();
            let name = self.parse_ident("field name");
            let ty = if self.eat(Punct::Colon) {
                self.parse_type()
            } else {
                self.expected("`:`");
                let span = self.zero_span();
                self.infer_type(span)
            };
            fields.push(FieldDef {
                id: self.id(),
                vis,
                name,
                ty,
                span: self.span_from(f_start),
            });
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.at(Punct::RBrace) {
                break;
            }
            if self.is_item_start() {
                break;
            }
            self.expected("`,` or `}`");
            if self.pos == before {
                self.bump();
            }
        }
        self.expect_close(Punct::RBrace, opener, &format!("`}}` of the {what}"));
        fields
    }

    pub(super) fn parse_enum_item(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `enum`
        let name = self.parse_ident("enum name");
        let generics = self.parse_generics();
        if !self.at(Punct::LBrace) {
            self.expected("`{`");
            self.recover_item();
            return None;
        }
        let opener = self.cur_span();
        self.bump();
        let mut variants = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                break;
            }
            let before = self.pos;
            let v_start = self.cur_span();
            // `pub` on a variant is accepted but has no slot in the frozen AST;
            // Core variants inherit the enum visibility.
            let _ = self.parse_vis();
            let v_name = self.parse_ident("variant name");
            let fields = if self.at(Punct::LParen) {
                let open = self.cur_span();
                self.bump();
                let mut tys = Vec::new();
                loop {
                    if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                        break;
                    }
                    let ty_before = self.pos;
                    tys.push(self.parse_type());
                    if self.eat(Punct::Comma) {
                        if self.at(Punct::RParen) {
                            break;
                        }
                        continue;
                    }
                    if self.pos == ty_before {
                        self.bump();
                        continue;
                    }
                    break;
                }
                self.expect_close(Punct::RParen, open, "`)`");
                VariantDefFields::Tuple(tys)
            } else if self.at(Punct::LBrace) {
                VariantDefFields::Struct(self.parse_fields("variant"))
            } else {
                VariantDefFields::Unit
            };
            variants.push(VariantDef {
                id: self.id(),
                name: v_name,
                fields,
                span: self.span_from(v_start),
            });
            if self.eat(Punct::Comma) {
                continue;
            }
            if self.at(Punct::RBrace) {
                break;
            }
            if self.is_item_start() {
                break;
            }
            self.expected("`,` or `}`");
            if self.pos == before {
                self.bump();
            }
        }
        self.expect_close(Punct::RBrace, opener, "`}` of the enum");
        Some(ItemKind::Enum(EnumItem {
            name,
            generics,
            variants,
            span: self.span_from(start),
        }))
    }

    pub(super) fn parse_impl_item(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `impl`
        let generics = self.parse_generics();
        let self_ty = self.parse_type();
        if self.at_kw(Kw::For) {
            let span = self.cur_span();
            self.bump();
            // `impl Drop` has its own registered code (`E-UNSUPPORTED-DROP-IMPL`);
            // every other trait implementation is covered by `E-UNSUPPORTED-TRAIT`.
            let is_drop = matches!(
                &self_ty.kind,
                crate::ast::TypeKind::Path(path) if path.display() == "Drop"
            );
            self.diags.error(
                if is_drop {
                    "E-UNSUPPORTED-DROP-IMPL"
                } else {
                    "E-UNSUPPORTED-TRAIT"
                },
                span,
                "trait implementations (`impl Trait for Type`) are not supported in Ravum Core 1.0",
            );
            self.skip_to_brace_or_semi();
            if self.at(Punct::LBrace) {
                self.skip_balanced();
            }
            self.recover_item();
            return None;
        }
        if !self.at(Punct::LBrace) {
            self.expected("`{`");
            self.recover_item();
            return None;
        }
        let opener = self.cur_span();
        self.bump();
        let mut items = Vec::new();
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                break;
            }
            let before = self.pos;
            let item_start = self.cur_span();
            self.skip_attributes();
            let vis = self.parse_vis();
            let kind = match self.kind().clone() {
                TokenKind::Kw(Kw::Fn) => self.parse_fn_item(false),
                TokenKind::Kw(Kw::Const) => self.parse_const_item(),
                TokenKind::Kw(Kw::Trait) => self.unsupported_item("E-UNSUPPORTED-TRAIT", "`trait`"),
                TokenKind::Kw(Kw::Unsafe) => {
                    self.unsupported_item("E-UNSUPPORTED-UNSAFE", "`unsafe`")
                }
                TokenKind::Kw(Kw::Async) => self.unsupported_item("E-UNSUPPORTED-ASYNC", "`async`"),
                TokenKind::Kw(Kw::Extern) => {
                    self.unsupported_item("E-UNSUPPORTED-EXTERN", "`extern`")
                }
                _ => {
                    let found = self.describe_token();
                    self.unexpected(format!(
                        "expected a method or `}}` in the impl block, found {found}"
                    ));
                    self.recover_impl_item();
                    if self.pos == before {
                        self.bump();
                    }
                    None
                }
            };
            if let Some(kind) = kind {
                items.push(Item {
                    id: self.id(),
                    vis,
                    kind,
                    span: self.span_from(item_start),
                });
            }
            if self.pos == before && !self.fatal {
                self.bump();
            }
        }
        self.expect_close(Punct::RBrace, opener, "`}` of the impl block");
        Some(ItemKind::Impl(ImplItem {
            generics,
            self_ty,
            items,
            span: self.span_from(start),
        }))
    }

    pub(super) fn skip_to_brace_or_semi(&mut self) {
        let mut angles = 0i32;
        while !self.at_eof() {
            match self.kind() {
                TokenKind::Punct(Punct::LBrace) if angles == 0 => return,
                TokenKind::Punct(Punct::Semi) if angles == 0 => return,
                TokenKind::Punct(Punct::Lt) => {
                    angles += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::Gt) if angles > 0 => {
                    angles -= 1;
                    self.bump();
                }
                _ => self.bump(),
            }
        }
    }

    pub(super) fn recover_impl_item(&mut self) {
        let mut depth = 0i32;
        while !self.at_eof() && !self.fatal {
            if depth == 0 {
                if self.at(Punct::RBrace) {
                    return;
                }
                if self.at_kw(Kw::Fn) || self.at_kw(Kw::Const) {
                    return;
                }
            }
            match self.kind() {
                TokenKind::Punct(Punct::LBrace) => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::Punct(Punct::RBrace) => {
                    depth = (depth - 1).max(0);
                    self.bump();
                }
                _ => self.bump(),
            }
        }
    }

    pub(super) fn parse_const_item(&mut self) -> Option<ItemKind> {
        let start = self.cur_span();
        self.bump(); // `const`
        let name = self.parse_ident("constant name");
        let ty = if self.eat(Punct::Colon) {
            self.parse_type()
        } else {
            self.expected("`:` and a constant type");
            let span = self.zero_span();
            self.infer_type(span)
        };
        let value = if self.eat(Punct::Eq) {
            self.parse_expr(true)
        } else {
            self.expected("`=` and a constant value");
            let span = self.zero_span();
            self.unit_expr(span)
        };
        if !self.eat(Punct::Semi) {
            self.expected("`;`");
        }
        Some(ItemKind::Const(ConstItem {
            name,
            ty,
            value,
            span: self.span_from(start),
        }))
    }

    // ---------------------------------------------------------------------
    // Types
    // ---------------------------------------------------------------------
}
