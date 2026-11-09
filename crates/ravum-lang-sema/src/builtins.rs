//! Table des builtins Ravum Core 1.0 (prélude + modules std).
//!
//! **Source de vérité unique** pour les signatures et les effets des fonctions,
//! fonctions associées et méthodes fournies par le runtime. Toute méthode ou
//! fonction absente de cette table est `E-RES-UNKNOWN` (le checker ne devine
//! jamais une signature).
//!
//! Conventions de chemins (contrat public avec le codegen) :
//! * fonction libre : chemin canonique complet, `std::io::println`,
//!   `std::panic::panic`, `std::math::sqrt`, `drop` ;
//! * fonction associée : `Type::nom`, `String::from`, `Vec::new`, `Error::new` ;
//! * méthode : `famille::nom` où famille ∈ `String`, `str`, `slice`, `Vec`,
//!   `Option`, `Result`, `int`, `f64`, `bool`, `char` (jamais le type concret
//!   pour les primitifs, afin que le codegen ait une clé stable).
//!
//! Le type prélude `Error` n'a pas de `NodeId` d'item : il est représenté par
//! `Type::Nominal { def: PRELUDE_ERROR_DEF, name: "Error", .. }` (constante
//! réservée, aucun item utilisateur ne peut porter cet id).
//!
//! Limite documentée : `assert`/`assert_eq`/`unwrap` déclarent l'effet `panic`
//! de façon conservatrice (ils ne paniquent pas toujours). Les panics
//! **intrinsèques** (débordement, division par zéro, index hors bornes) ne sont
//! pas comptés dans l'effet `panic` : c'est une approximation assumée, cohérente
//! avec la spec d'effets (traps non déclarés).

use crate::facts::Effect;
use crate::types::Type;
use ravum_lang_syntax::ast::NodeId;

/// `NodeId` réservé au type prélude `Error` (voir doc du module).
pub const PRELUDE_ERROR_DEF: NodeId = NodeId::MAX;

/// Type prélude `Error` (`struct Error { pub message: String }`).
pub fn error_type() -> Type {
    Type::Nominal {
        def: PRELUDE_ERROR_DEF,
        name: "Error".to_string(),
        args: Vec::new(),
    }
}

/// Vrai si `ty` est le type prélude `Error`.
pub fn is_error_type(ty: &Type) -> bool {
    matches!(ty, Type::Nominal { def, .. } if *def == PRELUDE_ERROR_DEF)
}

/// Noms de types réservés par le prélude (collision = `E-RES-CONFLICT`).
pub const PRELUDE_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "isize", "usize", "f32",
    "f64", "bool", "char", "String", "Vec", "Option", "Result", "Error",
];

/// Noms de valeurs réservés par le prélude (collision = `E-RES-CONFLICT`).
pub const PRELUDE_VALUES: &[&str] = &[
    "panic",
    "assert",
    "assert_eq",
    "drop",
    "Some",
    "None",
    "Ok",
    "Err",
];

/// Modules accessibles sans `use` (chemins canoniques).
pub const PRELUDE_MODULES: &[&str] = &["io", "fs", "string", "math", "env", "process", "std"];

/// Module builtin canonique (`std::io`, ...) pour un nom racine non qualifié.
pub fn builtin_root_module(name: &str) -> Option<&'static str> {
    Some(match name {
        "io" => "std::io",
        "fs" => "std::fs",
        "string" => "std::string",
        "math" => "std::math",
        "env" => "std::env",
        "process" => "std::process",
        "std" => "std",
        _ => return None,
    })
}

/// Tous les modules builtin canoniques connus.
pub const BUILTIN_MODULES: &[&str] = &[
    "std",
    "std::io",
    "std::fs",
    "std::string",
    "std::math",
    "std::env",
    "std::process",
    "std::panic",
    "std::mem",
];

/// Sous-module canonique de `parent` (ou `None`).
pub fn builtin_child_module(parent: &str, name: &str) -> Option<&'static str> {
    let candidate = format!("{parent}::{name}");
    BUILTIN_MODULES.iter().copied().find(|m| *m == candidate)
}

/// Vrai si `path` désigne un module builtin.
pub fn is_builtin_module(path: &str) -> bool {
    BUILTIN_MODULES.contains(&path)
}

/// Signature statique d'une fonction libre builtin.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinFnSig {
    pub path: &'static str,
    /// Paramètres de type nus : `["T"]` → `T` est un `Type::Param { index }`.
    pub generics: &'static [&'static str],
    pub params: &'static [&'static str],
    pub ret: &'static str,
    pub effects: &'static [Effect],
}

const IO: &[Effect] = &[Effect::Io];
const IO_ALLOC: &[Effect] = &[Effect::Io, Effect::Alloc];
const FS: &[Effect] = &[Effect::Fs];
const FS_ALLOC: &[Effect] = &[Effect::Fs, Effect::Alloc];
const ALLOC: &[Effect] = &[Effect::Alloc];
const PANIC: &[Effect] = &[Effect::Panic];
const PURE: &[Effect] = &[];

/// Table des fonctions libres builtin (ordre stable, lookup linéaire borné).
pub static BUILTIN_FNS: &[BuiltinFnSig] = &[
    BuiltinFnSig {
        path: "std::io::println",
        generics: &[],
        params: &["&str"],
        ret: "()",
        effects: IO,
    },
    BuiltinFnSig {
        path: "std::io::print",
        generics: &[],
        params: &["&str"],
        ret: "()",
        effects: IO,
    },
    BuiltinFnSig {
        path: "std::io::eprintln",
        generics: &[],
        params: &["&str"],
        ret: "()",
        effects: IO,
    },
    BuiltinFnSig {
        path: "std::io::read_line",
        generics: &[],
        params: &[],
        ret: "Result<String, Error>",
        effects: IO_ALLOC,
    },
    BuiltinFnSig {
        path: "std::env::args",
        generics: &[],
        params: &[],
        ret: "Vec<String>",
        effects: IO_ALLOC,
    },
    BuiltinFnSig {
        path: "std::env::var",
        generics: &[],
        params: &["&str"],
        ret: "Result<String, Error>",
        effects: IO_ALLOC,
    },
    BuiltinFnSig {
        path: "std::fs::read_to_string",
        generics: &[],
        params: &["&str"],
        ret: "Result<String, Error>",
        effects: FS_ALLOC,
    },
    BuiltinFnSig {
        path: "std::fs::write_string",
        generics: &[],
        params: &["&str", "&str"],
        ret: "Result<(), Error>",
        effects: FS,
    },
    BuiltinFnSig {
        path: "std::fs::exists",
        generics: &[],
        params: &["&str"],
        ret: "bool",
        effects: FS,
    },
    BuiltinFnSig {
        path: "std::process::exit",
        generics: &[],
        params: &["i32"],
        ret: "!",
        effects: IO,
    },
    BuiltinFnSig {
        path: "std::panic::panic",
        generics: &[],
        params: &["&str"],
        ret: "!",
        effects: PANIC,
    },
    BuiltinFnSig {
        path: "std::panic::assert",
        generics: &[],
        params: &["bool"],
        ret: "()",
        effects: PANIC,
    },
    BuiltinFnSig {
        path: "std::panic::assert_eq",
        generics: &["T"],
        params: &["T", "T"],
        ret: "()",
        effects: PANIC,
    },
    BuiltinFnSig {
        path: "std::string::parse_i64",
        generics: &[],
        params: &["&str"],
        ret: "Result<i64, Error>",
        effects: ALLOC,
    },
    BuiltinFnSig {
        path: "std::string::parse_f64",
        generics: &[],
        params: &["&str"],
        ret: "Result<f64, Error>",
        effects: ALLOC,
    },
    BuiltinFnSig {
        path: "std::string::from",
        generics: &[],
        params: &["&str"],
        ret: "String",
        effects: ALLOC,
    },
    BuiltinFnSig {
        path: "std::math::abs",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::sqrt",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::floor",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::ceil",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::round",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::pow",
        generics: &[],
        params: &["f64", "f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::min",
        generics: &[],
        params: &["f64", "f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::max",
        generics: &[],
        params: &["f64", "f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::sin",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::cos",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::tan",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::exp",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::ln",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::math::log10",
        generics: &[],
        params: &["f64"],
        ret: "f64",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "Error::new",
        generics: &[],
        params: &["&str"],
        ret: "Error",
        effects: ALLOC,
    },
    BuiltinFnSig {
        path: "drop",
        generics: &["T"],
        params: &["T"],
        ret: "()",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::mem::drop",
        generics: &["T"],
        params: &["T"],
        ret: "()",
        effects: PURE,
    },
];

/// Lookup d'une fonction libre builtin par chemin canonique.
pub fn builtin_fn(path: &str) -> Option<BuiltinFnSig> {
    BUILTIN_FNS.iter().copied().find(|f| f.path == path)
}

/// Chemins canoniques des fonctions du prélude accessibles sans qualification.
pub fn prelude_value_path(name: &str) -> Option<&'static str> {
    Some(match name {
        "panic" => "std::panic::panic",
        "assert" => "std::panic::assert",
        "assert_eq" => "std::panic::assert_eq",
        "drop" => "drop",
        _ => return None,
    })
}

/// Type prélude correspondant à un nom (`String`, `Vec`, primitifs, `Error`).
pub fn prelude_type(name: &str) -> Option<Type> {
    Some(match name {
        "i8" => Type::I8,
        "i16" => Type::I16,
        "i32" => Type::I32,
        "i64" => Type::I64,
        "i128" => Type::I128,
        "u8" => Type::U8,
        "u16" => Type::U16,
        "u32" => Type::U32,
        "u64" => Type::U64,
        "u128" => Type::U128,
        "isize" => Type::Isize,
        "usize" => Type::Usize,
        "f32" => Type::F32,
        "f64" => Type::F64,
        "bool" => Type::Bool,
        "char" => Type::Char,
        "String" => Type::String,
        "Error" => return Some(error_type()),
        _ => return None,
    })
}

/// Vrai si le nom est réservé par le prélude dans l'espace de noms des types.
pub fn is_prelude_type_name(name: &str) -> bool {
    PRELUDE_TYPES.contains(&name)
}

/// Vrai si le nom est réservé par le prélude dans l'espace de noms des valeurs.
pub fn is_prelude_value_name(name: &str) -> bool {
    PRELUDE_VALUES.contains(&name)
}

/// Signature statique d'une fonction associée builtin (`Vec::new`, ...).
#[derive(Debug, Clone, Copy)]
pub struct BuiltinAssocFn {
    pub path: &'static str,
    pub generics: &'static [&'static str],
    pub params: &'static [&'static str],
    pub ret: &'static str,
    pub effects: &'static [Effect],
}

pub static BUILTIN_ASSOC_FNS: &[BuiltinAssocFn] = &[
    BuiltinAssocFn {
        path: "String::from",
        generics: &[],
        params: &["&str"],
        ret: "String",
        effects: ALLOC,
    },
    BuiltinAssocFn {
        path: "String::new",
        generics: &[],
        params: &[],
        ret: "String",
        effects: ALLOC,
    },
    BuiltinAssocFn {
        path: "Vec::new",
        generics: &["T"],
        params: &[],
        ret: "Vec<T>",
        effects: ALLOC,
    },
    BuiltinAssocFn {
        path: "Vec::with_capacity",
        generics: &["T"],
        params: &["usize"],
        ret: "Vec<T>",
        effects: ALLOC,
    },
    BuiltinAssocFn {
        path: "Error::new",
        generics: &[],
        params: &["&str"],
        ret: "Error",
        effects: ALLOC,
    },
];

/// Lookup d'une fonction associée builtin.
pub fn builtin_assoc_fn(path: &str) -> Option<BuiltinAssocFn> {
    BUILTIN_ASSOC_FNS.iter().copied().find(|f| f.path == path)
}

/// Famille de type pour la résolution des méthodes builtin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodSelfKind {
    String,
    StrRef,
    Slice,
    Vec,
    Option,
    Result,
    /// Entier signé ou non signé (les entrées marquées `Int` s'appliquent aux deux).
    Int,
    SignedInt,
    F64,
    F32,
    Bool,
    Char,
}

/// Signature statique d'une méthode builtin.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinMethodSig {
    pub kinds: &'static [MethodSelfKind],
    pub name: &'static str,
    /// Chemin canonique stable pour le codegen/effets.
    pub path: &'static str,
    pub params: &'static [&'static str],
    pub ret: &'static str,
    pub effects: &'static [Effect],
}

macro_rules! method {
    ($kinds:expr, $name:literal, $path:literal, [$($p:literal),*], $ret:literal, $eff:expr) => {
        BuiltinMethodSig {
            kinds: $kinds,
            name: $name,
            path: $path,
            params: &[$($p),*],
            ret: $ret,
            effects: $eff,
        }
    };
}

const STRING_KINDS: &[MethodSelfKind] = &[MethodSelfKind::String];
const STR_KINDS: &[MethodSelfKind] = &[MethodSelfKind::StrRef];
const VEC_KINDS: &[MethodSelfKind] = &[MethodSelfKind::Vec];
const SLICE_KINDS: &[MethodSelfKind] = &[MethodSelfKind::Slice];
const OPTION_KINDS: &[MethodSelfKind] = &[MethodSelfKind::Option];
const RESULT_KINDS: &[MethodSelfKind] = &[MethodSelfKind::Result];
const INT_KINDS: &[MethodSelfKind] = &[MethodSelfKind::Int, MethodSelfKind::SignedInt];
const SIGNED_KINDS: &[MethodSelfKind] = &[MethodSelfKind::SignedInt];
const F64_KINDS: &[MethodSelfKind] = &[MethodSelfKind::F64];
const FLOAT_KINDS: &[MethodSelfKind] = &[MethodSelfKind::F64, MethodSelfKind::F32];
const BOOL_KINDS: &[MethodSelfKind] = &[MethodSelfKind::Bool];
const CHAR_KINDS: &[MethodSelfKind] = &[MethodSelfKind::Char];

/// Table des méthodes builtin (familles `String`, `str`, `slice`, `Vec`,
/// `Option`, `Result`, `int`, `f64`, primitifs).
pub static BUILTIN_METHODS: &[BuiltinMethodSig] = &[
    // --- String ---
    method!(STRING_KINDS, "len", "String::len", [], "usize", PURE),
    method!(
        STRING_KINDS,
        "is_empty",
        "String::is_empty",
        [],
        "bool",
        PURE
    ),
    method!(STRING_KINDS, "as_str", "String::as_str", [], "&str", PURE),
    method!(
        STRING_KINDS,
        "push_str",
        "String::push_str",
        ["&str"],
        "()",
        ALLOC
    ),
    method!(STRING_KINDS, "push", "String::push", ["char"], "()", ALLOC),
    method!(STRING_KINDS, "clone", "String::clone", [], "String", ALLOC),
    // `trim`, `to_uppercase`, `to_lowercase` renvoient des valeurs possédées.
    method!(STRING_KINDS, "trim", "String::trim", [], "String", ALLOC),
    method!(
        STRING_KINDS,
        "to_uppercase",
        "String::to_uppercase",
        [],
        "String",
        ALLOC
    ),
    method!(
        STRING_KINDS,
        "to_lowercase",
        "String::to_lowercase",
        [],
        "String",
        ALLOC
    ),
    method!(
        STRING_KINDS,
        "contains",
        "String::contains",
        ["&str"],
        "bool",
        PURE
    ),
    method!(
        STRING_KINDS,
        "replace",
        "String::replace",
        ["&str", "&str"],
        "String",
        ALLOC
    ),
    method!(
        STRING_KINDS,
        "lines",
        "String::lines",
        [],
        "Vec<String>",
        ALLOC
    ),
    method!(
        STRING_KINDS,
        "starts_with",
        "String::starts_with",
        ["&str"],
        "bool",
        PURE
    ),
    method!(
        STRING_KINDS,
        "ends_with",
        "String::ends_with",
        ["&str"],
        "bool",
        PURE
    ),
    method!(
        STRING_KINDS,
        "parse_i64",
        "String::parse_i64",
        [],
        "Result<i64, Error>",
        ALLOC
    ),
    method!(
        STRING_KINDS,
        "parse_f64",
        "String::parse_f64",
        [],
        "Result<f64, Error>",
        ALLOC
    ),
    // --- &str (lecture seule, valeurs possédées pour les transformations) ---
    method!(STR_KINDS, "len", "str::len", [], "usize", PURE),
    method!(STR_KINDS, "is_empty", "str::is_empty", [], "bool", PURE),
    method!(STR_KINDS, "as_str", "str::as_str", [], "&str", PURE),
    method!(STR_KINDS, "trim", "str::trim", [], "String", ALLOC),
    method!(
        STR_KINDS,
        "to_uppercase",
        "str::to_uppercase",
        [],
        "String",
        ALLOC
    ),
    method!(
        STR_KINDS,
        "to_lowercase",
        "str::to_lowercase",
        [],
        "String",
        ALLOC
    ),
    method!(
        STR_KINDS,
        "contains",
        "str::contains",
        ["&str"],
        "bool",
        PURE
    ),
    method!(
        STR_KINDS,
        "replace",
        "str::replace",
        ["&str", "&str"],
        "String",
        ALLOC
    ),
    method!(STR_KINDS, "lines", "str::lines", [], "Vec<String>", ALLOC),
    method!(
        STR_KINDS,
        "starts_with",
        "str::starts_with",
        ["&str"],
        "bool",
        PURE
    ),
    method!(
        STR_KINDS,
        "ends_with",
        "str::ends_with",
        ["&str"],
        "bool",
        PURE
    ),
    method!(
        STR_KINDS,
        "parse_i64",
        "str::parse_i64",
        [],
        "Result<i64, Error>",
        ALLOC
    ),
    method!(
        STR_KINDS,
        "parse_f64",
        "str::parse_f64",
        [],
        "Result<f64, Error>",
        ALLOC
    ),
    // --- &[T] ---
    method!(SLICE_KINDS, "len", "slice::len", [], "usize", PURE),
    method!(SLICE_KINDS, "is_empty", "slice::is_empty", [], "bool", PURE),
    // --- Vec<T> ---
    method!(VEC_KINDS, "len", "Vec::len", [], "usize", PURE),
    method!(VEC_KINDS, "is_empty", "Vec::is_empty", [], "bool", PURE),
    method!(VEC_KINDS, "push", "Vec::push", ["T"], "()", ALLOC),
    method!(VEC_KINDS, "pop", "Vec::pop", [], "Option<T>", PURE),
    method!(VEC_KINDS, "clear", "Vec::clear", [], "()", PURE),
    method!(VEC_KINDS, "clone", "Vec::clone", [], "Vec<T>", ALLOC),
    method!(VEC_KINDS, "as_slice", "Vec::as_slice", [], "&[T]", PURE),
    // --- Option<T> ---
    method!(OPTION_KINDS, "is_some", "Option::is_some", [], "bool", PURE),
    method!(OPTION_KINDS, "is_none", "Option::is_none", [], "bool", PURE),
    method!(OPTION_KINDS, "unwrap", "Option::unwrap", [], "T", PANIC),
    method!(
        OPTION_KINDS,
        "unwrap_or",
        "Option::unwrap_or",
        ["T"],
        "T",
        PURE
    ),
    method!(
        OPTION_KINDS,
        "ok_or",
        "Option::ok_or",
        ["Error"],
        "Result<T, Error>",
        PURE
    ),
    // --- Result<T, E> ---
    method!(RESULT_KINDS, "is_ok", "Result::is_ok", [], "bool", PURE),
    method!(RESULT_KINDS, "is_err", "Result::is_err", [], "bool", PURE),
    method!(RESULT_KINDS, "unwrap", "Result::unwrap", [], "T", PANIC),
    method!(
        RESULT_KINDS,
        "unwrap_or",
        "Result::unwrap_or",
        ["T"],
        "T",
        PURE
    ),
    method!(RESULT_KINDS, "ok", "Result::ok", [], "Option<T>", PURE),
    method!(RESULT_KINDS, "err", "Result::err", [], "Option<E>", PURE),
    // --- primitifs ---
    method!(
        INT_KINDS,
        "to_string",
        "int::to_string",
        [],
        "String",
        ALLOC
    ),
    method!(SIGNED_KINDS, "abs", "int::abs", [], "Self", PURE),
    method!(
        FLOAT_KINDS,
        "to_string",
        "float::to_string",
        [],
        "String",
        ALLOC
    ),
    method!(F64_KINDS, "abs", "f64::abs", [], "f64", PURE),
    method!(F64_KINDS, "sqrt", "f64::sqrt", [], "f64", PURE),
    method!(F64_KINDS, "floor", "f64::floor", [], "f64", PURE),
    method!(F64_KINDS, "ceil", "f64::ceil", [], "f64", PURE),
    method!(F64_KINDS, "round", "f64::round", [], "f64", PURE),
    method!(F64_KINDS, "powf", "f64::powf", ["f64"], "f64", PURE),
    method!(F64_KINDS, "min", "f64::min", ["f64"], "f64", PURE),
    method!(F64_KINDS, "max", "f64::max", ["f64"], "f64", PURE),
    method!(
        BOOL_KINDS,
        "to_string",
        "bool::to_string",
        [],
        "String",
        ALLOC
    ),
    method!(
        CHAR_KINDS,
        "to_string",
        "char::to_string",
        [],
        "String",
        ALLOC
    ),
];

/// Lookup d'une méthode builtin par famille de receveur et nom.
pub fn builtin_method(kind: MethodSelfKind, name: &str) -> Option<&'static BuiltinMethodSig> {
    BUILTIN_METHODS
        .iter()
        .find(|m| m.name == name && m.kinds.iter().any(|k| kind_accepts(*k, kind)))
}

fn kind_accepts(entry: MethodSelfKind, receiver: MethodSelfKind) -> bool {
    if entry == receiver {
        return true;
    }
    // Une entrée `Int` (to_string) s'applique aux entiers signés.
    entry == MethodSelfKind::Int && receiver == MethodSelfKind::SignedInt
}

/// Effets d'une méthode builtin depuis son chemin canonique.
pub fn builtin_method_effects(path: &str) -> Option<&'static [Effect]> {
    BUILTIN_METHODS
        .iter()
        .find(|m| m.path == path)
        .map(|m| m.effects)
}

/// Effets d'une fonction associée builtin depuis son chemin canonique.
pub fn builtin_assoc_fn_effects(path: &str) -> Option<&'static [Effect]> {
    BUILTIN_ASSOC_FNS
        .iter()
        .find(|f| f.path == path)
        .map(|f| f.effects)
}

// ---------------------------------------------------------------------------
// Mini-analyseur de type (les signatures ci-dessus sont des chaînes statiques).
// ---------------------------------------------------------------------------

/// Résout une chaîne de type builtin en [`Type`], avec un environnement
/// `nom -> Type` (paramètres génériques et `Self`).
pub fn type_from_str(text: &str, env: &[(&str, Type)]) -> Option<Type> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if t == "()" {
        return Some(Type::Unit);
    }
    if t == "!" {
        return Some(Type::Never);
    }
    if let Some(rest) = t.strip_prefix("&mut ") {
        return Some(Type::Ref {
            mutable: true,
            inner: Box::new(type_from_str(rest, env)?),
        });
    }
    if let Some(rest) = t.strip_prefix('&') {
        return Some(Type::Ref {
            mutable: false,
            inner: Box::new(type_from_str(rest, env)?),
        });
    }
    if t.starts_with('[') && t.ends_with(']') {
        return Some(Type::Slice(Box::new(type_from_str(
            &t[1..t.len() - 1],
            env,
        )?)));
    }
    if t.starts_with('(') && t.ends_with(')') {
        let inner = &t[1..t.len() - 1];
        if inner.trim().is_empty() {
            return Some(Type::Unit);
        }
        let parts = split_top_level(inner, ',')?;
        let mut items = Vec::new();
        for part in parts {
            items.push(type_from_str(part, env)?);
        }
        return Some(Type::Tuple(items));
    }
    for (prefix, wrap) in [("Vec<", 0usize), ("Option<", 1usize), ("Result<", 2usize)] {
        if let Some(inner) = t.strip_prefix(prefix).and_then(|s| s.strip_suffix('>')) {
            let parts = split_top_level(inner, ',')?;
            let types: Option<Vec<Type>> = parts.iter().map(|p| type_from_str(p, env)).collect();
            let types = types?;
            return match wrap {
                0 if types.len() == 1 => Some(Type::Vec(Box::new(types.into_iter().next()?))),
                1 if types.len() == 1 => Some(Type::Option(Box::new(types.into_iter().next()?))),
                2 if types.len() == 2 => {
                    let mut it = types.into_iter();
                    Some(Type::Result(Box::new(it.next()?), Box::new(it.next()?)))
                }
                _ => None,
            };
        }
    }
    if let Some((_, ty)) = env.iter().find(|(name, _)| *name == t) {
        return Some(ty.clone());
    }
    if t == "str" {
        return Some(Type::Str);
    }
    prelude_type(t)
}

fn split_top_level(text: &str, sep: char) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, ch) in text.char_indices() {
        match ch {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(&text[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    parts.push(&text[start..]);
    Some(parts)
}

fn env_for_generics(generics: &'static [&'static str]) -> Vec<(&'static str, Type)> {
    generics
        .iter()
        .enumerate()
        .map(|(index, name)| {
            (
                *name,
                Type::Param {
                    name: (*name).to_string(),
                    index,
                },
            )
        })
        .collect()
}

/// Types des paramètres d'une fonction libre builtin (paramètres génériques
/// représentés par [`Type::Param`]).
pub fn fn_param_types(sig: &BuiltinFnSig) -> Vec<Type> {
    let env = env_for_generics(sig.generics);
    sig.params
        .iter()
        .map(|p| type_from_str(p, &env).unwrap_or(Type::Error))
        .collect()
}

/// Type de retour d'une fonction libre builtin.
pub fn fn_ret_type(sig: &BuiltinFnSig) -> Type {
    let env = env_for_generics(sig.generics);
    type_from_str(sig.ret, &env).unwrap_or(Type::Error)
}

/// Types des paramètres d'une fonction associée builtin.
pub fn assoc_param_types(sig: &BuiltinAssocFn) -> Vec<Type> {
    let env = env_for_generics(sig.generics);
    sig.params
        .iter()
        .map(|p| type_from_str(p, &env).unwrap_or(Type::Error))
        .collect()
}

/// Type de retour d'une fonction associée builtin.
pub fn assoc_ret_type(sig: &BuiltinAssocFn) -> Type {
    let env = env_for_generics(sig.generics);
    type_from_str(sig.ret, &env).unwrap_or(Type::Error)
}

/// Environnement `Self` + paramètres génériques du receveur.
pub fn method_env<'g>(self_ty: &Type, generics: &'g [(&'g str, Type)]) -> Vec<(&'g str, Type)> {
    let mut env: Vec<(&str, Type)> = Vec::with_capacity(generics.len() + 1);
    env.push(("Self", self_ty.clone()));
    env.extend(generics.iter().map(|(n, t)| (*n, t.clone())));
    env
}

/// Types des paramètres d'une méthode builtin pour un receveur donné.
pub fn method_param_types(sig: &BuiltinMethodSig, env: &[(&str, Type)]) -> Vec<Type> {
    sig.params
        .iter()
        .map(|p| type_from_str(p, env).unwrap_or(Type::Error))
        .collect()
}

/// Type de retour d'une méthode builtin pour un receveur donné.
pub fn method_ret_type(sig: &BuiltinMethodSig, env: &[(&str, Type)]) -> Type {
    type_from_str(sig.ret, env).unwrap_or(Type::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_fn_parses() {
        for sig in BUILTIN_FNS {
            assert!(
                fn_ret_type(sig) != Type::Error,
                "retour illisible: {}",
                sig.path
            );
            for ty in fn_param_types(sig) {
                assert!(!ty.is_error(), "paramètre illisible: {}", sig.path);
            }
        }
        for sig in BUILTIN_ASSOC_FNS {
            assert!(
                !assoc_ret_type(sig).is_error(),
                "retour illisible: {}",
                sig.path
            );
            for ty in assoc_param_types(sig) {
                assert!(!ty.is_error(), "paramètre illisible: {}", sig.path);
            }
        }
    }

    #[test]
    fn effects_are_declared_for_fallible_builtins() {
        assert_eq!(builtin_fn("std::io::println").unwrap().effects, IO);
        assert_eq!(builtin_fn("std::fs::exists").unwrap().effects, FS);
        assert_eq!(builtin_fn("std::panic::panic").unwrap().effects, PANIC);
        assert!(builtin_fn("std::io::println").is_some());
        assert!(builtin_fn("std::io::nope").is_none());
    }

    #[test]
    fn text_methods_return_owned_values() {
        let ty = type_from_str("String", &[]).unwrap();
        let env = method_env(&ty, &[]);
        let trim = builtin_method(MethodSelfKind::String, "trim").unwrap();
        assert_eq!(method_ret_type(trim, &env), Type::String);
        let as_str = builtin_method(MethodSelfKind::String, "as_str").unwrap();
        assert_eq!(method_ret_type(as_str, &env), Type::str_ref());
    }

    #[test]
    fn generic_method_and_fn_types_substitute() {
        let vec_ty = Type::Vec(Box::new(Type::I32));
        let env = method_env(&vec_ty, &[("T", Type::I32)]);
        let pop = builtin_method(MethodSelfKind::Vec, "pop").unwrap();
        assert_eq!(
            method_ret_type(pop, &env),
            Type::Option(Box::new(Type::I32))
        );
        let assert_eq = builtin_fn("std::panic::assert_eq").unwrap();
        assert_eq!(
            fn_param_types(&assert_eq),
            vec![
                Type::Param {
                    name: "T".to_string(),
                    index: 0
                },
                Type::Param {
                    name: "T".to_string(),
                    index: 0
                }
            ]
        );
        assert_eq!(fn_ret_type(&assert_eq), Type::Unit);
    }

    #[test]
    fn error_type_is_reserved_and_distinct() {
        assert!(is_error_type(&error_type()));
        assert!(!is_error_type(&Type::String));
        assert_eq!(error_type().to_string(), "Error");
    }
}
