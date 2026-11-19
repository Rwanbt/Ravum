//! Checked-program facts: the only surface the code generator may consume.
//!
//! A valid `CheckedProgram` guarantees: every path is resolved, every expression
//! has a type, every call has a target, every function has an effect set, and no
//! diagnostic of severity `error` was produced. The code generator never guesses.

use crate::types::Type;
use ravum_lang_syntax::ast::{NodeId, Program, Receiver};
use ravum_lang_syntax::diag::DiagnosticBag;
use ravum_lang_syntax::span::Span;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// One effect group. Names are part of the public diagnostic/JSON contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Effect {
    Io,
    Fs,
    Net,
    Thread,
    Alloc,
    Panic,
    Unsafe,
    Ffi,
    Async,
    Rt,
    Nondet,
}

impl Effect {
    pub fn name(&self) -> &'static str {
        match self {
            Effect::Io => "io",
            Effect::Fs => "fs",
            Effect::Net => "net",
            Effect::Thread => "thread",
            Effect::Alloc => "alloc",
            Effect::Panic => "panic",
            Effect::Unsafe => "unsafe",
            Effect::Ffi => "ffi",
            Effect::Async => "async",
            Effect::Rt => "rt",
            Effect::Nondet => "nondet",
        }
    }

    pub fn from_name(name: &str) -> Option<Effect> {
        Some(match name {
            "io" => Effect::Io,
            "fs" => Effect::Fs,
            "net" => Effect::Net,
            "thread" => Effect::Thread,
            "alloc" => Effect::Alloc,
            "panic" => Effect::Panic,
            "unsafe" => Effect::Unsafe,
            "ffi" => Effect::Ffi,
            "async" => Effect::Async,
            "rt" => Effect::Rt,
            "nondet" => Effect::Nondet,
            _ => return None,
        })
    }

    /// Effects that are declared but cannot be produced in Core 1.0. Empty
    /// since `async` became producible (RFC-E11-CONCURRENCY phase 2, D-19-9).
    pub fn is_unsupported(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EffectSet {
    pub effects: BTreeSet<Effect>,
}

impl EffectSet {
    pub fn new(effects: impl IntoIterator<Item = Effect>) -> Self {
        Self {
            effects: effects.into_iter().collect(),
        }
    }

    pub fn insert(&mut self, effect: Effect) {
        self.effects.insert(effect);
    }

    pub fn extend(&mut self, other: &EffectSet) {
        self.effects.extend(other.effects.iter().copied());
    }

    pub fn contains(&self, effect: Effect) -> bool {
        self.effects.contains(&effect)
    }

    pub fn difference(&self, other: &EffectSet) -> Vec<Effect> {
        self.effects.difference(&other.effects).copied().collect()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.effects.iter().map(|e| e.name()).collect()
    }

    pub fn display(&self) -> String {
        if self.effects.is_empty() {
            "pure".to_string()
        } else {
            self.names().join(", ")
        }
    }
}

/// Verification profile selected by `ravum-lang check --profile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Core,
    Pure,
    Rt,
}

impl Profile {
    pub fn parse(s: &str) -> Option<Profile> {
        match s {
            "core" => Some(Profile::Core),
            "pure" => Some(Profile::Pure),
            "rt" => Some(Profile::Rt),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Profile::Core => "core",
            Profile::Pure => "pure",
            Profile::Rt => "rt",
        }
    }

    /// Effects forbidden at the profile entry point. `None` = no restriction.
    pub fn forbidden(&self) -> Option<EffectSet> {
        match self {
            Profile::Core => None,
            Profile::Pure => Some(EffectSet::new([
                Effect::Io,
                Effect::Fs,
                Effect::Net,
                Effect::Thread,
                Effect::Alloc,
                Effect::Panic,
                Effect::Unsafe,
                Effect::Ffi,
                Effect::Async,
                Effect::Rt,
                Effect::Nondet,
            ])),
            Profile::Rt => Some(EffectSet::new([
                Effect::Io,
                Effect::Fs,
                Effect::Net,
                Effect::Thread,
                Effect::Alloc,
                Effect::Panic,
                Effect::Unsafe,
                Effect::Ffi,
                Effect::Async,
                Effect::Rt,
            ])),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnSig {
    pub item: NodeId,
    /// Full Rust path (`crate::m::f`), usable by the generator.
    pub rust_path: String,
    pub name: String,
    pub generics: Vec<String>,
    pub params: Vec<ParamSig>,
    pub ret: Type,
    pub receiver: Option<Receiver>,
    pub effects: EffectSet,
    pub declared_effects: Option<EffectSet>,
    pub is_test: bool,
    pub is_unsafe: bool,
    /// `true` for a function declared in an `extern "C" { ... }` block: it has
    /// no Ravum body, is callable only inside `unsafe`, and carries the `ffi`
    /// and `unsafe` effects.
    pub is_extern: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSig {
    pub name: String,
    pub ty: Type,
    pub is_receiver: bool,
    /// `mut name: Ty` — the binding is mutable inside the body.
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    pub name: String,
    pub rust_name: String,
    pub ty: Type,
    pub public: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructInfo {
    pub item: NodeId,
    pub rust_path: String,
    pub name: String,
    pub generics: Vec<String>,
    pub fields: Vec<FieldInfo>,
    pub is_copy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariantKind {
    Unit,
    Tuple,
    Struct,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantInfo {
    pub index: usize,
    pub name: String,
    pub rust_name: String,
    pub kind: VariantKind,
    /// For tuple variants: payload types. For struct variants: fields.
    pub tuple_types: Vec<Type>,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumInfo {
    pub item: NodeId,
    pub rust_path: String,
    pub name: String,
    pub generics: Vec<String>,
    pub variants: Vec<VariantInfo>,
    pub is_copy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstValue {
    Int(i128),
    Bool(bool),
    Char(char),
    Str(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstInfo {
    pub item: NodeId,
    pub rust_path: String,
    pub name: String,
    pub ty: Type,
    pub value: ConstValue,
}

/// Resolution of a value path (`x`, `f`, `io::println`, `E::V`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Local {
        name: String,
        mutable: bool,
        /// Declaration span of the binding: the `Pat` span for a `let`, match
        /// or `for` binding, and the enclosing item span for a parameter or
        /// `self` (the same identity `OwnFacts::place` records). The emitter's
        /// step-B guard uses it to verify the binding, not only the
        /// projections.
        decl: Span,
    },
    Item {
        item: NodeId,
    },
    Variant {
        enum_item: NodeId,
        variant: usize,
    },
    Module,
    BuiltinModule,
    BuiltinFn {
        path: String,
    },
    BuiltinConst {
        path: String,
    },
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
    UserFn { item: NodeId },
    BuiltinFn { path: String },
    VariantCtor { enum_item: NodeId, variant: usize },
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodTarget {
    User { item: NodeId },
    Builtin { path: String, ret: Type },
    Error,
}

/// One source file registered in the checked program (for `// rv:` line maps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub path: String,
    pub line_starts: Vec<u32>,
}

impl FileInfo {
    pub fn line_of(&self, offset: u32) -> u32 {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i as u32 + 1,
            Err(i) => i.saturating_sub(1) as u32 + 1,
        }
    }
}

/// How one expression uses its value (RFC-OWN-FACTS-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    Move,
    Copy,
    Borrow,
    BorrowMut,
}

/// Index projection of a place: a literal index, or an opaque one that may
/// equal any other index (conservative, Q-OWN-5).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectionIndex {
    Lit(u128),
    Opaque,
}

/// One projection on a place, mirroring `own.rs`'s internal `Step`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Projection {
    Deref,
    Field(String),
    Tuple(u32),
    Index(ProjectionIndex),
}

/// A place as recorded for one expression: the binding identified by its
/// declaration span (stable within a checked program) and its projections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnPlace {
    pub binding: Span,
    pub steps: Vec<Projection>,
}

/// Opaque identity of one binding (parameter, `self`, `let`, match or `for`
/// binding), unique within a checked program (RFC-OWN-FACTS-2 step C4).
///
/// Two parameters of the same function have different identities, and a
/// shadowing binding is a new identity, never a reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingId(pub u32);

/// Ownership decisions taken by `own.rs`, recorded for the backend
/// (RFC-OWN-FACTS-1 step A, extended by RFC-OWN-FACTS-2 step C).
#[derive(Debug, Clone, Default)]
pub struct OwnFacts {
    /// For every expression with a value use: move, copy, or borrow.
    pub use_kind: BTreeMap<NodeId, UseKind>,
    /// For every expression that is a place: binding + projections.
    pub place: BTreeMap<NodeId, OwnPlace>,
    /// Assignment targets that re-initialise a moved place located under an
    /// array index (`a[0] = e` after `a[0]` was moved). Rust re-initialises an
    /// array element only by assigning the whole array, so the backend rebuilds
    /// the array at these targets (`spec/language/ownership.md` OWN-5.4).
    pub element_reinit: BTreeSet<NodeId>,
    /// Step C1 of RFC-OWN-FACTS-2: the resolved types that are `Copy` at the
    /// ownership level, keyed by type-syntax node id. The backend must read this
    /// instead of re-deriving copy-ness from `Type`; while the derivation still
    /// exists it is compared against this table by the step-B guard.
    pub copy_type: BTreeSet<NodeId>,
    /// Step C2: place expressions whose *value* the checker takes (the
    /// expression is evaluated by value, not borrowed, not assigned). A member
    /// is either moved (`consumed`) or copied.
    pub owned_position: BTreeSet<NodeId>,
    /// Step C3: place expressions the checker effectively moves out of (an
    /// owned position of a non-`Copy` type). The decision is recorded wherever
    /// the checker evaluates the expression: direct flow, `if` branch, `match`
    /// arm, block tail or loop body alike.
    pub consumed: BTreeSet<NodeId>,
    /// Step C4: the binding every local path expression (`x`, `self`) refers
    /// to, as the checker resolved it through its scopes.
    pub binding_identity: BTreeMap<NodeId, BindingId>,
    /// Declaration (span, name) of every binding identity. The span alone is
    /// not an identity: the parameters of one function share the item span.
    pub bindings: BTreeMap<(Span, String), BindingId>,
}

impl OwnFacts {
    /// The identity of the binding `name` declared at `decl`, created on first
    /// use. Idempotent, so the silent fixpoint passes of a loop and its final
    /// pass agree on the identity of the bindings declared in the body.
    pub fn intern_binding(&mut self, decl: Span, name: &str) -> BindingId {
        let next = BindingId(u32::try_from(self.bindings.len()).unwrap_or(u32::MAX));
        *self
            .bindings
            .entry((decl, name.to_string()))
            .or_insert(next)
    }
}

/// A function whose body an incremental driver reuses (T13 step 2): the
/// checker skips its body and assumes these effect facts, taken from the
/// previous check of the same, unchanged function.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReusedBody {
    pub direct: EffectSet,
    pub edges: Vec<NodeId>,
    /// Imports the body used (`module index:display`), marked used again.
    pub used_imports: Vec<String>,
    /// The function was in `Facts::static_str_fns`.
    pub static_str: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// T13 step 2, set by an incremental driver **before** checking: bodies
    /// not to check again. Their facts are absent from the result, which is
    /// then good for diagnostics only (never for code generation).
    pub reuse_bodies: BTreeMap<NodeId, ReusedBody>,
    /// Effects a function's own body produces (without callees), per
    /// function: what a driver stores to reuse the body later.
    pub direct_effects: BTreeMap<NodeId, EffectSet>,
    /// Imports each checked function body used (`module index:display`).
    pub fn_used_imports: BTreeMap<NodeId, Vec<String>>,
    /// Function item id → user functions it calls (the effects phase's
    /// call graph; T19 uses it to link a contract to the tests reaching it).
    pub call_graph: BTreeMap<NodeId, Vec<NodeId>>,
    /// Type of every expression node id.
    pub expr_types: BTreeMap<NodeId, Type>,
    /// Type of every type syntax node id (resolved `Type`).
    pub type_nodes: BTreeMap<NodeId, Type>,
    /// Type of every pattern node id.
    pub pat_types: BTreeMap<NodeId, Type>,
    /// Resolution of every value-path node id (including type paths used as callees).
    pub resolutions: BTreeMap<NodeId, Resolution>,
    /// Callee of every call expression id.
    pub calls: BTreeMap<NodeId, CallTarget>,
    /// Target of every method-call expression id.
    pub methods: BTreeMap<NodeId, MethodTarget>,
    /// Signatures of every function, keyed by the function item id.
    pub fn_sigs: BTreeMap<NodeId, FnSig>,
    /// C symbol linked for each extern function (keyed by its item id).
    pub extern_symbols: BTreeMap<NodeId, String>,
    pub structs: BTreeMap<NodeId, StructInfo>,
    pub enums: BTreeMap<NodeId, EnumInfo>,
    pub consts: BTreeMap<NodeId, ConstInfo>,
    /// Composed effect set per function item id.
    pub effects: BTreeMap<NodeId, EffectSet>,
    /// Rust path of every item (`crate::m::f`), keyed by item id.
    pub item_paths: BTreeMap<NodeId, String>,
    /// Module path (Ravum names joined by `::`) per module node id.
    pub module_paths: BTreeMap<NodeId, String>,
    /// Functions whose return type is exactly `&str` and whose body certainly
    /// returns a `'static` text: a string literal, an `if`/`match`/block whose
    /// every branch (and tail) is static, or a call to another function already
    /// in this set. Computed by a fixpoint at the end of type checking.
    ///
    /// Soundness: a function is only added when *every* explicit `return` is
    /// static and, if it has a tail expression, that tail is static too. A
    /// recursive cycle without a static base is never added. This set is the
    /// only interprocedural provenance `own::is_static_ref` accepts; no builtin
    /// (notably `String::as_str`) is ever a member.
    pub static_str_fns: BTreeSet<NodeId>,
    /// `thread::spawn(f, arg)` call node -> the free function `f` it runs
    /// (RFC-E11-CONCURRENCY CONC-3.1). Its effects flow to the caller.
    pub spawn_targets: BTreeMap<NodeId, NodeId>,
    /// `async fn` items (RFC-E11-CONCURRENCY phase 2). Their `FnSig::ret` is
    /// `Future<T>`; the body is checked against `T`.
    pub async_fns: BTreeSet<NodeId>,
    /// Test functions in declaration order.
    pub test_fns: Vec<NodeId>,
    pub main_fn: Option<NodeId>,
    /// Ownership decisions recorded by `own.rs` (RFC-OWN-FACTS-1, step A).
    ///
    /// Nothing reads them yet: they are produced so a later backend can stop
    /// re-deriving moves/copies/borrows (`emit.rs`'s `MovePlanner`). Additive
    /// measurement of a decision the checker already takes; see
    /// `docs/rfc/RFC-OWN-FACTS.md`.
    pub own: OwnFacts,
}

#[derive(Debug, Clone)]
pub struct CheckedProgram {
    pub program: Program,
    pub facts: Facts,
    pub files: Vec<FileInfo>,
    pub profile: Profile,
}

#[derive(Debug, Clone)]
pub struct CheckOptions {
    pub profile: Profile,
    pub edition: String,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            profile: Profile::Core,
            edition: ravum_lang_syntax::LANGUAGE_EDITION.to_string(),
        }
    }
}

pub struct CheckResult {
    pub checked: Option<CheckedProgram>,
    pub diags: DiagnosticBag,
}

impl CheckResult {
    pub fn failed(diags: DiagnosticBag) -> Self {
        Self {
            checked: None,
            diags,
        }
    }
}

/// Convert a source map to the codegen-facing file table.
///
/// The backend indexes this table by `span.file` (`emit::location`), so a
/// session map with `add_at` gaps must keep the hole slots: a hole holds
/// `<unknown>` and is never referenced by a span. The common dense map (CLI,
/// tests) takes the historical fast path where position == id.
pub fn file_infos(sources: &ravum_lang_syntax::source::SourceMap) -> Vec<FileInfo> {
    let needed = sources
        .files()
        .last()
        .map_or(0, |file| file.id as usize + 1);
    if needed == sources.len() {
        return sources.files().iter().map(source_file_info).collect();
    }
    let mut infos: Vec<FileInfo> = (0..needed)
        .map(|_| FileInfo {
            path: "<unknown>".to_string(),
            line_starts: vec![0],
        })
        .collect();
    for file in sources.files() {
        // `needed` is `max(id) + 1`, so the slot selected for `file.id` exists.
        infos[file.id as usize] = source_file_info(file);
    }
    infos
}

fn source_file_info(f: &ravum_lang_syntax::span::SourceFile) -> FileInfo {
    let mut line_starts = vec![0u32];
    for (i, byte) in f.text.bytes().enumerate() {
        if byte == b'\n' {
            line_starts.push(i as u32 + 1);
        }
    }
    FileInfo {
        path: f.path.clone(),
        line_starts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ravum_lang_syntax::source::SourceMap;

    const ALL_EFFECTS: [Effect; 11] = [
        Effect::Io,
        Effect::Fs,
        Effect::Net,
        Effect::Thread,
        Effect::Alloc,
        Effect::Panic,
        Effect::Unsafe,
        Effect::Ffi,
        Effect::Async,
        Effect::Rt,
        Effect::Nondet,
    ];

    /// Effect names are part of the diagnostic and JSON contract (EFF-* notes,
    /// `check --json` effect sets). Renaming one silently would change every
    /// message that mentions effects, so the round trip is locked here.
    #[test]
    fn effect_names_round_trip() {
        for effect in ALL_EFFECTS {
            assert_eq!(
                Effect::from_name(effect.name()),
                Some(effect),
                "round trip failed for {effect:?}"
            );
        }
    }

    #[test]
    fn unknown_effect_names_are_refused() {
        for text in ["", " ", "IO", "Pure", "pure", "io ", "ffi,unsafe"] {
            assert_eq!(Effect::from_name(text), None, "accepted {text:?}");
        }
    }

    /// Every effect is producible: `net` since RFC-E11-NET-HTTP (D-18-2),
    /// `async` since RFC-E11-CONCURRENCY phase 2 (D-19-9).
    #[test]
    fn no_effect_is_unsupported() {
        for effect in ALL_EFFECTS {
            assert!(!effect.is_unsupported(), "{effect:?}");
        }
    }

    #[test]
    fn effect_set_is_ordered_and_displayed() {
        let empty = EffectSet::default();
        assert_eq!(empty.display(), "pure");
        assert!(empty.names().is_empty());

        let mut set = EffectSet::new([Effect::Ffi, Effect::Io]);
        assert_eq!(
            set.names(),
            vec!["io", "ffi"],
            "BTreeSet order, not insertion"
        );
        assert_eq!(set.display(), "io, ffi");
        assert!(set.contains(Effect::Ffi));
        assert!(!set.contains(Effect::Rt));

        set.insert(Effect::Rt);
        assert_eq!(set.display(), "io, ffi, rt");
    }

    #[test]
    fn effect_set_extend_and_difference() {
        let mut base = EffectSet::new([Effect::Io]);
        base.extend(&EffectSet::new([Effect::Io, Effect::Panic]));
        assert_eq!(base.names(), vec!["io", "panic"], "extend is a union");

        let declared = EffectSet::new([Effect::Panic]);
        assert_eq!(base.difference(&declared), vec![Effect::Io]);
        assert!(base.difference(&base).is_empty());
        assert_eq!(EffectSet::default().difference(&base), Vec::new());
    }

    /// Profile names appear in diagnostics (`profile \`pure\` forbids ...`) and
    /// on the command line; the parser is the only accepted spelling.
    #[test]
    fn profile_names_round_trip() {
        for (text, profile) in [
            ("core", Profile::Core),
            ("pure", Profile::Pure),
            ("rt", Profile::Rt),
        ] {
            assert_eq!(Profile::parse(text), Some(profile));
            assert_eq!(profile.name(), text);
        }
        for text in ["", "Core", "PURE", "safe"] {
            assert_eq!(Profile::parse(text), None, "accepted {text:?}");
        }
    }

    /// EFF-4.x: `core` restricts nothing, `pure` forbids every group, `rt`
    /// forbids every group but `nondet` (the point of the profile).
    #[test]
    fn profile_forbidden_sets() {
        assert_eq!(Profile::Core.forbidden(), None);

        let pure = Profile::Pure.forbidden().expect("pure forbids effects");
        for effect in ALL_EFFECTS {
            assert!(pure.contains(effect), "pure must forbid {effect:?}");
        }

        let rt = Profile::Rt.forbidden().expect("rt forbids effects");
        assert!(!rt.contains(Effect::Nondet), "rt allows nondet");
        assert!(rt.contains(Effect::Ffi), "rt still forbids ffi");
        assert!(rt.contains(Effect::Rt), "rt still forbids rt");
    }

    #[test]
    fn file_info_maps_offsets_to_one_based_lines() {
        let info = FileInfo {
            path: "src/main.rv".to_string(),
            line_starts: vec![0, 6, 11],
        };
        assert_eq!(info.line_of(0), 1);
        assert_eq!(info.line_of(5), 1);
        assert_eq!(
            info.line_of(6),
            2,
            "an offset at a line start belongs to it"
        );
        assert_eq!(info.line_of(10), 2);
        assert_eq!(info.line_of(11), 3);
        assert_eq!(
            info.line_of(4096),
            3,
            "past the end clamps to the last line"
        );
    }

    #[test]
    fn file_infos_derives_line_starts_from_the_source_map() {
        let mut sources = SourceMap::new();
        sources.add("src/main.rv", "fn main() {\n}\n");
        let infos = file_infos(&sources);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].path, "src/main.rv");
        assert_eq!(
            infos[0].line_starts,
            vec![0, 12, 14],
            "the trailing newline opens an empty third line"
        );
        assert_eq!(infos[0].line_of(12), 2);
        assert_eq!(infos[0].line_of(14), 3);
    }

    #[test]
    fn file_infos_are_indexed_by_file_id_when_the_source_map_has_gaps() {
        let mut sources = SourceMap::new();
        sources.add_at(2, "src/late.rv", "fn main() {\n}\n");
        let infos = file_infos(&sources);
        assert_eq!(infos.len(), 3, "the backend indexes the table by FileId");
        assert_eq!(infos[2].path, "src/late.rv");
        assert_eq!(infos[2].line_of(12), 2);
        assert_eq!(infos[0].path, "<unknown>", "a hole has no source file");
    }
}
