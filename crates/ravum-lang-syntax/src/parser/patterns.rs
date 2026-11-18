use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_pattern_inner(&mut self) -> Pat {
        let start = self.cur_span();
        match self.kind().clone() {
            TokenKind::Kw(Kw::Underscore) => {
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Wild,
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::Mut) => {
                self.bump();
                let name = self.parse_ident("binding name");
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Binding {
                        mutable: true,
                        name,
                    },
                    span: self.span_from(start),
                }
            }
            TokenKind::Ident(ref name) if name == "ref" && self.starts_pattern_at(1) => {
                let span = start;
                self.diags.error(
                    "E-UNSUPPORTED-TOKEN",
                    span,
                    "`ref`/`ref mut` patterns are not supported in Ravum Core 1.0",
                );
                self.bump();
                self.eat_kw(Kw::Mut);
                if self.starts_pattern() {
                    let _ = self.parse_pattern();
                }
                self.wild_pat(self.span_from(start))
            }
            TokenKind::Ident(name) => {
                let binding_span = start;
                // G-6.41 (MUST): the four prelude constructors are constructor
                // patterns. Without this branch a bare `None` fell through to
                // the binding case, so `match o { None => …, Some(x) => … }`
                // made the first arm irrefutable: it bound the whole `Option`
                // and hid the exhaustiveness hole instead of matching `None`.
                let prelude_constructor = matches!(name.as_str(), "Some" | "None" | "Ok" | "Err");
                if prelude_constructor
                    || self.peek_punct(1, Punct::ColonColon)
                    || self.peek_punct(1, Punct::LParen)
                    || self.peek_punct(1, Punct::LBrace)
                    || self.peek_punct(1, Punct::DotDot)
                    || self.peek_punct(1, Punct::DotDotEq)
                {
                    let path = self.parse_path(true, false);
                    self.finish_path_pattern(path, start)
                } else {
                    self.bump();
                    let id = self.id();
                    Pat {
                        id,
                        kind: PatKind::Binding {
                            mutable: false,
                            name: Ident::new(name, binding_span),
                        },
                        span: self.span_from(start),
                    }
                }
            }
            // `effects` is contextual (LEX-5.5): outside a signature clause it is
            // an ordinary identifier, so `let effects = 1;` binds it.
            TokenKind::Kw(Kw::Effects) => {
                let binding_span = start;
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Binding {
                        mutable: false,
                        name: Ident::new("effects".to_string(), binding_span),
                    },
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::True) => {
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Bool(true)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Kw(Kw::False) => {
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Bool(false)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Int { value, suffix } => {
                let span = self.cur_span();
                self.bump();
                let suffix = self.int_suffix(suffix, span);
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Int {
                        value,
                        suffix,
                        neg: false,
                    }),
                    span: self.span_from(start),
                }
            }
            // G-6.36: a float literal is a valid pattern, compared with `==`.
            TokenKind::Float { text, suffix } => {
                let span = self.cur_span();
                self.bump();
                let suffix = self.float_suffix(suffix, span);
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Float { text, suffix }),
                    span: self.span_from(start),
                }
            }
            TokenKind::Str(value) => {
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Str(value)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Char(value) => {
                self.bump();
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Lit(LitPat::Char(value)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Punct(Punct::LParen) => {
                let opener = start;
                self.bump();
                if self.eat(Punct::RParen) {
                    let id = self.id();
                    return Pat {
                        id,
                        kind: PatKind::Tuple(Vec::new()),
                        span: self.span_from(start),
                    };
                }
                let mut pats = Vec::new();
                loop {
                    if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                        break;
                    }
                    let before = self.pos;
                    pats.push(self.parse_pattern());
                    if self.eat(Punct::Comma) {
                        if self.at(Punct::RParen) {
                            break;
                        }
                        continue;
                    }
                    if self.pos == before {
                        self.bump();
                    }
                    break;
                }
                self.expect_close(Punct::RParen, opener, "`)`");
                let id = self.id();
                Pat {
                    id,
                    kind: PatKind::Tuple(pats),
                    span: self.span_from(start),
                }
            }
            // G-6.37 (MUST): `-N` is admitted for an integer and for a float.
            // Any other negative form (`-x`, `-(1 + 2)`) is `E-PARSE-EXPECTED`.
            TokenKind::Punct(Punct::Minus) => {
                self.bump();
                match self.kind().clone() {
                    TokenKind::Int { value, suffix } => {
                        let span = self.cur_span();
                        self.bump();
                        let suffix = self.int_suffix(suffix, span);
                        let id = self.id();
                        Pat {
                            id,
                            kind: PatKind::Lit(LitPat::Int {
                                value,
                                suffix,
                                neg: true,
                            }),
                            span: self.span_from(start),
                        }
                    }
                    TokenKind::Float { text, suffix } => {
                        let span = self.cur_span();
                        self.bump();
                        let suffix = self.float_suffix(suffix, span);
                        let id = self.id();
                        Pat {
                            id,
                            kind: PatKind::Lit(LitPat::Float {
                                text: format!("-{text}"),
                                suffix,
                            }),
                            span: self.span_from(start),
                        }
                    }
                    _ => {
                        self.expected("an integer or float literal after `-` in a pattern");
                        if !self.is_pattern_stopper() && !self.at_eof() {
                            self.bump();
                        }
                        self.wild_pat(self.span_from(start))
                    }
                }
            }
            TokenKind::Punct(Punct::DotDot) | TokenKind::Punct(Punct::DotDotEq) => {
                let span = start;
                let eq = self.at(Punct::DotDotEq);
                self.bump();
                self.diags.error(
                    "E-UNSUPPORTED-REST-PAT",
                    span,
                    if eq {
                        "`..=` rest patterns are not supported in Ravum Core 1.0"
                    } else {
                        "`..` rest patterns are not supported in Ravum Core 1.0"
                    },
                );
                self.wild_pat(self.span_from(start))
            }
            TokenKind::Kw(Kw::Crate) | TokenKind::Kw(Kw::Super) => {
                let path = self.parse_path(true, false);
                self.finish_path_pattern(path, start)
            }
            TokenKind::Kw(Kw::SelfValue) if self.peek_punct(1, Punct::ColonColon) => {
                let path = self.parse_path(true, false);
                self.finish_path_pattern(path, start)
            }
            TokenKind::Kw(Kw::SelfType) => {
                self.unexpected("`Self` is not a valid pattern");
                self.bump();
                self.wild_pat(self.span_from(start))
            }
            _ => {
                self.expected("a pattern");
                if !self.is_pattern_stopper() && !self.at_eof() {
                    self.bump();
                }
                self.wild_pat(self.span_from(start))
            }
        }
    }

    pub(super) fn finish_path_pattern(&mut self, path: Path, start: Span) -> Pat {
        if self.at(Punct::LParen) {
            let opener = self.cur_span();
            self.bump();
            let mut pats = Vec::new();
            loop {
                if self.fatal || self.at_eof() || self.at(Punct::RParen) {
                    break;
                }
                let before = self.pos;
                pats.push(self.parse_pattern());
                if self.eat(Punct::Comma) {
                    if self.at(Punct::RParen) {
                        break;
                    }
                    continue;
                }
                if self.pos == before {
                    self.bump();
                }
                break;
            }
            self.expect_close(Punct::RParen, opener, "`)`");
            let id = self.id();
            Pat {
                id,
                kind: PatKind::Variant {
                    path,
                    fields: VariantPatFields::Tuple(pats),
                },
                span: self.span_from(start),
            }
        } else if self.at(Punct::LBrace) {
            let opener = self.cur_span();
            self.bump();
            let mut fields = Vec::new();
            loop {
                if self.fatal || self.at_eof() || self.at(Punct::RBrace) {
                    break;
                }
                let before = self.pos;
                if self.at(Punct::DotDot) || self.at(Punct::DotDotEq) {
                    let span = self.cur_span();
                    self.bump();
                    self.diags.error(
                        "E-UNSUPPORTED-REST-PAT",
                        span,
                        "struct rest patterns (`..`) are not supported in Ravum Core 1.0",
                    );
                    break;
                }
                let name = self.parse_ident("field name");
                let pat = if self.eat(Punct::Colon) {
                    self.parse_pattern()
                } else {
                    let id = self.id();
                    Pat {
                        id,
                        kind: PatKind::Binding {
                            mutable: false,
                            name: name.clone(),
                        },
                        span: name.span,
                    }
                };
                fields.push(FieldPat { name, pat });
                if self.eat(Punct::Comma) {
                    continue;
                }
                if self.at(Punct::RBrace) {
                    break;
                }
                self.expected("`,` or `}`");
                if self.pos == before {
                    self.bump();
                }
            }
            self.expect_close(Punct::RBrace, opener, "`}`");
            let id = self.id();
            Pat {
                id,
                kind: PatKind::Variant {
                    path,
                    fields: VariantPatFields::Struct(fields),
                },
                span: self.span_from(start),
            }
        } else {
            let id = self.id();
            Pat {
                id,
                kind: PatKind::Path(path),
                span: self.span_from(start),
            }
        }
    }

    // ---------------------------------------------------------------------
    // Expressions
    // ---------------------------------------------------------------------
}
