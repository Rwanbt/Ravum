//! Ravum abstract syntax tree.
//!
//! **Interface gelée** pour le travail parallèle : chaque nœud sémantique porte
//! un [`NodeId`] unique attribué par le parser dans l'ordre de lecture. Le
//! checker remplit des tables indexées par ces identifiants (`sema::facts`) et le
//! codegen ne consomme que ces tables + l'AST ; aucun identifiant non résolu ne
//! subsiste après vérification.
//!
//! Le CST (commentaires, trivia) n'est pas représenté ici : le formateur travaille
//! sur les tokens, l'AST ne conserve que la structure sémantique.

use crate::span::{FileId, Span};

pub type NodeId = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

impl Ident {
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self {
            name: name.into(),
            span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntTy {
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    Isize,
    Usize,
}

impl IntTy {
    pub fn from_name(name: &str) -> Option<IntTy> {
        Some(match name {
            "i8" => IntTy::I8,
            "i16" => IntTy::I16,
            "i32" => IntTy::I32,
            "i64" => IntTy::I64,
            "i128" => IntTy::I128,
            "u8" => IntTy::U8,
            "u16" => IntTy::U16,
            "u32" => IntTy::U32,
            "u64" => IntTy::U64,
            "u128" => IntTy::U128,
            "isize" => IntTy::Isize,
            "usize" => IntTy::Usize,
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            IntTy::I8 => "i8",
            IntTy::I16 => "i16",
            IntTy::I32 => "i32",
            IntTy::I64 => "i64",
            IntTy::I128 => "i128",
            IntTy::U8 => "u8",
            IntTy::U16 => "u16",
            IntTy::U32 => "u32",
            IntTy::U64 => "u64",
            IntTy::U128 => "u128",
            IntTy::Isize => "isize",
            IntTy::Usize => "usize",
        }
    }

    pub fn is_signed(&self) -> bool {
        matches!(
            self,
            IntTy::I8 | IntTy::I16 | IntTy::I32 | IntTy::I64 | IntTy::I128 | IntTy::Isize
        )
    }

    pub fn bits(&self) -> u32 {
        match self {
            IntTy::I8 | IntTy::U8 => 8,
            IntTy::I16 | IntTy::U16 => 16,
            IntTy::I32 | IntTy::U32 => 32,
            IntTy::I64 | IntTy::U64 => 64,
            IntTy::I128 | IntTy::U128 => 128,
            IntTy::Isize | IntTy::Usize => 64,
        }
    }

    /// Inclusive range of values representable by the type (as i128 where it fits).
    pub fn min_i128(&self) -> i128 {
        match self {
            IntTy::I8 => i8::MIN as i128,
            IntTy::I16 => i16::MIN as i128,
            IntTy::I32 => i32::MIN as i128,
            IntTy::I64 => i64::MIN as i128,
            IntTy::I128 => i128::MIN,
            IntTy::Isize => i64::MIN as i128,
            _ => 0,
        }
    }

    pub fn max_i128(&self) -> i128 {
        match self {
            IntTy::I8 => i8::MAX as i128,
            IntTy::I16 => i16::MAX as i128,
            IntTy::I32 => i32::MAX as i128,
            IntTy::I64 => i64::MAX as i128,
            IntTy::I128 => i128::MAX,
            IntTy::Isize => i64::MAX as i128,
            IntTy::U8 => u8::MAX as i128,
            IntTy::U16 => u16::MAX as i128,
            IntTy::U32 => u32::MAX as i128,
            IntTy::U64 => u64::MAX as i128,
            IntTy::U128 => u128::MAX as i128,
            IntTy::Usize => u64::MAX as i128,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatTy {
    F32,
    F64,
}

impl FloatTy {
    pub fn from_name(name: &str) -> Option<FloatTy> {
        match name {
            "f32" => Some(FloatTy::F32),
            "f64" => Some(FloatTy::F64),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            FloatTy::F32 => "f32",
            FloatTy::F64 => "f64",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vis {
    Private,
    Public(Span),
}

impl Vis {
    pub fn is_public(&self) -> bool {
        matches!(self, Vis::Public(_))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub file: FileId,
    pub root: Module,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub id: NodeId,
    pub name: Option<Ident>,
    pub items: Vec<Item>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: NodeId,
    pub vis: Vis,
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Fn(FnItem),
    Struct(StructItem),
    Enum(EnumItem),
    Impl(ImplItem),
    Const(ConstItem),
    Use(UseItem),
    /// Module inline : `mod foo { ... }`.
    Mod(Module),
    /// Déclaration de module fichier : `mod foo;` (résolue par le loader).
    ModDecl(ModDecl),
    /// Bloc FFI : `extern "C" { fn f(x: i32) -> i32; }`.
    ///
    /// Core 1.0 n'accepte que l'ABI `"C"` et des signatures composées de
    /// primitives FFI-sûres (entiers, flottants, `bool`, `()`). Les fonctions
    /// déclarées ne sont appelables que dans un bloc `unsafe` et produisent les
    /// effets `unsafe` + `ffi`.
    Extern(ExternBlock),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternBlock {
    /// Nom de l'ABI (`C` uniquement en Core 1.0).
    pub abi: Ident,
    pub fns: Vec<ExternFn>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternFn {
    pub id: NodeId,
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModDecl {
    pub name: Ident,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnItem {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret: Type,
    /// `None` = effets inférés et exportés ; `Some([])` = pure déclarée.
    pub effects: Option<Vec<Ident>>,
    pub body: Block,
    pub is_test: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericParam {
    pub id: NodeId,
    pub name: Ident,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub id: NodeId,
    pub kind: ParamKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamKind {
    Receiver(Receiver),
    Binding {
        mutable: bool,
        name: Ident,
        ty: Type,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receiver {
    Shared,
    Exclusive,
    ByValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructItem {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub fields: Vec<FieldDef>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub id: NodeId,
    pub vis: Vis,
    pub name: Ident,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumItem {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub variants: Vec<VariantDef>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantDef {
    pub id: NodeId,
    pub name: Ident,
    pub fields: VariantDefFields,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantDefFields {
    Unit,
    Tuple(Vec<Type>),
    Struct(Vec<FieldDef>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplItem {
    pub generics: Vec<GenericParam>,
    pub self_ty: Type,
    pub items: Vec<Item>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstItem {
    pub name: Ident,
    pub ty: Type,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UseItem {
    pub path: Path,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub id: NodeId,
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Path(Path),
    Ref {
        mutable: bool,
        inner: Box<Type>,
    },
    Tuple(Vec<Type>),
    Array {
        elem: Box<Type>,
        len: Box<Expr>,
    },
    Slice(Box<Type>),
    Unit,
    /// `!` — the Never type (G-5.11, TYP-1.6). Nameable as a whole return type;
    /// `sema` rejects it in parameters, fields and generic arguments.
    Never,
    Infer,
    SelfType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRoot {
    Plain,
    Crate,
    Super,
    SelfMod,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub id: NodeId,
    pub root: PathRoot,
    pub segments: Vec<PathSegment>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathSegment {
    pub name: Ident,
    pub args: Vec<Type>,
}

impl Path {
    /// Dotted textual form used in diagnostics and builtin lookup.
    pub fn display(&self) -> String {
        let mut out = String::new();
        match self.root {
            PathRoot::Crate => out.push_str("crate::"),
            PathRoot::Super => out.push_str("super::"),
            PathRoot::SelfMod => out.push_str("self::"),
            PathRoot::Plain => {}
        }
        out.push_str(
            &self
                .segments
                .iter()
                .map(|s| s.name.name.clone())
                .collect::<Vec<_>>()
                .join("::"),
        );
        out
    }

    pub fn last(&self) -> Option<&PathSegment> {
        self.segments.last()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub id: NodeId,
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub id: NodeId,
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Let(LetStmt),
    Expr { expr: Expr, semi: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetStmt {
    pub pat: Pat,
    pub ty: Option<Type>,
    pub init: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pat {
    pub id: NodeId,
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatKind {
    Wild,
    Binding {
        mutable: bool,
        name: Ident,
    },
    Lit(LitPat),
    Tuple(Vec<Pat>),
    /// Variante unitaire (`Shape::Empty`) ou constante.
    Path(Path),
    Variant {
        path: Path,
        fields: VariantPatFields,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LitPat {
    Int { value: u128, suffix: Option<IntTy> },
    Bool(bool),
    Char(char),
    Str(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantPatFields {
    Tuple(Vec<Pat>),
    Struct(Vec<FieldPat>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldPat {
    pub name: Ident,
    pub pat: Pat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Unit,
    Int {
        value: u128,
        suffix: Option<IntTy>,
    },
    Float {
        text: String,
        suffix: Option<FloatTy>,
    },
    Bool(bool),
    Char(char),
    Str(String),
    SelfValue,
    Path(Path),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: Ident,
    },
    TupleIndex {
        base: Box<Expr>,
        index: u32,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    AssignOp {
        op: BinOp,
        target: Box<Expr>,
        value: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        ty: Type,
    },
    If {
        cond: Box<Expr>,
        then_block: Block,
        else_branch: Option<Box<Expr>>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    /// `while cond { body }` — expression de type `()`.
    While {
        cond: Box<Expr>,
        body: Block,
    },
    /// `loop { body }` — expression de type `()` (pas de `break` avec valeur en Core 1.0).
    Loop {
        body: Block,
    },
    /// `for pat in expr { body }` — itère un range `a..b` ou une collection
    /// (`Vec<T>` par valeur, `&Vec<T>`/`&[T]` par référence).
    For {
        pat: Pat,
        iter: Box<Expr>,
        body: Block,
    },
    Block(Block),
    /// `unsafe { ... }` — même sémantique de bloc, mais introduit l'effet
    /// `unsafe` et autorise l'appel de fonctions `extern`.
    Unsafe(Block),
    Tuple(Vec<Expr>),
    /// `[a, b, c]`
    Array(Vec<Expr>),
    /// `[value; len]`
    ArrayRepeat {
        value: Box<Expr>,
        len: Box<Expr>,
    },
    StructLit {
        path: Path,
        fields: Vec<FieldInit>,
    },
    Range {
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        inclusive: bool,
    },
    Try(Box<Expr>),
    Return(Option<Box<Expr>>),
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub id: NodeId,
    pub pat: Pat,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldInit {
    pub name: Ident,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    Ref,
    RefMut,
    Deref,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }

    pub fn is_comparison(&self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        )
    }

    pub fn is_arithmetic(&self) -> bool {
        matches!(
            self,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Rem
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr
        )
    }
}

/// Finds every module item in a tree, depth-first. Useful for tests and drivers.
pub fn walk_items<'a>(module: &'a Module, out: &mut Vec<&'a Item>) {
    for item in &module.items {
        out.push(item);
        match &item.kind {
            ItemKind::Mod(inner) => walk_items(inner, out),
            ItemKind::Impl(imp) => {
                for sub in &imp.items {
                    out.push(sub);
                }
            }
            _ => {}
        }
    }
}
