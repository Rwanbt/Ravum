use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_type(&mut self) -> Type {
        if self.fatal {
            return self.infer_type(self.cur_span());
        }
        if self.depth >= MAX_DEPTH {
            self.fail_depth();
            return self.infer_type(self.cur_span());
        }
        self.descend();
        let ty = self.parse_type_inner();
        self.depth -= 1;
        ty
    }

    pub(super) fn parse_type_inner(&mut self) -> Type {
        let start = self.cur_span();
        if let Some(hash) = self.attribute_hash_pos() {
            self.report_attribute(hash);
            return self.infer_type(self.span_from(start));
        }
        match self.kind().clone() {
            TokenKind::Punct(Punct::Amp) => {
                self.bump();
                let mutable = self.eat_kw(Kw::Mut);
                let inner = self.parse_type();
                let id = self.id();
                Type {
                    id,
                    kind: TypeKind::Ref {
                        mutable,
                        inner: Box::new(inner),
                    },
                    span: self.span_from(start),
                }
            }
            TokenKind::Lifetime(_) => {
                // Already refused by the lexer; skipping it lets `&''a i32` be
                // read as `&i32` instead of producing a second diagnostic.
                self.bump();
                self.parse_type()
            }
            TokenKind::Punct(Punct::Star) => {
                self.diags.error(
                    "E-UNSUPPORTED-UNSAFE",
                    start,
                    "raw pointer types (`*const T` / `*mut T`) are not supported in Ravum Core 1.0",
                );
                self.bump(); // `*`
                if self.at_kw(Kw::Const) || self.at_kw(Kw::Mut) {
                    self.bump();
                }
                self.skip_type_recovery();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Punct(Punct::LBracket) => {
                let opener = start;
                self.bump();
                let elem = self.parse_type();
                if self.eat(Punct::Semi) {
                    let len = self.parse_expr(false);
                    self.expect_close(Punct::RBracket, opener, "`]`");
                    let id = self.id();
                    Type {
                        id,
                        kind: TypeKind::Array {
                            elem: Box::new(elem),
                            len: Box::new(len),
                        },
                        span: self.span_from(start),
                    }
                } else {
                    self.expect_close(Punct::RBracket, opener, "`]`");
                    let id = self.id();
                    Type {
                        id,
                        kind: TypeKind::Slice(Box::new(elem)),
                        span: self.span_from(start),
                    }
                }
            }
            TokenKind::Punct(Punct::LParen) => {
                self.bump();
                if self.eat(Punct::RParen) {
                    let id = self.id();
                    return Type {
                        id,
                        kind: TypeKind::Unit,
                        span: self.span_from(start),
                    };
                }
                let first = self.parse_type();
                if self.eat(Punct::Comma) {
                    let mut elems = vec![first];
                    loop {
                        if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                            break;
                        }
                        let before = self.pos;
                        elems.push(self.parse_type());
                        if self.eat(Punct::Comma) {
                            continue;
                        }
                        if self.pos == before {
                            self.bump();
                        }
                        break;
                    }
                    self.expect_close(Punct::RParen, start, "`)`");
                    let id = self.id();
                    Type {
                        id,
                        kind: TypeKind::Tuple(elems),
                        span: self.span_from(start),
                    }
                } else {
                    self.expect_close(Punct::RParen, start, "`)`");
                    // Parenthesized type: return the inner node unchanged.
                    first
                }
            }
            TokenKind::Kw(Kw::Underscore) => {
                self.bump();
                let id = self.id();
                Type {
                    id,
                    kind: TypeKind::Infer,
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::SelfType) => {
                if self.peek_punct(1, Punct::ColonColon) {
                    self.diags.error(
                        "E-UNSUPPORTED-ASSOC-TYPE",
                        start,
                        "associated types on `Self` are not supported in Ravum Core 1.0",
                    );
                    self.bump_n(2);
                    self.skip_type_recovery();
                    self.infer_type(self.span_from(start))
                } else {
                    self.bump();
                    let id = self.id();
                    Type {
                        id,
                        kind: TypeKind::SelfType,
                        span: self.span_from(start),
                    }
                }
            }
            TokenKind::Kw(Kw::Impl) => {
                self.diags.error(
                    "E-UNSUPPORTED-IMPL-TRAIT",
                    start,
                    "`impl Trait` return/argument types are not supported in Ravum Core 1.0",
                );
                self.bump();
                self.skip_type_recovery();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Kw(Kw::Fn) => {
                self.diags.error(
                    "E-UNSUPPORTED-FN-TYPE",
                    start,
                    "function-pointer types (`fn(...)`) are not supported in Ravum Core 1.0",
                );
                self.bump();
                self.skip_type_recovery();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Kw(Kw::Trait) => {
                self.diags.error(
                    "E-UNSUPPORTED-TRAIT",
                    start,
                    "trait objects are not supported in Ravum Core 1.0",
                );
                self.bump();
                self.skip_type_recovery();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Punct(Punct::Bang) => {
                // G-5.11 / TYP-1.6: `!` is nameable. It is legal as the whole
                // return type of a divergent function; the checker rejects it in
                // parameters, fields, generic arguments, and on a body that
                // completes normally (E-TYP-RETURN).
                self.bump();
                self.never_type(self.span_from(start))
            }
            TokenKind::Ident(ref name) if name == "dyn" => {
                self.diags.error(
                    "E-UNSUPPORTED-TRAIT",
                    start,
                    "trait objects (`dyn Trait`) are not supported in Ravum Core 1.0",
                );
                self.bump();
                self.skip_type_recovery();
                self.infer_type(self.span_from(start))
            }
            TokenKind::Ident(_)
            | TokenKind::Kw(Kw::Crate)
            | TokenKind::Kw(Kw::Super)
            | TokenKind::Kw(Kw::SelfValue) => {
                let path = self.parse_path(true, true);
                let id = self.id();
                Type {
                    id,
                    kind: TypeKind::Path(path),
                    span: self.span_from(start),
                }
            }
            _ => {
                self.expected("a type");
                if !self.is_type_stopper() && !self.at_eof() {
                    self.bump();
                }
                self.infer_type(self.span_from(start))
            }
        }
    }

    pub(super) fn skip_type_recovery(&mut self) {
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
                TokenKind::Punct(
                    Punct::Comma
                    | Punct::RParen
                    | Punct::RBracket
                    | Punct::Semi
                    | Punct::LBrace
                    | Punct::RBrace
                    | Punct::Eq
                    | Punct::FatArrow,
                ) if angles == 0 => break,
                TokenKind::Punct(Punct::LParen | Punct::LBracket) if angles == 0 => {
                    self.skip_balanced();
                }
                _ => self.bump(),
            }
        }
    }

    pub(super) fn parse_type_args(&mut self, opener: Span) -> Vec<Type> {
        let mut out = Vec::new();
        if self.at(Punct::Gt) {
            self.expected("generic argument type");
            self.bump();
            return out;
        }
        loop {
            if self.fatal || self.at_eof() || self.at(Punct::Gt) {
                break;
            }
            let before = self.pos;
            out.push(self.parse_type());
            if self.eat(Punct::Comma) {
                if self.at(Punct::Gt) {
                    break;
                }
                continue;
            }
            if self.pos == before && !self.at(Punct::Gt) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_gt(opener, "unclosed generic argument list");
        out
    }

    // ---------------------------------------------------------------------
    // Paths
    // ---------------------------------------------------------------------

    pub(super) fn parse_path(&mut self, allow_generics: bool, type_ctx: bool) -> Path {
        let start = self.cur_span();
        let mut root = PathRoot::Plain;
        if self.at_kw(Kw::Crate) {
            self.bump();
            root = PathRoot::Crate;
            if !self.eat(Punct::ColonColon) {
                self.expected("`::` after `crate`");
            }
        } else if self.at_kw(Kw::Super) {
            self.bump();
            root = PathRoot::Super;
            if !self.eat(Punct::ColonColon) {
                self.expected("`::` after `super`");
            }
        } else if self.at_kw(Kw::SelfValue) && self.peek_punct(1, Punct::ColonColon) {
            self.bump_n(2);
            root = PathRoot::SelfMod;
        }

        let mut segments = Vec::new();
        loop {
            if self.fatal {
                break;
            }
            match self.kind().clone() {
                TokenKind::Ident(name) => {
                    let seg_span = self.cur_span();
                    self.bump();
                    let mut args = Vec::new();
                    if allow_generics {
                        if type_ctx && self.at(Punct::Lt) {
                            let opener = self.cur_span();
                            self.bump();
                            args = self.parse_type_args(opener);
                        } else if self.at(Punct::ColonColon) && self.peek_punct(1, Punct::Lt) {
                            self.bump(); // `::`
                            let opener = self.cur_span();
                            self.bump(); // `<`
                            args = self.parse_type_args(opener);
                        }
                    }
                    segments.push(PathSegment {
                        name: Ident::new(name, seg_span),
                        args,
                    });
                    if self.at(Punct::ColonColon) && self.peek_ident() {
                        self.bump(); // `::`
                        continue;
                    }
                    break;
                }
                _ => {
                    self.expected("path segment");
                    break;
                }
            }
        }
        Path {
            id: self.id(),
            root,
            segments,
            span: self.span_from(start),
        }
    }

    // ---------------------------------------------------------------------
    // Blocks and statements
    // ---------------------------------------------------------------------
}
