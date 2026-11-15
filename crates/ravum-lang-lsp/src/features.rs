//! Language features: navigation, hover, completion, symbols, rename,
//! formatting.
//!
//! Every answer is derived from checker facts (`Facts::resolutions`,
//! `Facts::expr_types`, signatures) or from the AST. When the checker has no
//! fact for the cursor, the feature returns "nothing" instead of guessing: a
//! wrong jump is worse than no jump in an editor.
//!
//! Documented limitations (also stated in `docs/qualification/LSP.md`):
//! * rename/references on *locals* are only offered when the name is declared
//!   exactly once in the file, because Core 1.0 has no per-binding resolution
//!   map to disambiguate shadowing;
//! * a definition that points into another package can only be resolved when
//!   the target file exists at its package-relative display path.

use ravum_lang_sema::builtins;
use ravum_lang_sema::facts::{CheckedProgram, Resolution, VariantKind};
use ravum_lang_syntax::span::Span;
use ravum_lang_syntax::NodeId;
use serde_json::{json, Value};

use crate::analysis::Analysis;
use crate::convert::{range_json, span_range, TextIndex};
use crate::paths::{path_to_uri, uri_to_path};
use crate::walk::{symbol_kind, Symbol};

/// Core 1.0 keywords offered by completion (mirrors `token::Kw`).
const KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "const", "struct", "enum", "impl", "use", "pub", "mod", "match", "if",
    "else", "while", "loop", "for", "in", "break", "continue", "return", "true", "false", "as",
    "test", "unsafe", "self", "Self", "crate", "super", "extern", "effects",
];

fn facts(analysis: &Analysis) -> Option<&CheckedProgram> {
    analysis.checked.as_ref()
}

/// Resolution of the symbol under the cursor.
///
/// A cursor on a *use* goes through `Facts::resolutions`, which is the checker's
/// own answer. A cursor on the *declaration name* has no path node, so the item
/// itself is the resolution target: rename and references from a declaration are
/// what every editor exposes, and refusing them would be a bug, not a
/// limitation.
fn resolution_at(analysis: &Analysis, file: u32, offset: u32) -> Option<Resolution> {
    let checked = facts(analysis)?;
    if let Some(node) = analysis.index.path_at(file, offset) {
        if let Some(resolution) = checked.facts.resolutions.get(&node) {
            return Some(resolution.clone());
        }
    }
    let item = analysis.index.item_name_at(file, offset)?;
    Some(Resolution::Item { item })
}

/// Absolute path for a display path recorded by the loader.
pub fn document_path(analysis: &Analysis, display: &str) -> Option<std::path::PathBuf> {
    let root = &analysis.root;
    let base = if root.is_dir() {
        root.clone()
    } else {
        root.parent().unwrap_or(root).to_path_buf()
    };
    let candidate = if std::path::Path::new(display).is_absolute() {
        std::path::PathBuf::from(display)
    } else {
        base.join(display)
    };
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

fn location(analysis: &Analysis, span: Span) -> Option<Value> {
    let sources = analysis.sources();
    let file = sources.file(span.file)?;
    let path = document_path(analysis, &file.path)?;
    let range = span_range(sources, span)?;
    Some(json!({ "uri": path_to_uri(&path), "range": range }))
}

/// Declaration span for a resolution target.
fn target_span(
    analysis: &Analysis,
    resolution: &Resolution,
    file: u32,
    offset: u32,
) -> Option<Span> {
    let index = &analysis.index;
    match resolution {
        // Navigation should land on the *name*, not on the whole item.
        Resolution::Item { item } => index
            .item_name_spans
            .get(item)
            .copied()
            .or_else(|| index.declaration_span(*item)),
        Resolution::Variant { enum_item, variant } => index
            .variants
            .get(enum_item)
            .and_then(|variants| variants.get(variant).copied()),
        Resolution::Local { name, .. } => {
            let occurrences = index
                .bindings
                .iter()
                .filter(|binding| binding.name == *name && binding.span.file == file)
                .count();
            if occurrences == 1 {
                index
                    .local_declaration(name, file, u32::MAX)
                    .map(|b| b.span)
            } else {
                index.local_declaration(name, file, offset).map(|b| b.span)
            }
        }
        _ => None,
    }
}

fn signature_markdown(analysis: &Analysis, item: NodeId) -> Option<String> {
    let checked = facts(analysis)?;
    let facts = &checked.facts;
    if let Some(signature) = facts.fn_sigs.get(&item) {
        let params = signature
            .params
            .iter()
            .map(|param| format!("{}: {}", param.name, param.ty))
            .collect::<Vec<_>>()
            .join(", ");
        let mut body = format!("fn {}({}) -> {}", signature.name, params, signature.ret);
        if signature.is_unsafe {
            body = format!("unsafe {body}");
        }
        let mut out = format!("```ravum\n{body}\n```\n");
        out.push_str(&format!("effects: `{}`", signature.effects.display()));
        if signature.is_test {
            out.push_str("\n\n_test function_");
        }
        return Some(out);
    }
    if let Some(definition) = facts.structs.get(&item) {
        let fields = definition
            .fields
            .iter()
            .map(|field| format!("    {}: {},", field.name, field.ty))
            .collect::<Vec<_>>()
            .join("\n");
        return Some(format!(
            "```ravum\nstruct {} {{\n{fields}\n}}\n```",
            definition.name
        ));
    }
    if let Some(definition) = facts.enums.get(&item) {
        let variants = definition
            .variants
            .iter()
            .map(|variant| format!("    {},", variant.name))
            .collect::<Vec<_>>()
            .join("\n");
        return Some(format!(
            "```ravum\nenum {} {{\n{variants}\n}}\n```",
            definition.name
        ));
    }
    if let Some(constant) = facts.consts.get(&item) {
        return Some(format!(
            "```ravum\nconst {}: {}\n```",
            constant.name, constant.ty
        ));
    }
    facts
        .item_paths
        .get(&item)
        .map(|path| format!("```ravum\n{path}\n```"))
}

fn describe(analysis: &Analysis, resolution: &Resolution) -> Option<String> {
    match resolution {
        Resolution::Item { item } => signature_markdown(analysis, *item),
        Resolution::Variant { enum_item, variant } => {
            let checked = facts(analysis)?;
            let definition = checked.facts.enums.get(enum_item)?;
            let info = definition.variants.get(*variant)?;
            let payload = match info.kind {
                VariantKind::Unit => String::new(),
                VariantKind::Tuple => format!(
                    "({})",
                    info.tuple_types
                        .iter()
                        .map(|ty| format!("{ty}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                VariantKind::Struct => format!(
                    " {{ {} }}",
                    info.fields
                        .iter()
                        .map(|field| format!("{}: {}", field.name, field.ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
            Some(format!(
                "```ravum\n{}{}\n```\n\nvariant of `enum {}`",
                info.name, payload, definition.name
            ))
        }
        Resolution::Local { name, mutable, .. } => Some(format!(
            "```ravum\n{} {name}\n```",
            if *mutable { "let mut" } else { "let" }
        )),
        Resolution::BuiltinFn { path } => {
            let signature = builtins::builtin_fn(path)?;
            let effects = signature
                .effects
                .iter()
                .map(|effect| effect.name())
                .collect::<Vec<_>>()
                .join(", ");
            Some(format!(
                "```ravum\nfn {}({}) -> {}\n```\n\neffects: `{}`",
                signature.path,
                signature.params.join(", "),
                signature.ret,
                effects
            ))
        }
        Resolution::BuiltinConst { path } => Some(format!("```ravum\nconst {path}\n```")),
        Resolution::Module | Resolution::BuiltinModule => Some("module".to_string()),
        Resolution::Error => None,
    }
}

pub fn hover(analysis: &Analysis, file: u32, offset: u32) -> Option<Value> {
    let checked = facts(analysis)?;
    let facts = &checked.facts;
    let sources = analysis.sources();

    if let Some(node) = analysis.index.path_at(file, offset) {
        if let Some(resolution) = facts.resolutions.get(&node) {
            if let Some(markdown) = describe(analysis, resolution) {
                let mut value = json!({
                    "contents": { "kind": "markdown", "value": markdown },
                });
                if let Some(range) = analysis
                    .index
                    .paths
                    .get(&node)
                    .copied()
                    .and_then(|span| span_range(sources, span))
                {
                    value["range"] = range;
                }
                return Some(value);
            }
        }
    }

    if let Some(node) = analysis.index.expr_at(file, offset) {
        if let Some(ty) = facts.expr_types.get(&node) {
            let mut value = json!({
                "contents": { "kind": "markdown", "value": format!("```ravum\n{ty}\n```") },
            });
            if let Some(range) = analysis
                .index
                .exprs
                .get(&node)
                .copied()
                .and_then(|span| span_range(sources, span))
            {
                value["range"] = range;
            }
            return Some(value);
        }
    }

    // Cursor on a declaration name: show the item's own signature.
    if let Some(item) = analysis.index.item_name_at(file, offset) {
        if let Some(markdown) = signature_markdown(analysis, item) {
            let mut value = json!({
                "contents": { "kind": "markdown", "value": markdown },
            });
            if let Some(range) = analysis
                .index
                .item_name_spans
                .get(&item)
                .copied()
                .and_then(|span| span_range(sources, span))
            {
                value["range"] = range;
            }
            return Some(value);
        }
    }
    None
}

pub fn definition(analysis: &Analysis, file: u32, offset: u32) -> Option<Value> {
    let resolution = resolution_at(analysis, file, offset)?;
    let span = target_span(analysis, &resolution, file, offset)?;
    let location = location(analysis, span)?;
    Some(json!([location]))
}

fn same_target(a: &Resolution, b: &Resolution) -> bool {
    match (a, b) {
        (Resolution::Item { item: a }, Resolution::Item { item: b }) => a == b,
        (
            Resolution::Variant {
                enum_item: a,
                variant: av,
            },
            Resolution::Variant {
                enum_item: b,
                variant: bv,
            },
        ) => a == b && av == bv,
        (Resolution::Local { name: a, .. }, Resolution::Local { name: b, .. }) => a == b,
        _ => false,
    }
}

/// Number of declarations of `name` in `file` (used to refuse unsafe local
/// renames when shadowing is possible).
fn local_declarations(dossier: &Analysis, name: &str, file: u32) -> usize {
    dossier
        .index
        .bindings
        .iter()
        .filter(|binding| binding.name == name && binding.span.file == file)
        .count()
}

pub fn references(
    analysis: &Analysis,
    file: u32,
    offset: u32,
    include_declaration: bool,
) -> Option<Value> {
    let checked = facts(analysis)?;
    let target = resolution_at(analysis, file, offset)?;
    if matches!(target, Resolution::Error) {
        return None;
    }
    if let Resolution::Local { name, .. } = &target {
        if local_declarations(analysis, name, file) > 1 {
            // Shadowing makes a name-based search unsound; refuse rather than
            // return references that may belong to another binding.
            return None;
        }
    }

    let mut locations: Vec<Value> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (id, resolution) in &checked.facts.resolutions {
        if !same_target(&target, resolution) {
            continue;
        }
        let span = analysis
            .index
            .paths
            .get(id)
            .copied()
            .or_else(|| analysis.index.exprs.get(id).copied());
        let Some(span) = span else { continue };
        let Some(value) = location(analysis, span) else {
            continue;
        };
        let key = serde_json::to_string(&value).unwrap_or_default();
        if seen.insert(key) {
            locations.push(value);
        }
    }
    if include_declaration {
        if let Some(span) = target_span(analysis, &target, file, offset) {
            if let Some(value) = location(analysis, span) {
                let key = serde_json::to_string(&value).unwrap_or_default();
                if seen.insert(key) {
                    locations.push(value);
                }
            }
        }
    }
    Some(json!(locations))
}

pub fn is_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if ravum_lang_syntax::token::Kw::from_str(name).is_some() {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_alphanumeric() || ch == '_')
}

/// Span of the renameable symbol under the cursor, if any.
fn symbol_span(analysis: &Analysis, file: u32, offset: u32) -> Option<Span> {
    if let Some(node) = analysis.index.path_at(file, offset) {
        if let Some(span) = analysis.index.paths.get(&node).copied() {
            return Some(span);
        }
    }
    let item = analysis.index.item_name_at(file, offset)?;
    analysis.index.item_name_spans.get(&item).copied()
}

/// `textDocument/prepareRename`: clients such as VS Code query this before
/// showing the rename box. Without an answer the whole rename command fails,
/// even when the edit itself would be valid.
pub fn prepare_rename(analysis: &Analysis, file: u32, offset: u32) -> Option<Value> {
    let span = symbol_span(analysis, file, offset)?;
    // The result is a bare `Range`: that is the LSP variant every client
    // understands. Returning `{ range, defaultBehavior }` makes clients such as
    // VS Code fail to recognise the answer and refuse the rename outright.
    span_range(analysis.sources(), span)
}

pub fn rename(
    analysis: &Analysis,
    file: u32,
    offset: u32,
    new_name: &str,
) -> Result<Option<Value>, String> {
    if !is_identifier(new_name) {
        return Err(format!(
            "`{new_name}` is not a valid Ravum identifier (or is a keyword)"
        ));
    }
    let Some(checked) = facts(analysis) else {
        return Ok(None);
    };
    let Some(target) = resolution_at(analysis, file, offset) else {
        return Ok(None);
    };
    if matches!(target, Resolution::Error) {
        return Ok(None);
    }

    let mut edits: std::collections::BTreeMap<String, Vec<Value>> =
        std::collections::BTreeMap::new();
    let push = |analysis: &Analysis,
                span: Span,
                edits: &mut std::collections::BTreeMap<String, Vec<Value>>| {
        let sources = analysis.sources();
        let Some(source) = sources.file(span.file) else {
            return;
        };
        let Some(path) = document_path(analysis, &source.path) else {
            return;
        };
        let Some(range) = span_range(sources, span) else {
            return;
        };
        edits
            .entry(path_to_uri(&path))
            .or_default()
            .push(json!({ "range": range, "newText": new_name }));
    };

    match &target {
        Resolution::Local { name, .. } => {
            if local_declarations(analysis, name, file) > 1 {
                return Err(format!(
                    "`{name}` is declared more than once in this file; renaming a shadowed \
                     local is refused because Core 1.0 has no per-binding resolution map"
                ));
            }
            for (id, resolution) in &checked.facts.resolutions {
                if same_target(&target, resolution) {
                    if let Some(span) = analysis
                        .index
                        .paths
                        .get(id)
                        .copied()
                        .or_else(|| analysis.index.exprs.get(id).copied())
                    {
                        push(analysis, span, &mut edits);
                    }
                }
            }
            if let Some(span) = target_span(analysis, &target, file, offset) {
                push(analysis, span, &mut edits);
            }
        }
        Resolution::Item { item } => {
            for (id, resolution) in &checked.facts.resolutions {
                if same_target(&target, resolution) {
                    if let Some(span) = analysis
                        .index
                        .paths
                        .get(id)
                        .copied()
                        .or_else(|| analysis.index.exprs.get(id).copied())
                    {
                        push(analysis, span, &mut edits);
                    }
                }
            }
            if let Some(span) = analysis.index.item_name_spans.get(item).copied() {
                push(analysis, span, &mut edits);
            }
        }
        Resolution::Variant { .. } => {
            if let Some(span) = target_span(analysis, &target, file, offset) {
                push(analysis, span, &mut edits);
            }
            for (id, resolution) in &checked.facts.resolutions {
                if same_target(&target, resolution) {
                    if let Some(span) = analysis.index.paths.get(id).copied() {
                        push(analysis, span, &mut edits);
                    }
                }
            }
        }
        _ => return Ok(None),
    }

    if edits.is_empty() {
        return Ok(None);
    }
    Ok(Some(json!({ "changes": edits })))
}

pub fn completion(analysis: &Analysis) -> Value {
    let mut items: Vec<Value> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut add = |label: &str, kind: u32, detail: &str, items: &mut Vec<Value>| {
        if seen.insert(label.to_string()) {
            items.push(json!({ "label": label, "kind": kind, "detail": detail }));
        }
    };
    for keyword in KEYWORDS {
        add(keyword, 14, "keyword", &mut items);
    }
    for name in builtins::PRELUDE_VALUES {
        add(name, 3, "prelude", &mut items);
    }
    for name in builtins::PRELUDE_TYPES {
        add(name, 7, "prelude type", &mut items);
    }
    for name in builtins::PRELUDE_MODULES {
        add(name, 9, "module", &mut items);
    }
    if let Some(checked) = facts(analysis) {
        for (item, path) in &checked.facts.item_paths {
            if let Some(name) = analysis.index.item_names.get(item) {
                let kind = analysis
                    .index
                    .item_kinds
                    .get(item)
                    .copied()
                    .map(completion_kind)
                    .unwrap_or(3);
                add(name, kind, path, &mut items);
            }
        }
    }
    json!({ "isIncomplete": false, "items": items })
}

fn completion_kind(symbol: u32) -> u32 {
    match symbol {
        symbol_kind::FUNCTION => 3,
        symbol_kind::METHOD => 2,
        symbol_kind::STRUCT => 22,
        symbol_kind::ENUM => 13,
        symbol_kind::ENUM_MEMBER => 20,
        symbol_kind::CONSTANT => 21,
        symbol_kind::MODULE => 9,
        symbol_kind::FIELD => 5,
        _ => 3,
    }
}

pub fn document_symbols(analysis: &Analysis, file: u32) -> Value {
    let symbols = match &analysis.checked {
        Some(checked) => crate::walk::document_symbols(&checked.program, file),
        None => match &analysis.package.program {
            Some(program) => crate::walk::document_symbols(program, file),
            None => Vec::new(),
        },
    };
    let converted = symbols
        .iter()
        .filter_map(|symbol| symbol_json(analysis, symbol))
        .collect::<Vec<_>>();
    json!(converted)
}

fn symbol_json(analysis: &Analysis, symbol: &Symbol) -> Option<Value> {
    let sources = analysis.sources();
    let range = span_range(sources, symbol.span)?;
    Some(json!({
        "name": symbol.name,
        "kind": symbol.kind,
        "detail": symbol.detail,
        "range": range,
        "selectionRange": range,
        "children": symbol
            .children
            .iter()
            .filter_map(|child| symbol_json(analysis, child))
            .collect::<Vec<_>>(),
    }))
}

pub fn workspace_symbols(analysis: &Analysis, query: &str) -> Value {
    let Some(checked) = facts(analysis) else {
        return json!([]);
    };
    let needle = query.to_lowercase();
    let mut items: Vec<Value> = Vec::new();
    let mut entries: Vec<(&str, &NodeId)> = checked
        .facts
        .item_paths
        .iter()
        .map(|(item, path)| (path.as_str(), item))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    for (path, item) in entries {
        let name = analysis
            .index
            .item_names
            .get(item)
            .cloned()
            .unwrap_or_else(|| path.rsplit("::").next().unwrap_or(path).to_string());
        if !needle.is_empty()
            && !name.to_lowercase().contains(&needle)
            && !path.to_lowercase().contains(&needle)
        {
            continue;
        }
        let kind = analysis
            .index
            .item_kinds
            .get(item)
            .copied()
            .map(completion_kind)
            .unwrap_or(3);
        let mut value = json!({ "name": path, "kind": kind });
        if let Some(span) = analysis.index.declaration_span(*item) {
            if let Some(location) = location(analysis, span) {
                value["location"] = location;
            }
        }
        items.push(value);
    }
    json!(items)
}

/// Whole-document text edit from the formatter, or `None` when the source does
/// not parse (the formatter refuses to rewrite invalid source).
pub fn formatting(text: &str) -> Option<Value> {
    match ravum_lang_syntax::fmt::format_source(0, text) {
        Ok(formatted) => {
            if formatted == text {
                return Some(json!([]));
            }
            let index = TextIndex::new(text);
            let end_line = index.line_count().saturating_sub(1) as u32;
            let end_character = text
                .rsplit('\n')
                .next()
                .map(|last| last.encode_utf16().count() as u32)
                .unwrap_or(0);
            Some(json!([{
                "range": range_json((0, 0), (end_line, end_character)),
                "newText": formatted,
            }]))
        }
        Err(_) => None,
    }
}

/// Resolve a URI to an absolute path, used by the server for `didOpen` etc.
pub fn uri_path(uri: &str) -> Option<std::path::PathBuf> {
    uri_to_path(uri)
}
