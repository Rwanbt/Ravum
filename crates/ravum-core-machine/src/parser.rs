//! Recursive-descent parser of the pure core, written from
//! `spec/language/grammar.md` without reusing `ravum-lang-syntax`.

use crate::ast::*;
use crate::lexer::Tok;

pub struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

type PResult<T> = Result<T, String>;

impl Parser {
    pub fn new(toks: Vec<Tok>) -> Self {
        Parser { toks, pos: 0 }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos]
    }

    fn peek_at(&self, n: usize) -> &Tok {
        self.toks.get(self.pos + n).unwrap_or(&Tok::Eof)
    }

    fn bump(&mut self) -> Tok {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(q) if *q == p)
    }

    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Ident(w) if w == k)
    }

    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, k: &str) -> bool {
        if self.is_kw(k) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, p: &str) -> PResult<()> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(format!("expected `{p}`, found {:?}", self.peek()))
        }
    }

    fn ident(&mut self) -> PResult<String> {
        match self.bump() {
            Tok::Ident(w) => Ok(w),
            t => Err(format!("expected identifier, found {t:?}")),
        }
    }

    pub fn program(&mut self) -> PResult<Vec<Item>> {
        let mut items = Vec::new();
        while *self.peek() != Tok::Eof {
            // `use` lines carry no meaning for the machine.
            if self.eat_kw("use") {
                while !self.eat(";") {
                    self.bump();
                }
                continue;
            }
            self.eat_kw("pub");
            if self.eat_kw("fn") {
                items.push(Item::Func(self.func()?));
            } else if self.eat_kw("struct") {
                let name = self.ident()?;
                self.expect("{")?;
                let mut fields = Vec::new();
                while !self.eat("}") {
                    self.eat_kw("pub");
                    let f = self.ident()?;
                    self.expect(":")?;
                    fields.push((f, self.ty()?));
                    if !self.eat(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                items.push(Item::Struct(name, fields));
            } else if self.eat_kw("enum") {
                let name = self.ident()?;
                self.expect("{")?;
                let mut vars = Vec::new();
                while !self.eat("}") {
                    let v = self.ident()?;
                    let mut tys = Vec::new();
                    if self.eat("(") {
                        while !self.eat(")") {
                            tys.push(self.ty()?);
                            if !self.eat(",") {
                                self.expect(")")?;
                                break;
                            }
                        }
                    }
                    vars.push((v, tys));
                    if !self.eat(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                items.push(Item::Enum(name, vars));
            } else if self.eat_kw("const") {
                let name = self.ident()?;
                self.expect(":")?;
                let ty = self.ty()?;
                self.expect("=")?;
                let e = self.expr()?;
                self.expect(";")?;
                items.push(Item::Const(name, ty, e));
            } else {
                return Err(format!("unsupported item at {:?}", self.peek()));
            }
        }
        Ok(items)
    }

    fn func(&mut self) -> PResult<Func> {
        let name = self.ident()?;
        self.expect("(")?;
        let mut params = Vec::new();
        while !self.eat(")") {
            self.eat_kw("mut");
            let p = self.ident()?;
            self.expect(":")?;
            params.push((p, self.ty()?));
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        let ret = if self.eat("->") { self.ty()? } else { Ty::Unit };
        let effects = if self.eat_kw("effects") {
            self.expect("(")?;
            let mut list = Vec::new();
            while !self.eat(")") {
                list.push(self.ident()?);
                if !self.eat(",") {
                    self.expect(")")?;
                    break;
                }
            }
            Some(list)
        } else {
            None
        };
        let body = self.block()?;
        Ok(Func {
            name,
            params,
            ret,
            effects,
            body,
        })
    }

    pub fn ty(&mut self) -> PResult<Ty> {
        if self.eat("(") {
            let mut tys = Vec::new();
            while !self.eat(")") {
                tys.push(self.ty()?);
                if !self.eat(",") {
                    self.expect(")")?;
                    break;
                }
            }
            return Ok(if tys.is_empty() {
                Ty::Unit
            } else {
                Ty::Tuple(tys)
            });
        }
        if self.eat("[") {
            let elem = self.ty()?;
            self.expect(";")?;
            let n = match self.bump() {
                Tok::Int(n, _) => n as usize,
                t => return Err(format!("expected array length, found {t:?}")),
            };
            self.expect("]")?;
            return Ok(Ty::Array(Box::new(elem), n));
        }
        if self.eat("&") {
            let name = self.ident()?;
            return if name == "str" {
                Ok(Ty::Str)
            } else {
                Err("references other than &str are outside the pure core".into())
            };
        }
        let name = self.ident()?;
        if let Some(t) = IntTy::from_name(&name) {
            return Ok(Ty::Int(t));
        }
        Ok(match name.as_str() {
            "f32" => Ty::Float(FloatTy::F32),
            "f64" => Ty::Float(FloatTy::F64),
            "bool" => Ty::Bool,
            "char" => Ty::Char,
            _ => {
                let mut args = Vec::new();
                if self.eat("<") {
                    loop {
                        args.push(self.ty()?);
                        if self.eat(">") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                Ty::Named(name, args)
            }
        })
    }

    fn block(&mut self) -> PResult<Block> {
        self.expect("{")?;
        let mut stmts = Vec::new();
        let mut tail = None;
        while !self.eat("}") {
            if self.eat(";") {
                continue;
            }
            if self.eat_kw("let") {
                self.eat_kw("mut");
                let pat = self.pattern()?;
                let ty = if self.eat(":") {
                    Some(self.ty()?)
                } else {
                    None
                };
                self.expect("=")?;
                let e = self.expr()?;
                self.expect(";")?;
                stmts.push(Stmt::Let(pat, ty, e));
                continue;
            }
            let e = self.expr_stmt()?;
            if self.eat(";") {
                stmts.push(Stmt::Expr(e));
            } else if self.is("}") {
                tail = Some(Box::new(e));
            } else if ends_with_block(&e) {
                stmts.push(Stmt::Expr(e));
            } else {
                return Err(format!("expected `;`, found {:?}", self.peek()));
            }
        }
        Ok(Block { stmts, tail })
    }

    fn expr_stmt(&mut self) -> PResult<Expr> {
        let lhs = self.expr()?;
        for (p, op) in [
            ("+=", BinOp::Add),
            ("-=", BinOp::Sub),
            ("*=", BinOp::Mul),
            ("/=", BinOp::Div),
            ("%=", BinOp::Rem),
            ("&=", BinOp::BitAnd),
            ("|=", BinOp::BitOr),
            ("^=", BinOp::BitXor),
            ("<<=", BinOp::Shl),
            (">>=", BinOp::Shr),
        ] {
            if self.eat(p) {
                let rhs = self.expr()?;
                return Ok(Expr::AssignOp(op, Box::new(lhs), Box::new(rhs)));
            }
        }
        if self.eat("=") {
            let rhs = self.expr()?;
            return Ok(Expr::Assign(Box::new(lhs), Box::new(rhs)));
        }
        Ok(lhs)
    }

    pub fn expr(&mut self) -> PResult<Expr> {
        self.binary(0, true)
    }

    fn expr_no_struct(&mut self) -> PResult<Expr> {
        self.binary(0, false)
    }

    fn binary(&mut self, min: u8, structs: bool) -> PResult<Expr> {
        let mut lhs = self.unary(structs)?;
        while let Some((op, prec)) = self.binop() {
            if prec < min {
                break;
            }
            self.bump();
            let rhs = self.binary(prec + 1, structs)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn binop(&self) -> Option<(BinOp, u8)> {
        let Tok::Punct(p) = self.peek() else {
            return None;
        };
        Some(match *p {
            "||" => (BinOp::Or, 1),
            "&&" => (BinOp::And, 2),
            "==" => (BinOp::Eq, 3),
            "!=" => (BinOp::Ne, 3),
            "<" => (BinOp::Lt, 3),
            "<=" => (BinOp::Le, 3),
            ">" => (BinOp::Gt, 3),
            ">=" => (BinOp::Ge, 3),
            "|" => (BinOp::BitOr, 4),
            "^" => (BinOp::BitXor, 5),
            "&" => (BinOp::BitAnd, 6),
            "<<" => (BinOp::Shl, 7),
            ">>" => (BinOp::Shr, 7),
            "+" => (BinOp::Add, 8),
            "-" => (BinOp::Sub, 8),
            "*" => (BinOp::Mul, 9),
            "/" => (BinOp::Div, 9),
            "%" => (BinOp::Rem, 9),
            _ => return None,
        })
    }

    fn unary(&mut self, structs: bool) -> PResult<Expr> {
        if self.eat("-") {
            return Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary(structs)?)));
        }
        if self.eat("!") {
            return Ok(Expr::Unary(UnOp::Not, Box::new(self.unary(structs)?)));
        }
        let mut e = self.postfix(structs)?;
        while self.eat_kw("as") {
            e = Expr::Cast(Box::new(e), self.ty()?);
        }
        Ok(e)
    }

    fn postfix(&mut self, structs: bool) -> PResult<Expr> {
        let mut e = self.primary(structs)?;
        loop {
            if self.eat(".") {
                match self.bump() {
                    Tok::Int(n, _) => e = Expr::TupleField(Box::new(e), n as usize),
                    Tok::Float(_, _) => return Err("nested tuple field needs parentheses".into()),
                    Tok::Ident(name) => {
                        if self.eat("(") {
                            let args = self.args()?;
                            e = Expr::Method(Box::new(e), name, args);
                        } else {
                            e = Expr::Field(Box::new(e), name);
                        }
                    }
                    t => return Err(format!("unexpected {t:?} after `.`")),
                }
            } else if self.eat("[") {
                let idx = self.expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(idx));
            } else if self.is("(") {
                self.bump();
                let args = self.args()?;
                e = Expr::Call(Box::new(e), args);
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn args(&mut self) -> PResult<Vec<Expr>> {
        let mut args = Vec::new();
        while !self.eat(")") {
            args.push(self.expr()?);
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok(args)
    }

    fn primary(&mut self, structs: bool) -> PResult<Expr> {
        match self.peek().clone() {
            Tok::Int(v, s) => {
                self.bump();
                let ty = match s.as_deref() {
                    None => None,
                    Some(name) => Some(IntTy::from_name(name).ok_or("bad suffix")?),
                };
                Ok(Expr::Lit(Lit::Int(v, ty)))
            }
            Tok::Float(v, s) => {
                self.bump();
                let ty = match s.as_deref() {
                    Some("f32") => Some(FloatTy::F32),
                    Some(_) => Some(FloatTy::F64),
                    None => None,
                };
                Ok(Expr::Lit(Lit::Float(v, ty)))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Expr::Lit(Lit::Str(s)))
            }
            Tok::Char(c) => {
                self.bump();
                Ok(Expr::Lit(Lit::Char(c)))
            }
            Tok::Punct("(") => {
                self.bump();
                let mut items = Vec::new();
                let mut trailing = false;
                while !self.eat(")") {
                    items.push(self.expr()?);
                    trailing = self.eat(",");
                    if !trailing {
                        self.expect(")")?;
                        break;
                    }
                }
                Ok(match items.len() {
                    0 => Expr::Tuple(vec![]),
                    1 if !trailing => items.pop().unwrap(),
                    _ => Expr::Tuple(items),
                })
            }
            Tok::Punct("[") => {
                self.bump();
                if self.eat("]") {
                    return Err("empty array".into());
                }
                let first = self.expr()?;
                if self.eat(";") {
                    let n = self.expr()?;
                    self.expect("]")?;
                    return Ok(Expr::ArrayRepeat(Box::new(first), Box::new(n)));
                }
                let mut items = vec![first];
                while self.eat(",") {
                    if self.is("]") {
                        break;
                    }
                    items.push(self.expr()?);
                }
                self.expect("]")?;
                Ok(Expr::Array(items))
            }
            Tok::Punct("{") => Ok(Expr::Block(self.block()?)),
            Tok::Ident(w) => match w.as_str() {
                "true" | "false" => {
                    self.bump();
                    Ok(Expr::Lit(Lit::Bool(w == "true")))
                }
                "if" => self.if_expr(),
                "match" => {
                    self.bump();
                    let scrut = self.expr_no_struct()?;
                    self.expect("{")?;
                    let mut arms = Vec::new();
                    while !self.eat("}") {
                        let pat = self.pattern()?;
                        self.expect("=>")?;
                        let body = self.expr()?;
                        let block_body = matches!(body, Expr::Block(_));
                        arms.push(Arm { pat, body });
                        if !self.eat(",") && !block_body {
                            self.expect("}")?;
                            break;
                        }
                    }
                    Ok(Expr::Match(Box::new(scrut), arms))
                }
                "while" => {
                    self.bump();
                    let c = self.expr_no_struct()?;
                    Ok(Expr::While(Box::new(c), self.block()?))
                }
                "loop" => {
                    self.bump();
                    Ok(Expr::Loop(self.block()?))
                }
                "for" => {
                    self.bump();
                    let var = match self.bump() {
                        Tok::Ident(v) => v,
                        Tok::Punct("_") => "_".into(),
                        t => return Err(format!("expected loop variable, found {t:?}")),
                    };
                    if !self.eat_kw("in") {
                        return Err("expected `in`".into());
                    }
                    let lo = self.binary(8, false)?;
                    if !self.eat("..") {
                        return Err("the machine only iterates over ranges `a..b`".into());
                    }
                    let hi = self.binary(8, false)?;
                    Ok(Expr::For(var, Box::new(lo), Box::new(hi), self.block()?))
                }
                "break" => {
                    self.bump();
                    Ok(Expr::Break)
                }
                "continue" => {
                    self.bump();
                    Ok(Expr::Continue)
                }
                "return" => {
                    self.bump();
                    if self.is(";") || self.is("}") {
                        Ok(Expr::Return(None))
                    } else {
                        Ok(Expr::Return(Some(Box::new(self.expr()?))))
                    }
                }
                _ => {
                    let path = self.path()?;
                    if structs && self.is("{") && starts_uppercase(path.last().unwrap()) {
                        self.bump();
                        let mut fields = Vec::new();
                        while !self.eat("}") {
                            let f = self.ident()?;
                            self.expect(":")?;
                            fields.push((f, self.expr()?));
                            if !self.eat(",") {
                                self.expect("}")?;
                                break;
                            }
                        }
                        return Ok(Expr::StructLit(path, fields));
                    }
                    Ok(Expr::Path(path))
                }
            },
            t => Err(format!("unexpected token {t:?}")),
        }
    }

    fn path(&mut self) -> PResult<Vec<String>> {
        let mut path = vec![self.ident()?];
        while self.is("::") {
            self.bump();
            path.push(self.ident()?);
        }
        Ok(path)
    }

    fn if_expr(&mut self) -> PResult<Expr> {
        self.bump();
        let cond = self.expr_no_struct()?;
        let then = self.block()?;
        let other = if self.eat_kw("else") {
            if self.is_kw("if") {
                Some(Box::new(self.if_expr()?))
            } else {
                Some(Box::new(Expr::Block(self.block()?)))
            }
        } else {
            None
        };
        Ok(Expr::If(Box::new(cond), then, other))
    }

    fn pattern(&mut self) -> PResult<Pat> {
        let first = self.pattern_one()?;
        if self.is("|") {
            // Or-patterns are not part of Core 1.0 (`E-UNSUPPORTED-TOKEN`).
            return Err("or-patterns are not part of Core 1.0".into());
        }
        Ok(first)
    }

    fn pat_lit(&mut self) -> PResult<Option<(Lit, bool)>> {
        let neg = if self.is("-") && matches!(self.peek_at(1), Tok::Int(..) | Tok::Float(..)) {
            self.bump();
            true
        } else {
            false
        };
        Ok(Some(match self.peek().clone() {
            Tok::Int(v, s) => {
                self.bump();
                (Lit::Int(v, s.as_deref().and_then(IntTy::from_name)), neg)
            }
            Tok::Float(v, _) => {
                self.bump();
                (Lit::Float(v, None), neg)
            }
            Tok::Char(c) => {
                self.bump();
                (Lit::Char(c), false)
            }
            Tok::Str(s) => {
                self.bump();
                (Lit::Str(s), false)
            }
            Tok::Ident(w) if w == "true" || w == "false" => {
                self.bump();
                (Lit::Bool(w == "true"), false)
            }
            _ => return Ok(None),
        }))
    }

    fn pattern_one(&mut self) -> PResult<Pat> {
        if self.eat("_") {
            return Ok(Pat::Wild);
        }
        if let Some((lit, neg)) = self.pat_lit()? {
            if self.is("..=") || self.is("..") {
                // Range patterns are not part of Core 1.0
                // (`E-UNSUPPORTED-RANGE-PAT`).
                return Err("range patterns are not part of Core 1.0".into());
            }
            return Ok(Pat::Lit(lit, neg));
        }
        if self.eat("(") {
            let mut items = Vec::new();
            while !self.eat(")") {
                items.push(self.pattern()?);
                if !self.eat(",") {
                    self.expect(")")?;
                    break;
                }
            }
            return Ok(Pat::Tuple(items));
        }
        self.eat_kw("mut");
        let path = self.path()?;
        if self.eat("(") {
            let mut items = Vec::new();
            while !self.eat(")") {
                items.push(self.pattern()?);
                if !self.eat(",") {
                    self.expect(")")?;
                    break;
                }
            }
            return Ok(Pat::Variant(path, items));
        }
        if path.len() == 1 && !starts_uppercase(&path[0]) {
            return Ok(Pat::Bind(path.into_iter().next().unwrap()));
        }
        Ok(Pat::Variant(path, vec![]))
    }
}

fn starts_uppercase(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn ends_with_block(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Block(_)
            | Expr::If(..)
            | Expr::Match(..)
            | Expr::While(..)
            | Expr::Loop(_)
            | Expr::For(..)
    )
}
