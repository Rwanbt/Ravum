//! AST span index.
//!
//! The checker records `Facts::resolutions` keyed by `Path` node ids and
//! `Facts::expr_types` keyed by expression ids, but neither map carries a span.
//! The language server therefore needs its own index from `NodeId` to `Span`;
//! this module builds exactly that, plus the declaration spans used for
//! "go to definition" and the symbol tree used by document symbols.
//!
//! Nothing here invents semantic facts: spans and names come from the AST the
//! parser produced.

use ravum_lang_syntax::ast::{
    Block, Expr, ExprKind, FieldPat, Item, ItemKind, Module, Pat, PatKind, Program, Stmt,
    StmtKind, Type, VariantDefFields, VariantPatFields,
};
use ravum_lang_syntax::span::Span;
use ravum_lang_syntax::NodeId;
use std::collections::BTreeMap;

/// LSP `SymbolKind` values used by this server.
pub mod symbol_kind {
    pub const MODULE: u32 = 2;
    pub const CLASS: u32 = 5;
    pub const METHOD: u32 = 6;
    pub const FIELD: u32 = 8;
    pub const ENUM: u32 = 10;
    pub const FUNCTION: u32 = 12;
    pub const CONSTANT: u32 = 14;
    pub const STRUCT: u32 = 23;
    pub const ENUM_MEMBER: u32 = 22;
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: u32,
    pub span: Span,
    pub detail: String,
    pub children: Vec<Symbol>,
}

#[derive(Debug, Clone, Default)]
pub struct AstIndex {
    /// Expression node id -> span.
    pub exprs: BTreeMap<NodeId, Span>,
    /// Value/type path node id -> span (the keys `Facts::resolutions` uses).
    pub paths: BTreeMap<NodeId, Span>,
    /// Item node id (functions, structs, enums, consts) -> span.
    pub items: BTreeMap<NodeId, Span>,
    /// Item node id -> display name.
    pub item_names: BTreeMap<NodeId, String>,
    /// Item node id -> span of its *name* token (what `rename` must rewrite).
    pub item_name_spans: BTreeMap<NodeId, Span>,
    /// Item node id -> LSP symbol kind.
    pub item_kinds: BTreeMap<NodeId, u32>,
    /// Pattern node id -> span.
    pub pats: BTreeMap<NodeId, Span>,
    /// Local binding name -> (declaration span, mutable) for navigation.
    pub bindings: Vec<Binding>,
    /// Enum item id -> (variant index -> variant span).
    pub variants: BTreeMap<NodeId, BTreeMap<usize, Span>>,
    /// Call expression id -> span of the callee path (for definition on calls).
    pub calls: BTreeMap<NodeId, Span>,
    /// Method-call expression id -> span of the method name.
    pub methods: BTreeMap<NodeId, Span>,
    /// Field access expression id -> span of the field name.
    pub fields: BTreeMap<NodeId, Span>,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub name: String,
    pub span: Span,
    pub mutable: bool,
}

impl AstIndex {
    pub fn build(program: &Program) -> Self {
        let mut index = Self::default();
        index.walk_module(&program.root);
        index
    }

    fn walk_module(&mut self, module: &Module) {
        for item in &module.items {
            self.walk_item(item);
        }
    }

    fn walk_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Fn(function) => {
                self.items.insert(item.id, item.span);
                self.item_names
                    .insert(item.id, function.name.name.clone());
                self.item_name_spans.insert(item.id, function.name.span);
                self.item_kinds.insert(item.id, symbol_kind::FUNCTION);
                for param in &function.params {
                    self.walk_param(&param.kind);
                }
                self.walk_block(&function.body);
            }
            ItemKind::Struct(definition) => {
                self.items.insert(item.id, item.span);
                self.item_names.insert(item.id, definition.name.name.clone());
                self.item_name_spans
                    .insert(item.id, definition.name.span);
                self.item_kinds.insert(item.id, symbol_kind::STRUCT);
                for field in &definition.fields {
                    self.items.insert(field.id, field.span);
                    self.item_names
                        .insert(field.id, field.name.name.clone());
                    self.item_kinds.insert(field.id, symbol_kind::FIELD);
                }
            }
            ItemKind::Enum(definition) => {
                self.items.insert(item.id, item.span);
                self.item_names.insert(item.id, definition.name.name.clone());
                self.item_name_spans
                    .insert(item.id, definition.name.span);
                self.item_kinds.insert(item.id, symbol_kind::ENUM);
                let mut variants = BTreeMap::new();
                for (position, variant) in definition.variants.iter().enumerate() {
                    variants.insert(position, variant.span);
                    self.items.insert(variant.id, variant.span);
                    self.item_names
                        .insert(variant.id, variant.name.name.clone());
                    self.item_name_spans
                        .insert(variant.id, variant.name.span);
                    self.item_kinds.insert(variant.id, symbol_kind::ENUM_MEMBER);
                    if let VariantDefFields::Struct(fields) = &variant.fields {
                        for field in fields {
                            self.items.insert(field.id, field.span);
                            self.item_names
                                .insert(field.id, field.name.name.clone());
                            self.item_kinds.insert(field.id, symbol_kind::FIELD);
                        }
                    }
                }
                self.variants.insert(item.id, variants);
            }
            ItemKind::Impl(implementation) => {
                for member in &implementation.items {
                    self.walk_item(member);
                }
            }
            ItemKind::Const(constant) => {
                self.items.insert(item.id, item.span);
                self.item_names.insert(item.id, constant.name.name.clone());
                self.item_name_spans
                    .insert(item.id, constant.name.span);
                self.item_kinds.insert(item.id, symbol_kind::CONSTANT);
                self.walk_expr(&constant.value);
            }
            ItemKind::Use(usage) => {
                self.paths.insert(usage.path.id, usage.path.span);
            }
            ItemKind::Mod(inner) => self.walk_module(inner),
            ItemKind::ModDecl(_) => {}
            ItemKind::Extern(block) => {
                for function in &block.fns {
                    self.items.insert(function.id, function.span);
                    self.item_names
                        .insert(function.id, function.name.name.clone());
                    self.item_kinds.insert(function.id, symbol_kind::FUNCTION);
                    for param in &function.params {
                        self.walk_param(&param.kind);
                    }
                }
            }
        }
    }

    fn walk_param(&mut self, kind: &ravum_lang_syntax::ast::ParamKind) {
        if let ravum_lang_syntax::ast::ParamKind::Binding { name, .. } = kind {
            self.bindings.push(Binding {
                name: name.name.clone(),
                span: name.span,
                mutable: false,
            });
        }
    }

    fn walk_block(&mut self, block: &Block) {
        for statement in &block.stmts {
            self.walk_stmt(statement);
        }
        if let Some(tail) = &block.tail {
            self.walk_expr(tail);
        }
    }

    fn walk_stmt(&mut self, statement: &Stmt) {
        match &statement.kind {
            StmtKind::Let(let_statement) => {
                self.walk_pat(&let_statement.pat);
                if let Some(initializer) = &let_statement.init {
                    self.walk_expr(initializer);
                }
            }
            StmtKind::Expr { expr, .. } => self.walk_expr(expr),
        }
    }

    fn walk_pat(&mut self, pattern: &Pat) {
        self.pats.insert(pattern.id, pattern.span);
        match &pattern.kind {
            PatKind::Wild | PatKind::Lit(_) => {}
            PatKind::Binding { mutable, name } => self.bindings.push(Binding {
                name: name.name.clone(),
                span: name.span,
                mutable: *mutable,
            }),
            PatKind::Tuple(children) => {
                for child in children {
                    self.walk_pat(child);
                }
            }
            PatKind::Path(path) => {
                self.paths.insert(path.id, path.span);
            }
            PatKind::Variant { path, fields } => {
                self.paths.insert(path.id, path.span);
                match fields {
                    VariantPatFields::Tuple(children) => {
                        for child in children {
                            self.walk_pat(child);
                        }
                    }
                    VariantPatFields::Struct(children) => {
                        for FieldPat { pat, .. } in children {
                            self.walk_pat(pat);
                        }
                    }
                }
            }
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        self.exprs.insert(expr.id, expr.span);
        match &expr.kind {
            ExprKind::Unit
            | ExprKind::Int { .. }
            | ExprKind::Float { .. }
            | ExprKind::Bool(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Break
            | ExprKind::Continue => {}
            ExprKind::SelfValue => {}
            ExprKind::Path(path) => {
                self.paths.insert(path.id, path.span);
            }
            ExprKind::Call { callee, args } => {
                self.calls.insert(expr.id, callee.span);
                self.walk_expr(callee);
                for argument in args {
                    self.walk_expr(argument);
                }
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                self.methods.insert(expr.id, method.span);
                self.walk_expr(receiver);
                for argument in args {
                    self.walk_expr(argument);
                }
            }
            ExprKind::Field { base, name } => {
                self.fields.insert(expr.id, name.span);
                self.walk_expr(base);
            }
            ExprKind::TupleIndex { base, .. } => self.walk_expr(base),
            ExprKind::Index { base, index } => {
                self.walk_expr(base);
                self.walk_expr(index);
            }
            ExprKind::Unary { expr, .. } => self.walk_expr(expr),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(lhs);
                self.walk_expr(rhs);
            }
            ExprKind::Assign { target, value } | ExprKind::AssignOp { target, value, .. } => {
                self.walk_expr(target);
                self.walk_expr(value);
            }
            ExprKind::Cast { expr, ty } => {
                self.walk_expr(expr);
                self.walk_type(ty);
            }
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.walk_expr(cond);
                self.walk_block(then_block);
                if let Some(branch) = else_branch {
                    self.walk_expr(branch);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.walk_expr(scrutinee);
                for arm in arms {
                    self.walk_pat(&arm.pat);
                    self.walk_expr(&arm.body);
                }
            }
            ExprKind::While { cond, body } => {
                self.walk_expr(cond);
                self.walk_block(body);
            }
            ExprKind::Loop { body } | ExprKind::Unsafe(body) | ExprKind::Block(body) => {
                self.walk_block(body)
            }
            ExprKind::For { pat, iter, body } => {
                self.walk_pat(pat);
                self.walk_expr(iter);
                self.walk_block(body);
            }
            ExprKind::Tuple(items) | ExprKind::Array(items) => {
                for item in items {
                    self.walk_expr(item);
                }
            }
            ExprKind::ArrayRepeat { value, len } => {
                self.walk_expr(value);
                self.walk_expr(len);
            }
            ExprKind::StructLit { path, fields } => {
                self.paths.insert(path.id, path.span);
                for field in fields {
                    self.walk_expr(&field.value);
                }
            }
            ExprKind::Range { start, end, .. } => {
                if let Some(start) = start {
                    self.walk_expr(start);
                }
                if let Some(end) = end {
                    self.walk_expr(end);
                }
            }
            ExprKind::Try(inner) => self.walk_expr(inner),
            ExprKind::Return(value) => {
                if let Some(value) = value {
                    self.walk_expr(value);
                }
            }
        }
    }

    fn walk_type(&mut self, ty: &Type) {
        use ravum_lang_syntax::ast::TypeKind;
        match &ty.kind {
            TypeKind::Path(path) => {
                self.paths.insert(path.id, path.span);
            }
            TypeKind::Ref { inner, .. } => self.walk_type(inner),
            TypeKind::Tuple(items) => {
                for item in items {
                    self.walk_type(item);
                }
            }
            TypeKind::Array { elem, .. } => self.walk_type(elem),
            TypeKind::Slice(inner) => self.walk_type(inner),
            TypeKind::Unit | TypeKind::Infer | TypeKind::SelfType => {}
        }
    }

    /// Smallest indexed path whose span contains `offset` in `file`.
    pub fn path_at(&self, file: u32, offset: u32) -> Option<NodeId> {
        smallest(&self.paths, file, offset)
    }

    /// Smallest indexed expression whose span contains `offset` in `file`.
    pub fn expr_at(&self, file: u32, offset: u32) -> Option<NodeId> {
        smallest(&self.exprs, file, offset)
    }

    /// Smallest indexed pattern whose span contains `offset` in `file`.
    pub fn pat_at(&self, file: u32, offset: u32) -> Option<NodeId> {
        smallest(&self.pats, file, offset)
    }

    /// Span of a declaration node (item, variant or local binding).
    pub fn declaration_span(&self, id: NodeId) -> Option<Span> {
        self.items.get(&id).copied()
    }

    /// Innermost local binding named `name` declared before `offset`.
    pub fn local_declaration(&self, name: &str, file: u32, offset: u32) -> Option<&Binding> {
        self.bindings
            .iter()
            .filter(|binding| {
                binding.name == name
                    && binding.span.file == file
                    && binding.span.start <= offset
            })
            .max_by_key(|binding| binding.span.start)
    }
}

fn smallest(map: &BTreeMap<NodeId, Span>, file: u32, offset: u32) -> Option<NodeId> {
    map.iter()
        .filter(|(_, span)| span.file == file && span.start <= offset && offset <= span.end)
        .min_by_key(|(_, span)| span.end.saturating_sub(span.start))
        .map(|(id, _)| *id)
}

/// Render a syntactic type. `ast::Type` deliberately has no `Display` (only the
/// resolved `sema::Type` does), so the symbol tree renders the written form.
pub fn type_text(ty: &Type) -> String {
    use ravum_lang_syntax::ast::{ExprKind, TypeKind};
    match &ty.kind {
        TypeKind::Path(path) => path.display(),
        TypeKind::Ref { mutable, inner } => {
            if *mutable {
                format!("&mut {}", type_text(inner))
            } else {
                format!("&{}", type_text(inner))
            }
        }
        TypeKind::Tuple(items) => format!(
            "({})",
            items
                .iter()
                .map(type_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeKind::Array { elem, len } => {
            let length = match &len.kind {
                ExprKind::Int { value, .. } => value.to_string(),
                _ => "_".to_string(),
            };
            format!("[{}; {}]", type_text(elem), length)
        }
        TypeKind::Slice(inner) => format!("[{}]", type_text(inner)),
        TypeKind::Unit => "()".to_string(),
        TypeKind::Infer => "_".to_string(),
        TypeKind::SelfType => "Self".to_string(),
    }
}

/// Build the document-symbol tree for one file.
pub fn document_symbols(program: &Program, file: u32) -> Vec<Symbol> {
    module_symbols(&program.root, file)
}

fn module_symbols(module: &Module, file: u32) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for item in &module.items {
        if item.span.file != file {
            continue;
        }
        if let Some(symbol) = item_symbol(item, file) {
            symbols.push(symbol);
        }
    }
    symbols
}

fn item_symbol(item: &Item, file: u32) -> Option<Symbol> {
    let symbol = match &item.kind {
        ItemKind::Fn(function) => Symbol {
            name: function.name.name.clone(),
            kind: symbol_kind::FUNCTION,
            span: function.name.span,
            detail: signature_detail(function),
            children: Vec::new(),
        },
        ItemKind::Struct(definition) => Symbol {
            name: definition.name.name.clone(),
            kind: symbol_kind::STRUCT,
            span: definition.name.span,
            detail: String::new(),
            children: definition
                .fields
                .iter()
                .filter(|field| field.span.file == file)
                .map(|field| Symbol {
                    name: field.name.name.clone(),
                    kind: symbol_kind::FIELD,
                    span: field.name.span,
                    detail: type_text(&field.ty),
                    children: Vec::new(),
                })
                .collect(),
        },
        ItemKind::Enum(definition) => Symbol {
            name: definition.name.name.clone(),
            kind: symbol_kind::ENUM,
            span: definition.name.span,
            detail: String::new(),
            children: definition
                .variants
                .iter()
                .filter(|variant| variant.span.file == file)
                .map(|variant| Symbol {
                    name: variant.name.name.clone(),
                    kind: symbol_kind::ENUM_MEMBER,
                    span: variant.name.span,
                    detail: String::new(),
                    children: Vec::new(),
                })
                .collect(),
        },
        ItemKind::Impl(implementation) => Symbol {
            name: format!("impl {}", type_text(&implementation.self_ty)),
            kind: symbol_kind::CLASS,
            span: implementation.self_ty.span,
            detail: String::new(),
            children: implementation
                .items
                .iter()
                .filter_map(|member| item_symbol(member, file))
                .collect(),
        },
        ItemKind::Const(constant) => Symbol {
            name: constant.name.name.clone(),
            kind: symbol_kind::CONSTANT,
            span: constant.name.span,
            detail: type_text(&constant.ty),
            children: Vec::new(),
        },
        ItemKind::Mod(inner) => Symbol {
            name: inner
                .name
                .as_ref()
                .map(|name| name.name.clone())
                .unwrap_or_else(|| "<module>".to_string()),
            kind: symbol_kind::MODULE,
            span: inner.span,
            detail: String::new(),
            children: module_symbols(inner, file),
        },
        ItemKind::Extern(block) => Symbol {
            name: format!("extern \"{}\"", block.abi.name),
            kind: symbol_kind::MODULE,
            span: block.span,
            detail: String::new(),
            children: block
                .fns
                .iter()
                .filter(|function| function.span.file == file)
                .map(|function| Symbol {
                    name: function.name.name.clone(),
                    kind: symbol_kind::FUNCTION,
                    span: function.name.span,
                    detail: String::new(),
                    children: Vec::new(),
                })
                .collect(),
        },
        ItemKind::Use(_) | ItemKind::ModDecl(_) => return None,
    };
    Some(symbol)
}

fn signature_detail(function: &ravum_lang_syntax::ast::FnItem) -> String {
    let params = function
        .params
        .iter()
        .map(|param| match &param.kind {
            ravum_lang_syntax::ast::ParamKind::Receiver(receiver) => match receiver {
                ravum_lang_syntax::ast::Receiver::Shared => "&self".to_string(),
                ravum_lang_syntax::ast::Receiver::Exclusive => "&mut self".to_string(),
                ravum_lang_syntax::ast::Receiver::ByValue => "self".to_string(),
            },
            ravum_lang_syntax::ast::ParamKind::Binding { name, ty } => {
                format!("{}: {}", name.name, type_text(ty))
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("({params}) -> {}", type_text(&function.ret))
}
