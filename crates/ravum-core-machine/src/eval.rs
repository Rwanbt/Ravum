//! Evaluation rules of §4 and §6, budget of §7.

use crate::ast::*;
use crate::value::*;
use std::collections::HashMap;

/// MACH-7.3: deepest call nesting the machine accepts before it reports
/// `fault: stack-limit` instead of overflowing its own stack.
pub const MAX_CALL_DEPTH: usize = 2_000;

/// `sin`, `cos`, ... are outside the verifiable core (MACH-5.4, MACH-5.5).
const LIBM: [&str; 9] = [
    "sin", "cos", "tan", "exp", "ln", "log10", "pow", "powf", "powi",
];

pub struct Machine {
    funcs: HashMap<String, Func>,
    structs: HashMap<String, Vec<(String, Ty)>>,
    enums: HashMap<String, Vec<(String, Vec<Ty>)>>,
    consts: HashMap<String, (Ty, Expr)>,
    scopes: Vec<HashMap<String, Value>>,
    steps: u64,
    budget: u64,
    depth: usize,
}

impl Machine {
    pub fn new(items: Vec<Item>, budget: u64) -> Self {
        let mut m = Machine {
            funcs: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            consts: HashMap::new(),
            scopes: Vec::new(),
            steps: 0,
            budget,
            depth: 0,
        };
        for item in items {
            match item {
                Item::Func(f) => {
                    m.funcs.insert(f.name.clone(), f);
                }
                Item::Struct(n, fs) => {
                    m.structs.insert(n, fs);
                }
                Item::Enum(n, vs) => {
                    m.enums.insert(n, vs);
                }
                Item::Const(n, t, e) => {
                    m.consts.insert(n, (t, e));
                }
            }
        }
        m
    }

    pub fn function(&self, name: &str) -> Option<&Func> {
        self.funcs.get(name)
    }

    fn tick(&mut self) -> Eval<()> {
        self.steps += 1;
        if self.steps > self.budget {
            return Err(Stop::Diverge);
        }
        Ok(())
    }

    // ---------------------------------------------------------------- calls

    pub fn call(&mut self, name: &str, args: Vec<Value>) -> Eval<Value> {
        let Some(f) = self.funcs.get(name).cloned() else {
            return fault(format!("unbound {name}"));
        };
        if f.params.len() != args.len() {
            return fault(format!("arity of {name}"));
        }
        if self.depth >= MAX_CALL_DEPTH {
            return fault("stack-limit");
        }
        let mut frame = HashMap::new();
        for ((p, ty), v) in f.params.iter().zip(args) {
            frame.insert(p.clone(), self.coerce(v, ty)?);
        }
        let saved = std::mem::replace(&mut self.scopes, vec![frame]);
        self.depth += 1;
        let r = self.block(&f.body, Some(&f.ret));
        self.depth -= 1;
        self.scopes = saved;
        let v = match r {
            Ok(v) => v,
            Err(Stop::Return(v)) => v,
            Err(Stop::Break) | Err(Stop::Continue) => return fault("break outside a loop"),
            Err(e) => return Err(e),
        };
        self.coerce(v, &f.ret)
    }

    // ---------------------------------------------------------- environment

    fn lookup(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn bind(&mut self, name: &str, v: Value) {
        if name != "_" {
            self.scopes.last_mut().unwrap().insert(name.to_string(), v);
        }
    }

    fn scoped<T>(&mut self, f: impl FnOnce(&mut Self) -> Eval<T>) -> Eval<T> {
        self.scopes.push(HashMap::new());
        let r = f(self);
        self.scopes.pop();
        r
    }

    // --------------------------------------------------------------- typing

    /// Fix the type of a literal from its context, recursively (TYP-13.2).
    /// A literal that does not fit is a program the checker refuses: fault.
    pub fn coerce(&self, v: Value, ty: &Ty) -> Eval<Value> {
        match (v, ty) {
            (Value::IntLit(n), Ty::Int(t)) => {
                make_int_u128(*t, n).map_or_else(|| fault("literal out of range"), Ok)
            }
            (Value::IntLit(n), Ty::Float(t)) => Ok(Value::Float(*t, round_float(*t, n as f64))),
            (Value::FloatLit(f), Ty::Float(t)) => Ok(Value::Float(*t, round_float(*t, f))),
            (Value::Tuple(vs), Ty::Tuple(ts)) if vs.len() == ts.len() => Ok(Value::Tuple(
                vs.into_iter()
                    .zip(ts)
                    .map(|(v, t)| self.coerce(v, t))
                    .collect::<Eval<_>>()?,
            )),
            (Value::Array(vs), Ty::Array(t, _)) => Ok(Value::Array(
                vs.into_iter()
                    .map(|v| self.coerce(v, t))
                    .collect::<Eval<_>>()?,
            )),
            (Value::Enum(e, var, vs), Ty::Named(n, args)) if e == *n => {
                let tys: Vec<Ty> = match (e.as_str(), var.as_str()) {
                    ("Option", "Some") | ("Result", "Ok") => {
                        args.first().cloned().into_iter().collect()
                    }
                    ("Result", "Err") => args.get(1).cloned().into_iter().collect(),
                    _ => vec![],
                };
                if tys.len() == vs.len() {
                    let vs = vs
                        .into_iter()
                        .zip(&tys)
                        .map(|(v, t)| self.coerce(v, t))
                        .collect::<Eval<_>>()?;
                    return Ok(Value::Enum(e, var, vs));
                }
                Ok(Value::Enum(e, var, vs))
            }
            (v, _) => Ok(v),
        }
    }

    /// Default type of a literal with no context: `i32`, `f64` (TYP-13.4).
    fn settle(v: Value) -> Eval<Value> {
        match v {
            Value::IntLit(n) => {
                make_int_u128(IntTy::I32, n).map_or_else(|| fault("literal out of range"), Ok)
            }
            Value::FloatLit(f) => Ok(Value::Float(FloatTy::F64, f)),
            Value::Tuple(vs) => Ok(Value::Tuple(
                vs.into_iter().map(Self::settle).collect::<Eval<_>>()?,
            )),
            Value::Array(vs) => Ok(Value::Array(
                vs.into_iter().map(Self::settle).collect::<Eval<_>>()?,
            )),
            Value::Enum(e, var, vs) => Ok(Value::Enum(
                e,
                var,
                vs.into_iter().map(Self::settle).collect::<Eval<_>>()?,
            )),
            v => Ok(v),
        }
    }

    fn ty_of(v: &Value) -> Option<Ty> {
        match v {
            Value::Int(t, _) => Some(Ty::Int(*t)),
            Value::U128(_) => Some(Ty::Int(IntTy::U128)),
            Value::Float(t, _) => Some(Ty::Float(*t)),
            _ => None,
        }
    }

    // --------------------------------------------------------------- blocks

    fn block(&mut self, b: &Block, expected: Option<&Ty>) -> Eval<Value> {
        // Every block entry is a step, so an empty loop body still spends
        // the budget (MACH-7.1).
        self.tick()?;
        self.scoped(|m| {
            for s in &b.stmts {
                m.stmt(s)?;
            }
            match &b.tail {
                Some(e) => m.expr(e, expected),
                // MACH-6.3: a block without a tail expression is `()`.
                None => Ok(Value::Unit),
            }
        })
    }

    fn stmt(&mut self, s: &Stmt) -> Eval<()> {
        self.tick()?;
        match s {
            Stmt::Let(pat, ty, e) => {
                let v = self.expr(e, ty.as_ref())?;
                let v = match ty {
                    Some(t) => self.coerce(v, t)?,
                    None => Self::settle(v)?,
                };
                if !self.bind_pattern(pat, &v)? {
                    return fault("no-match");
                }
                Ok(())
            }
            Stmt::Expr(e) => {
                self.expr(e, None)?;
                Ok(())
            }
        }
    }

    // ---------------------------------------------------------- expressions

    pub fn expr(&mut self, e: &Expr, expected: Option<&Ty>) -> Eval<Value> {
        self.tick()?;
        match e {
            Expr::Lit(l) => Ok(match l {
                Lit::Int(n, None) => match expected {
                    Some(t @ (Ty::Int(_) | Ty::Float(_))) => self.coerce(Value::IntLit(*n), t)?,
                    _ => Value::IntLit(*n),
                },
                Lit::Int(n, Some(t)) => self.coerce(Value::IntLit(*n), &Ty::Int(*t))?,
                Lit::Float(f, None) => match expected {
                    Some(t @ Ty::Float(_)) => self.coerce(Value::FloatLit(*f), t)?,
                    _ => Value::FloatLit(*f),
                },
                Lit::Float(f, Some(t)) => Value::Float(*t, round_float(*t, *f)),
                Lit::Bool(b) => Value::Bool(*b),
                Lit::Char(c) => Value::Char(*c),
                Lit::Str(s) => Value::Str(s.clone()),
            }),
            Expr::Path(p) => self.path_value(p),
            Expr::Tuple(es) => {
                let tys = match expected {
                    Some(Ty::Tuple(ts)) if ts.len() == es.len() => ts.iter().map(Some).collect(),
                    _ => vec![None; es.len()],
                };
                let mut vs = Vec::new();
                for (e, t) in es.iter().zip(tys) {
                    vs.push(self.expr(e, t)?);
                }
                Ok(if vs.is_empty() {
                    Value::Unit
                } else {
                    Value::Tuple(vs)
                })
            }
            Expr::Array(es) => {
                let elem = match expected {
                    Some(Ty::Array(t, _)) => Some((**t).clone()),
                    _ => None,
                };
                let mut vs = Vec::new();
                let mut elem_ty = elem;
                for e in es {
                    let v = self.expr(e, elem_ty.as_ref())?;
                    if elem_ty.is_none() {
                        elem_ty = Self::ty_of(&v);
                    }
                    vs.push(v);
                }
                if let Some(t) = &elem_ty {
                    vs = vs
                        .into_iter()
                        .map(|v| self.coerce(v, t))
                        .collect::<Eval<_>>()?;
                }
                Ok(Value::Array(vs))
            }
            Expr::ArrayRepeat(v, n) => {
                let elem = match expected {
                    Some(Ty::Array(t, _)) => Some((**t).clone()),
                    _ => None,
                };
                let v = self.expr(v, elem.as_ref())?;
                let n = self.expr(n, Some(&Ty::Int(IntTy::Usize)))?;
                let n = as_i128(&n).ok_or(Stop::Fault("array length".into()))?;
                Ok(Value::Array(vec![v; n as usize]))
            }
            Expr::StructLit(path, fields) => {
                let name = path.last().unwrap().clone();
                let Some(def) = self.structs.get(&name).cloned() else {
                    return fault(format!("unbound {name}"));
                };
                let mut out = Vec::new();
                for (fname, fty) in &def {
                    let Some((_, fe)) = fields.iter().find(|(n, _)| n == fname) else {
                        return fault(format!("missing field {fname}"));
                    };
                    let v = self.expr(fe, Some(fty))?;
                    out.push((fname.clone(), self.coerce(v, fty)?));
                }
                Ok(Value::Struct(name, out))
            }
            Expr::Field(base, f) => match self.expr(base, None)? {
                Value::Struct(_, fs) => fs
                    .into_iter()
                    .find(|(n, _)| n == f)
                    .map(|(_, v)| v)
                    .ok_or(Stop::Fault(format!("no field {f}"))),
                _ => fault(format!("field {f} of a non-struct")),
            },
            Expr::TupleField(base, i) => match self.expr(base, None)? {
                Value::Tuple(vs) => vs
                    .into_iter()
                    .nth(*i)
                    .ok_or(Stop::Fault("tuple index".into())),
                _ => fault("tuple field of a non-tuple"),
            },
            Expr::Index(base, idx) => {
                let b = self.expr(base, None)?;
                let i = self.expr(idx, Some(&Ty::Int(IntTy::Usize)))?;
                let i = self.coerce(i, &Ty::Int(IntTy::Usize))?;
                match b {
                    Value::Array(vs) => {
                        let i = as_i128(&i).ok_or(Stop::Fault("index".into()))?;
                        vs.get(i as usize)
                            .cloned()
                            .map_or_else(|| panic("index out of bounds"), Ok)
                    }
                    _ => fault("index of a non-array"),
                }
            }
            Expr::Unary(op, inner) => self.unary(*op, inner, expected),
            Expr::Binary(op, a, b) => self.binary(*op, a, b, expected),
            Expr::Cast(inner, ty) => {
                let v = self.expr(inner, None)?;
                let v = Self::settle(v)?;
                cast(v, ty)
            }
            Expr::Block(b) => self.block(b, expected),
            Expr::If(c, then, other) => {
                let c = self.expr(c, Some(&Ty::Bool))?;
                // MACH-4.4: only the selected branch is reduced.
                match (c, other) {
                    (Value::Bool(true), _) => self.block(then, expected),
                    (Value::Bool(false), Some(o)) => self.expr(o, expected),
                    (Value::Bool(false), None) => Ok(Value::Unit),
                    _ => fault("non-bool condition"),
                }
            }
            Expr::Match(scrut, arms) => {
                let v = self.expr(scrut, None)?;
                let v = Self::settle(v)?;
                // MACH-4.5: arms in order, first match wins.
                for arm in arms {
                    self.scopes.push(HashMap::new());
                    let matched = match self.bind_pattern(&arm.pat, &v) {
                        Ok(m) => m,
                        Err(e) => {
                            self.scopes.pop();
                            return Err(e);
                        }
                    };
                    if matched {
                        let r = self.expr(&arm.body, expected);
                        self.scopes.pop();
                        return r;
                    }
                    self.scopes.pop();
                }
                fault("no-match")
            }
            Expr::While(c, body) => {
                loop {
                    match self.expr(c, Some(&Ty::Bool))? {
                        Value::Bool(true) => {}
                        Value::Bool(false) => break,
                        _ => return fault("non-bool condition"),
                    }
                    match self.block(body, None) {
                        Ok(_) | Err(Stop::Continue) => {}
                        Err(Stop::Break) => break,
                        Err(e) => return Err(e),
                    }
                }
                Ok(Value::Unit)
            }
            Expr::Loop(body) => loop {
                match self.block(body, None) {
                    Ok(_) | Err(Stop::Continue) => {}
                    Err(Stop::Break) => return Ok(Value::Unit),
                    Err(e) => return Err(e),
                }
            },
            Expr::For(var, lo, hi, body) => {
                // MACH-6.5: bounds evaluated once, `a >= b` is zero iterations.
                let lo = self.expr(lo, None)?;
                let hi = self.expr(hi, Self::ty_of(&lo).as_ref())?;
                let (lo, hi) = unify(lo, hi)?;
                let (lo, hi) = (Self::settle(lo)?, Self::settle(hi)?);
                let ty = int_ty(&lo).ok_or(Stop::Fault("non-integer range".into()))?;
                let mut i = lo;
                loop {
                    if compare(&i, &hi)? != std::cmp::Ordering::Less {
                        break;
                    }
                    let r = self.scoped(|m| {
                        m.bind(var, i.clone());
                        m.block(body, None)
                    });
                    match r {
                        Ok(_) | Err(Stop::Continue) => {}
                        Err(Stop::Break) => break,
                        Err(e) => return Err(e),
                    }
                    i = int_arith("+", &i, &make_int(ty, 1).unwrap())?;
                }
                Ok(Value::Unit)
            }
            Expr::Break => Err(Stop::Break),
            Expr::Continue => Err(Stop::Continue),
            Expr::Return(v) => {
                let v = match v {
                    Some(e) => self.expr(e, None)?,
                    None => Value::Unit,
                };
                Err(Stop::Return(v))
            }
            Expr::Assign(place, value) => {
                let current = self.expr(place, None)?;
                let v = self.expr(value, Self::ty_of(&current).as_ref())?;
                let v = match Self::ty_of(&current) {
                    Some(t) => self.coerce(v, &t)?,
                    None => Self::settle(v)?,
                };
                self.assign(place, v)?;
                Ok(Value::Unit)
            }
            Expr::AssignOp(op, place, value) => {
                let v = self.binary(*op, place, value, None)?;
                self.assign(place, v)?;
                Ok(Value::Unit)
            }
            Expr::Call(callee, args) => self.call_expr(callee, args, expected),
            Expr::Method(recv, name, args) => self.method(recv, name, args),
        }
    }

    fn path_value(&mut self, p: &[String]) -> Eval<Value> {
        if p.len() == 1 {
            if let Some(v) = self.lookup(&p[0]) {
                return Ok(v.clone());
            }
            if let Some((t, e)) = self.consts.get(&p[0]).cloned() {
                let v = self.expr(&e, Some(&t))?;
                return self.coerce(v, &t);
            }
            if p[0] == "None" {
                return Ok(Value::Enum("Option".into(), "None".into(), vec![]));
            }
            return fault(format!("unbound {}", p[0]));
        }
        let (e, var) = (&p[p.len() - 2], &p[p.len() - 1]);
        if let Some(vars) = self.enums.get(e) {
            if vars.iter().any(|(v, tys)| v == var && tys.is_empty()) {
                return Ok(Value::Enum(e.clone(), var.clone(), vec![]));
            }
        }
        fault(format!("unbound {}", p.join("::")))
    }

    fn assign(&mut self, place: &Expr, v: Value) -> Eval<()> {
        // MACH-2.3: a "mutation" replaces the binding with an updated value.
        let (root, steps) = place_path(place)?;
        let mut indices = Vec::new();
        for s in &steps {
            if let PlaceStep::Index(e) = s {
                let i = self.expr(e, Some(&Ty::Int(IntTy::Usize)))?;
                let i = self.coerce(i, &Ty::Int(IntTy::Usize))?;
                indices.push(as_i128(&i).unwrap_or(-1));
            }
        }
        let Some(slot) = self.scopes.iter_mut().rev().find_map(|s| s.get_mut(&root)) else {
            return fault(format!("unbound {root}"));
        };
        let mut cur = slot;
        let mut idx = indices.into_iter();
        for s in &steps {
            cur = match (s, cur) {
                (PlaceStep::Field(f), Value::Struct(_, fs)) => {
                    &mut fs
                        .iter_mut()
                        .find(|(n, _)| n == f)
                        .ok_or(Stop::Fault(format!("no field {f}")))?
                        .1
                }
                (PlaceStep::Tuple(i), Value::Tuple(vs)) => {
                    vs.get_mut(*i).ok_or(Stop::Fault("tuple index".into()))?
                }
                (PlaceStep::Index(_), Value::Array(vs)) => {
                    let i = idx.next().unwrap();
                    if i < 0 || i as usize >= vs.len() {
                        return panic("index out of bounds");
                    }
                    &mut vs[i as usize]
                }
                _ => return fault("invalid place"),
            };
        }
        *cur = v;
        Ok(())
    }

    // ------------------------------------------------------------ operators

    fn unary(&mut self, op: UnOp, inner: &Expr, expected: Option<&Ty>) -> Eval<Value> {
        if op == UnOp::Neg {
            if let Expr::Lit(Lit::Int(n, suffix)) = inner {
                // `-128i8`: the negative literal is a single value.
                let ty = suffix.or(match expected {
                    Some(Ty::Int(t)) => Some(*t),
                    _ => None,
                });
                if let Some(Ty::Float(t)) = expected {
                    return Ok(Value::Float(*t, round_float(*t, -(*n as f64))));
                }
                let ty = ty.unwrap_or(IntTy::I32);
                let v = i128::try_from(*n)
                    .ok()
                    .and_then(|n| make_int(ty, -n))
                    .or_else(|| {
                        (*n == 1u128 << 127 && ty == IntTy::I128)
                            .then_some(Value::Int(ty, i128::MIN))
                    });
                return v.map_or_else(|| fault("literal out of range"), Ok);
            }
        }
        let v = self.expr(inner, expected)?;
        let v = match (&v, expected) {
            (Value::IntLit(_) | Value::FloatLit(_), Some(t)) => self.coerce(v, t)?,
            _ => Self::settle(v)?,
        };
        match (op, v) {
            (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            (UnOp::Not, v @ (Value::Int(..) | Value::U128(_))) => {
                Ok(wrap(int_ty(&v).unwrap(), !raw_bits(&v)))
            }
            (UnOp::Neg, Value::Float(t, f)) => Ok(Value::Float(t, -f)),
            (UnOp::Neg, Value::Int(t, n)) if t.signed() => n
                .checked_neg()
                .and_then(|n| make_int(t, n))
                .map_or_else(|| panic("attempt to negate with overflow"), Ok),
            _ => fault("invalid unary operand"),
        }
    }

    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr, expected: Option<&Ty>) -> Eval<Value> {
        use BinOp::*;
        match op {
            And | Or => {
                // MACH-4.2: the right operand is not reduced when the left
                // one decides the result.
                let l = self.expr(a, Some(&Ty::Bool))?;
                match (op, l) {
                    (And, Value::Bool(false)) => Ok(Value::Bool(false)),
                    (Or, Value::Bool(true)) => Ok(Value::Bool(true)),
                    (_, Value::Bool(_)) => match self.expr(b, Some(&Ty::Bool))? {
                        Value::Bool(r) => Ok(Value::Bool(r)),
                        _ => fault("non-bool operand"),
                    },
                    _ => fault("non-bool operand"),
                }
            }
            Shl | Shr => {
                let l = self.expr(a, expected)?;
                let l = match expected {
                    Some(t) => self.coerce(l, t)?,
                    None => Self::settle(l)?,
                };
                let r = Self::settle(self.expr(b, None)?)?;
                shift(op, &l, &r)
            }
            Eq | Ne | Lt | Le | Gt | Ge => {
                // MACH-4.1: left operand fully reduced first.
                let l = self.expr(a, None)?;
                let r = self.expr(b, Self::ty_of(&l).as_ref())?;
                let (l, r) = unify(l, r)?;
                compare_op(op, &l, &r)
            }
            _ => {
                let l = self.expr(a, expected)?;
                let hint = Self::ty_of(&l).or_else(|| expected.cloned());
                let r = self.expr(b, hint.as_ref())?;
                let (l, r) = unify(l, r)?;
                let (l, r) = match expected {
                    Some(t) if matches!(l, Value::IntLit(_) | Value::FloatLit(_)) => {
                        (self.coerce(l, t)?, self.coerce(r, t)?)
                    }
                    _ => (Self::settle(l)?, Self::settle(r)?),
                };
                arith(op, &l, &r)
            }
        }
    }

    // ---------------------------------------------------------------- calls

    fn call_expr(&mut self, callee: &Expr, args: &[Expr], expected: Option<&Ty>) -> Eval<Value> {
        let Expr::Path(p) = callee else {
            return fault("call of a non-path");
        };
        let name = p.join("::");
        // MACH-4.3: arguments left to right, before the call.
        if let Some(f) = self.funcs.get(p.last().unwrap()).cloned() {
            if p.len() == 1 {
                let mut vs = Vec::new();
                for (e, (_, t)) in args.iter().zip(&f.params) {
                    let v = self.expr(e, Some(t))?;
                    vs.push(self.coerce(v, t)?);
                }
                if vs.len() != args.len() {
                    return fault(format!("arity of {name}"));
                }
                return self.call(&f.name, vs);
            }
        }
        match name.as_str() {
            "Some" | "Ok" | "Err" => {
                let (enum_name, slot) = match name.as_str() {
                    "Some" => ("Option", 0),
                    "Ok" => ("Result", 0),
                    _ => ("Result", 1),
                };
                let inner = match expected {
                    Some(Ty::Named(n, args)) if n == enum_name => args.get(slot).cloned(),
                    _ => None,
                };
                let v = self.expr(&args[0], inner.as_ref())?;
                let v = match &inner {
                    Some(t) => self.coerce(v, t)?,
                    None => Self::settle(v)?,
                };
                return Ok(Value::Enum(enum_name.into(), name, vec![v]));
            }
            "panic" => {
                let msg = match self.expr(&args[0], None)? {
                    Value::Str(s) => s,
                    v => display(&v),
                };
                return panic(msg);
            }
            "assert" => {
                return match self.expr(&args[0], Some(&Ty::Bool))? {
                    Value::Bool(true) => Ok(Value::Unit),
                    Value::Bool(false) => panic("assertion failed"),
                    _ => fault("non-bool assertion"),
                };
            }
            _ => {}
        }
        if p.len() >= 2 && p[p.len() - 2] == "math" {
            let f = p.last().unwrap().as_str();
            if LIBM.contains(&f) {
                return fault("unsupported-libm");
            }
            let mut vs = Vec::new();
            for e in args {
                let v = self.expr(e, Some(&Ty::Float(FloatTy::F64)))?;
                vs.push(self.coerce(v, &Ty::Float(FloatTy::F64))?);
            }
            return float_fn(f, &vs);
        }
        if p.len() >= 2 {
            let (e, var) = (&p[p.len() - 2], &p[p.len() - 1]);
            if let Some(vars) = self.enums.get(e).cloned() {
                if let Some((_, tys)) = vars.iter().find(|(v, _)| v == var) {
                    let mut vs = Vec::new();
                    for (a, t) in args.iter().zip(tys) {
                        let v = self.expr(a, Some(t))?;
                        vs.push(self.coerce(v, t)?);
                    }
                    return Ok(Value::Enum(e.clone(), var.clone(), vs));
                }
            }
        }
        fault(format!("unbound {name}"))
    }

    fn method(&mut self, recv: &Expr, name: &str, args: &[Expr]) -> Eval<Value> {
        let r = Self::settle(self.expr(recv, None)?)?;
        let hint = Self::ty_of(&r);
        let mut vs = Vec::new();
        for a in args {
            let v = self.expr(a, hint.as_ref())?;
            vs.push(match &hint {
                Some(t) => self.coerce(v, t)?,
                None => Self::settle(v)?,
            });
        }
        match &r {
            Value::Float(t, _) => {
                if LIBM.contains(&name) {
                    return fault("unsupported-libm");
                }
                let mut all = vec![Value::Float(FloatTy::F64, float_of(&r))];
                all.extend(vs.iter().map(|v| Value::Float(FloatTy::F64, float_of(v))));
                match float_fn(name, &all)? {
                    Value::Float(_, f) => Ok(Value::Float(*t, round_float(*t, f))),
                    v => Ok(v),
                }
            }
            Value::Int(..) | Value::U128(_) => int_method(name, &r, &vs),
            Value::Enum(e, var, inner) if e == "Option" || e == "Result" => {
                let ok = var == "Some" || var == "Ok";
                match name {
                    "is_some" | "is_ok" => Ok(Value::Bool(ok)),
                    "is_none" | "is_err" => Ok(Value::Bool(!ok)),
                    "unwrap" if ok => Ok(inner[0].clone()),
                    "unwrap" => panic("called `unwrap` on a `None` or `Err` value"),
                    "unwrap_or" if ok => Ok(inner[0].clone()),
                    "unwrap_or" => Ok(vs[0].clone()),
                    _ => fault(format!("unsupported method {name}")),
                }
            }
            _ => fault(format!("unsupported method {name}")),
        }
    }

    // ------------------------------------------------------------- patterns

    fn bind_pattern(&mut self, pat: &Pat, v: &Value) -> Eval<bool> {
        Ok(match pat {
            Pat::Wild => true,
            Pat::Bind(n) => {
                self.bind(n, v.clone());
                true
            }
            Pat::Lit(l, neg) => {
                let lit = lit_value(l, *neg, v)?;
                compare(&lit, v)? == std::cmp::Ordering::Equal
            }
            Pat::Tuple(ps) => match v {
                Value::Tuple(vs) if vs.len() == ps.len() => {
                    for (p, v) in ps.iter().zip(vs) {
                        if !self.bind_pattern(p, v)? {
                            return Ok(false);
                        }
                    }
                    true
                }
                _ => false,
            },
            Pat::Variant(path, ps) => match v {
                Value::Enum(_, var, vs) => {
                    if path.last() != Some(var) || ps.len() != vs.len() {
                        return Ok(false);
                    }
                    for (p, v) in ps.iter().zip(vs) {
                        if !self.bind_pattern(p, v)? {
                            return Ok(false);
                        }
                    }
                    true
                }
                _ => false,
            },
        })
    }
}

enum PlaceStep {
    Field(String),
    Tuple(usize),
    Index(Expr),
}

fn place_path(e: &Expr) -> Eval<(String, Vec<PlaceStep>)> {
    match e {
        Expr::Path(p) if p.len() == 1 => Ok((p[0].clone(), vec![])),
        Expr::Field(b, f) => {
            let (r, mut s) = place_path(b)?;
            s.push(PlaceStep::Field(f.clone()));
            Ok((r, s))
        }
        Expr::TupleField(b, i) => {
            let (r, mut s) = place_path(b)?;
            s.push(PlaceStep::Tuple(*i));
            Ok((r, s))
        }
        Expr::Index(b, i) => {
            let (r, mut s) = place_path(b)?;
            s.push(PlaceStep::Index((**i).clone()));
            Ok((r, s))
        }
        _ => fault("invalid place"),
    }
}

fn lit_value(l: &Lit, neg: bool, like: &Value) -> Eval<Value> {
    Ok(match l {
        Lit::Int(n, _) => match int_ty(like) {
            Some(t) => {
                let v = i128::try_from(*n).map_err(|_| Stop::Fault("pattern literal".into()))?;
                make_int(t, if neg { -v } else { v })
                    .ok_or(Stop::Fault("pattern literal".into()))?
            }
            None => return fault("integer pattern on a non-integer"),
        },
        Lit::Float(f, _) => Value::FloatLit(if neg { -f } else { *f }),
        Lit::Bool(b) => Value::Bool(*b),
        Lit::Char(c) => Value::Char(*c),
        Lit::Str(s) => Value::Str(s.clone()),
    })
}

/// Give two operands one type: a literal takes the type of the other side.
fn unify(l: Value, r: Value) -> Eval<(Value, Value)> {
    let fix = |lit: Value, other: &Value| -> Eval<Value> {
        match (lit, other) {
            (Value::IntLit(n), o) if int_ty(o).is_some() => make_int_u128(int_ty(o).unwrap(), n)
                .map_or_else(|| fault("literal out of range"), Ok),
            (Value::IntLit(n), Value::Float(t, _)) => Ok(Value::Float(*t, n as f64)),
            (Value::FloatLit(f), Value::Float(t, _)) => Ok(Value::Float(*t, round_float(*t, f))),
            (v, _) => Ok(v),
        }
    };
    let l2 = fix(l, &r)?;
    let r2 = fix(r, &l2)?;
    Ok((l2, r2))
}

fn float_of(v: &Value) -> f64 {
    match v {
        Value::Float(_, f) | Value::FloatLit(f) => *f,
        Value::IntLit(n) => *n as f64,
        _ => f64::NAN,
    }
}

fn arith(op: BinOp, l: &Value, r: &Value) -> Eval<Value> {
    use BinOp::*;
    match (l, r) {
        (Value::Float(t, a), Value::Float(_, b)) => {
            let v = match op {
                Add => a + b,
                Sub => a - b,
                Mul => a * b,
                Div => a / b,
                Rem => a % b,
                _ => return fault("invalid float operator"),
            };
            Ok(Value::Float(*t, round_float(*t, v)))
        }
        (Value::Bool(a), Value::Bool(b)) => match op {
            BitAnd => Ok(Value::Bool(a & b)),
            BitOr => Ok(Value::Bool(a | b)),
            BitXor => Ok(Value::Bool(a ^ b)),
            _ => fault("invalid bool operator"),
        },
        _ if int_ty(l).is_some() && int_ty(l) == int_ty(r) => {
            let ty = int_ty(l).unwrap();
            let sym = match op {
                Add => "+",
                Sub => "-",
                Mul => "*",
                Div => "/",
                Rem => "%",
                BitAnd => return Ok(wrap(ty, raw_bits(l) & raw_bits(r))),
                BitOr => return Ok(wrap(ty, raw_bits(l) | raw_bits(r))),
                BitXor => return Ok(wrap(ty, raw_bits(l) ^ raw_bits(r))),
                _ => return fault("invalid integer operator"),
            };
            int_arith(sym, l, r)
        }
        _ => fault("operands of different types"),
    }
}

fn shift(op: BinOp, l: &Value, r: &Value) -> Eval<Value> {
    let Some(ty) = int_ty(l) else {
        return fault("shift of a non-integer");
    };
    let n = match r {
        Value::Int(_, n) => *n,
        Value::U128(n) => i128::try_from(*n).unwrap_or(i128::MAX),
        _ => return fault("non-integer shift amount"),
    };
    // §5: `0 <= n < width`, otherwise a trap.
    if n < 0 || n >= ty.bits() as i128 {
        return panic("shift out of range");
    }
    let bits = raw_bits(l);
    Ok(match op {
        BinOp::Shl => wrap(ty, bits << n),
        _ if ty.signed() => {
            let v = as_i128(l).unwrap() >> n;
            Value::Int(ty, v)
        }
        _ => wrap(ty, bits >> n),
    })
}

fn compare(l: &Value, r: &Value) -> Eval<std::cmp::Ordering> {
    use std::cmp::Ordering;
    Ok(match (l, r) {
        (Value::U128(a), Value::U128(b)) => a.cmp(b),
        (Value::Int(_, a), Value::Int(_, b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::Char(a), Value::Char(b)) => a.cmp(b),
        (Value::Str(a), Value::Str(b)) => a.cmp(b),
        (Value::Float(_, a) | Value::FloatLit(a), Value::Float(_, b) | Value::FloatLit(b)) => {
            a.partial_cmp(b).unwrap_or(Ordering::Less)
        }
        _ => return fault("comparison of different types"),
    })
}

fn compare_op(op: BinOp, l: &Value, r: &Value) -> Eval<Value> {
    use BinOp::*;
    if let (Value::Float(_, a), Value::Float(_, b)) = (l, r) {
        // §5: IEEE comparisons; NaN compares false except `!=`.
        return Ok(Value::Bool(match op {
            Eq => a == b,
            Ne => a != b,
            Lt => a < b,
            Le => a <= b,
            Gt => a > b,
            _ => a >= b,
        }));
    }
    if matches!(op, Eq | Ne) {
        let eq = structural_eq(l, r)?;
        return Ok(Value::Bool(if op == Eq { eq } else { !eq }));
    }
    let o = compare(l, r)?;
    Ok(Value::Bool(match op {
        Lt => o.is_lt(),
        Le => o.is_le(),
        Gt => o.is_gt(),
        _ => o.is_ge(),
    }))
}

fn structural_eq(l: &Value, r: &Value) -> Eval<bool> {
    Ok(match (l, r) {
        (Value::Tuple(a), Value::Tuple(b)) | (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Ok(false);
            }
            for (x, y) in a.iter().zip(b) {
                let (x, y) = unify(x.clone(), y.clone())?;
                if !structural_eq(&x, &y)? {
                    return Ok(false);
                }
            }
            true
        }
        (Value::Enum(e1, v1, a), Value::Enum(e2, v2, b)) => {
            e1 == e2
                && v1 == v2
                && structural_eq(&Value::Tuple(a.clone()), &Value::Tuple(b.clone()))?
        }
        (Value::Struct(n1, a), Value::Struct(n2, b)) => {
            n1 == n2
                && structural_eq(
                    &Value::Tuple(a.iter().map(|(_, v)| v.clone()).collect()),
                    &Value::Tuple(b.iter().map(|(_, v)| v.clone()).collect()),
                )?
        }
        (Value::Float(_, a), Value::Float(_, b)) => a == b,
        (Value::Unit, Value::Unit) => true,
        _ => compare(l, r)? == std::cmp::Ordering::Equal,
    })
}

fn float_fn(name: &str, vs: &[Value]) -> Eval<Value> {
    let x = vs.first().map(float_of).unwrap_or(f64::NAN);
    let y = vs.get(1).map(float_of);
    let r = match (name, y) {
        ("sqrt", None) => x.sqrt(),
        ("floor", None) => x.floor(),
        ("ceil", None) => x.ceil(),
        // `round`: ties away from zero (STD-6.3).
        ("round", None) => x.round(),
        ("abs", None) => x.abs(),
        // STD-6.5: NaN yields the other operand; -0.0 < +0.0.
        ("min", Some(y)) => {
            if x.is_nan() {
                y
            } else if y.is_nan() || x < y || (x == y && x.is_sign_negative()) {
                x
            } else {
                y
            }
        }
        ("max", Some(y)) => {
            if x.is_nan() {
                y
            } else if y.is_nan() || x > y || (x == y && y.is_sign_negative()) {
                x
            } else {
                y
            }
        }
        _ => return fault(format!("unsupported math function {name}")),
    };
    Ok(Value::Float(FloatTy::F64, r))
}

fn int_method(name: &str, r: &Value, vs: &[Value]) -> Eval<Value> {
    let ty = int_ty(r).unwrap();
    let arg = vs.first();
    let some = |v: Value| Value::Enum("Option".into(), "Some".into(), vec![v]);
    let none = || Value::Enum("Option".into(), "None".into(), vec![]);
    let op = |m: &str| match m {
        "add" => "+",
        "sub" => "-",
        "mul" => "*",
        "div" => "/",
        _ => "%",
    };
    if name == "abs" && ty.signed() {
        let n = as_i128(r).unwrap();
        return n
            .checked_abs()
            .and_then(|n| make_int(ty, n))
            .map_or_else(|| panic("attempt to negate with overflow"), Ok);
    }
    if name == "min" || name == "max" {
        let b = arg.ok_or(Stop::Fault("missing argument".into()))?;
        let less = compare(r, b)?.is_lt();
        return Ok(if (name == "min") == less {
            r.clone()
        } else {
            b.clone()
        });
    }
    if let Some((kind, m)) = name.split_once('_') {
        let exact = if m == "neg" {
            let zero = make_int(ty, 0).unwrap();
            int_arith("-", &zero, r)
        } else {
            let b = arg.ok_or(Stop::Fault("missing argument".into()))?;
            int_arith(op(m), r, b)
        };
        return match (kind, exact) {
            (_, Ok(v)) => Ok(match kind {
                "checked" => some(v),
                _ => v,
            }),
            ("checked", Err(Stop::Panic(_))) => Ok(none()),
            ("wrapping", Err(Stop::Panic(_))) => {
                let b = arg.map(raw_bits).unwrap_or(0);
                let a = raw_bits(r);
                Ok(wrap(
                    ty,
                    match m {
                        "add" => a.wrapping_add(b),
                        "sub" => a.wrapping_sub(b),
                        "mul" => a.wrapping_mul(b),
                        _ => 0u128.wrapping_sub(a),
                    },
                ))
            }
            ("saturating", Err(Stop::Panic(_))) => {
                let (lo, hi) = if ty == IntTy::U128 {
                    (Value::U128(0), Value::U128(u128::MAX))
                } else {
                    let (lo, hi) = bounds(ty);
                    (make_int(ty, lo).unwrap(), make_int(ty, hi).unwrap())
                };
                let neg_a = as_i128(r).is_some_and(|v| v < 0);
                let neg_b = arg.and_then(as_i128).is_some_and(|v| v < 0);
                let low = match m {
                    "add" => neg_a && neg_b,
                    // Unsigned `a - b` can only overflow below zero.
                    "sub" => !ty.signed() || (neg_a && !neg_b),
                    "mul" => neg_a != neg_b,
                    _ => false,
                };
                Ok(if low { lo } else { hi })
            }
            (_, Err(e)) => Err(e),
        };
    }
    fault(format!("unsupported method {name}"))
}

pub fn cast(v: Value, ty: &Ty) -> Eval<Value> {
    match (v, ty) {
        (v @ (Value::Int(..) | Value::U128(_)), Ty::Int(t)) => Ok(wrap(*t, raw_bits(&v))),
        (Value::Int(_, n), Ty::Float(t)) => Ok(Value::Float(
            *t,
            match t {
                FloatTy::F32 => n as f32 as f64,
                FloatTy::F64 => n as f64,
            },
        )),
        (Value::U128(n), Ty::Float(t)) => Ok(Value::Float(
            *t,
            match t {
                FloatTy::F32 => n as f32 as f64,
                FloatTy::F64 => n as f64,
            },
        )),
        // Saturating, NaN -> 0 (TYP-13.2 table).
        (Value::Float(_, f), Ty::Int(t)) => {
            if f.is_nan() {
                return Ok(make_int(*t, 0).unwrap());
            }
            let tr = f.trunc();
            if *t == IntTy::U128 {
                return Ok(Value::U128(if tr <= 0.0 { 0 } else { tr as u128 }));
            }
            let (lo, hi) = bounds(*t);
            let v = if tr <= lo as f64 {
                lo
            } else if tr >= hi as f64 {
                hi
            } else {
                tr as i128
            };
            Ok(make_int(*t, v).unwrap())
        }
        (Value::Float(_, f), Ty::Float(t)) => Ok(Value::Float(*t, round_float(*t, f))),
        (Value::Bool(b), Ty::Int(t)) => Ok(make_int(*t, b as i128).unwrap()),
        (Value::Char(c), Ty::Int(t)) => Ok(wrap(*t, c as u128)),
        (v, _) => fault(format!("invalid cast of {} to {ty:?}", type_name(&v))),
    }
}
