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

/// `NodeId` réservés aux types opaques de `std::net` (RFC-E11-NET-HTTP).
pub const PRELUDE_TCP_LISTENER_DEF: NodeId = NodeId::MAX - 1;
pub const PRELUDE_TCP_STREAM_DEF: NodeId = NodeId::MAX - 2;

/// `NodeId` réservés aux types opaques génériques de `std::thread` et
/// `std::sync` (RFC-E11-CONCURRENCY §3). Chacun prend un argument de type.
pub const PRELUDE_JOIN_HANDLE_DEF: NodeId = NodeId::MAX - 3;
pub const PRELUDE_CHANNEL_DEF: NodeId = NodeId::MAX - 4;
pub const PRELUDE_SENDER_DEF: NodeId = NodeId::MAX - 5;
pub const PRELUDE_RECEIVER_DEF: NodeId = NodeId::MAX - 6;

/// `NodeId` réservé au handle C opaque `CHandle` (RFC-E11-SQLITE NAT-3.3) :
/// un pointeur C que le code Ravum ne peut ni lire ni déréférencer, seulement
/// recevoir d'une fonction `extern`, rendre à une autre, et comparer à nul.
/// C'est le seul type opaque `Copy` du prélude.
pub const PRELUDE_CHANDLE_DEF: NodeId = NodeId::MAX - 7;

/// `NodeId` réservés aux types opaques de `std::http` (RFC-E11-NET-HTTP §4).
pub const PRELUDE_HTTP_REQUEST_DEF: NodeId = NodeId::MAX - 8;
pub const PRELUDE_HTTP_RESPONSE_DEF: NodeId = NodeId::MAX - 9;

/// `NodeId` réservé au type opaque générique `Future<T>` (RFC-E11-CONCURRENCY
/// phase 2) : la valeur rendue par l'appel d'une `async fn`.
pub const PRELUDE_FUTURE_DEF: NodeId = NodeId::MAX - 10;

/// `NodeId` réservé au type prélude `Map<K, V>` (RFC-UI-LANG-1 P3,
/// STD-20) : une table ordonnée par l'ordre total des clés. Deux arguments
/// de type ; `K` est un entier, `bool`, `char` ou `String`.
pub const PRELUDE_MAP_DEF: NodeId = NodeId::MAX - 11;

/// Le type `Map<K, V>`.
pub fn map_type(k: Type, v: Type) -> Type {
    Type::Nominal {
        def: PRELUDE_MAP_DEF,
        name: "Map".to_string(),
        args: vec![k, v],
    }
}

/// STD-20.1 : types de clés admis (ordre total défini par la spec), plus
/// `Handle<T>` (HDL-9, RFC-UI-LANG-2, UI-D20).
pub fn is_map_key_type(t: &Type) -> bool {
    t.is_int() || matches!(t, Type::Bool | Type::Char | Type::String) || is_handle_type(t)
}

/// `NodeId` réservé au type prélude `Arena<T>` (RFC-UI-LANG-2, STD-21).
pub const PRELUDE_ARENA_DEF: NodeId = NodeId::MAX - 12;

/// `NodeId` réservé au type prélude `Handle<T>` (RFC-UI-LANG-2, STD-21) :
/// valeur `Copy` opaque (arène, place, génération), jamais une référence.
pub const PRELUDE_HANDLE_DEF: NodeId = NodeId::MAX - 13;

/// OWN-6.8 : méthodes qui rendent une **vue d'instruction** `Option<&V>` /
/// `Option<&mut V>` (STD-20.3, HDL-6).
pub fn is_view_method(path: &str) -> bool {
    matches!(
        path,
        "Map::get" | "Map::get_mut" | "Arena::get" | "Arena::get_mut"
    )
}

/// Vue exclusive (`Option<&mut V>`) : la table ou l'arène est empruntée en
/// exclusivité pendant le bras.
pub fn is_exclusive_view_method(path: &str) -> bool {
    matches!(path, "Map::get_mut" | "Arena::get_mut")
}

/// Le type `Arena<T>`.
pub fn arena_type(t: Type) -> Type {
    Type::Nominal {
        def: PRELUDE_ARENA_DEF,
        name: "Arena".to_string(),
        args: vec![t],
    }
}

/// Le type `Handle<T>`.
pub fn handle_type(t: Type) -> Type {
    Type::Nominal {
        def: PRELUDE_HANDLE_DEF,
        name: "Handle".to_string(),
        args: vec![t],
    }
}

pub fn is_handle_type(t: &Type) -> bool {
    matches!(t, Type::Nominal { def, .. } if *def == PRELUDE_HANDLE_DEF)
}

pub fn is_arena_type(t: &Type) -> bool {
    matches!(t, Type::Nominal { def, .. } if *def == PRELUDE_ARENA_DEF)
}

/// Le type `CHandle`.
pub fn chandle_type() -> Type {
    opaque_type(PRELUDE_CHANDLE_DEF, "CHandle")
}

/// Types opaques génériques du prélude : nom et `NodeId` réservé.
pub const PRELUDE_GENERIC_OPAQUES: &[(&str, NodeId)] = &[
    ("JoinHandle", PRELUDE_JOIN_HANDLE_DEF),
    ("Channel", PRELUDE_CHANNEL_DEF),
    ("Sender", PRELUDE_SENDER_DEF),
    ("Receiver", PRELUDE_RECEIVER_DEF),
    ("Future", PRELUDE_FUTURE_DEF),
];

/// Nom Rust du prélude pour un `NodeId` réservé à un type opaque.
pub fn prelude_opaque_name(def: NodeId) -> Option<&'static str> {
    if def == PRELUDE_TCP_LISTENER_DEF {
        return Some("TcpListener");
    }
    if def == PRELUDE_TCP_STREAM_DEF {
        return Some("TcpStream");
    }
    if def == PRELUDE_CHANDLE_DEF {
        return Some("CHandle");
    }
    if def == PRELUDE_HTTP_REQUEST_DEF {
        return Some("HttpRequest");
    }
    if def == PRELUDE_HTTP_RESPONSE_DEF {
        return Some("HttpResponse");
    }
    if def == PRELUDE_MAP_DEF {
        return Some("Map");
    }
    if def == PRELUDE_ARENA_DEF {
        return Some("Arena");
    }
    if def == PRELUDE_HANDLE_DEF {
        return Some("Handle");
    }
    PRELUDE_GENERIC_OPAQUES
        .iter()
        .find(|(_, id)| *id == def)
        .map(|(name, _)| *name)
}

/// Builtins typés par une règle dédiée du checker, pas par leur ligne de
/// table : leur signature décrit la forme, elle n'est pas analysable comme un
/// type. `thread::spawn` prend une **fonction nommée** (STD-18.2), et Ravum n'a
/// pas de type fonction.
pub const SPECIAL_FORM_FNS: &[&str] = &["std::thread::spawn"];

/// Type opaque générique du prélude (`JoinHandle<T>`, `Sender<T>`, ...).
pub fn generic_opaque(name: &str, arg: Type) -> Option<Type> {
    let (name, def) = PRELUDE_GENERIC_OPAQUES.iter().find(|(n, _)| *n == name)?;
    Some(Type::Nominal {
        def: *def,
        name: (*name).to_string(),
        args: vec![arg],
    })
}

fn opaque_type(def: NodeId, name: &str) -> Type {
    Type::Nominal {
        def,
        name: name.to_string(),
        args: Vec::new(),
    }
}

/// Vrai si `ty` est le type prélude `Error`.
pub fn is_error_type(ty: &Type) -> bool {
    matches!(ty, Type::Nominal { def, .. } if *def == PRELUDE_ERROR_DEF)
}

/// Noms de types réservés par le prélude (collision = `E-RES-CONFLICT`).
pub const PRELUDE_TYPES: &[&str] = &[
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "isize",
    "usize",
    "f32",
    "f64",
    "bool",
    "char",
    "String",
    "Vec",
    "Option",
    "Result",
    "Error",
    "TcpListener",
    "TcpStream",
    "JoinHandle",
    "Channel",
    "Sender",
    "Receiver",
    "CHandle",
    "HttpRequest",
    "HttpResponse",
    "Future",
    "Map",
    "Arena",
    "Handle",
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
pub const PRELUDE_MODULES: &[&str] = &[
    "io", "fs", "net", "thread", "sync", "task", "ffi", "http", "string", "math", "env", "process",
    "std",
];

/// Module builtin canonique (`std::io`, ...) pour un nom racine non qualifié.
pub fn builtin_root_module(name: &str) -> Option<&'static str> {
    Some(match name {
        "io" => "std::io",
        "fs" => "std::fs",
        "net" => "std::net",
        "thread" => "std::thread",
        "sync" => "std::sync",
        "task" => "std::task",
        "ffi" => "std::ffi",
        "http" => "std::http",
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
    "std::net",
    "std::thread",
    "std::sync",
    "std::ffi",
    "std::http",
    "std::task",
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
const NET: &[Effect] = &[Effect::Net];
const NET_ALLOC: &[Effect] = &[Effect::Net, Effect::Alloc];
const THREAD: &[Effect] = &[Effect::Thread];
const ASYNC: &[Effect] = &[Effect::Async];
const NET_ASYNC: &[Effect] = &[Effect::Net, Effect::Async, Effect::Alloc];
const THREAD_ALLOC: &[Effect] = &[Effect::Thread, Effect::Alloc];
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
    // --- std::net (RFC-E11-NET-HTTP §3) ---
    BuiltinFnSig {
        path: "std::net::listen",
        generics: &[],
        params: &["&str"],
        ret: "Result<TcpListener, Error>",
        effects: NET_ALLOC,
    },
    BuiltinFnSig {
        path: "std::net::local_port",
        generics: &[],
        params: &["&TcpListener"],
        ret: "u16",
        effects: NET,
    },
    BuiltinFnSig {
        path: "std::net::accept",
        generics: &[],
        params: &["&TcpListener"],
        ret: "Result<TcpStream, Error>",
        effects: NET_ALLOC,
    },
    BuiltinFnSig {
        path: "std::net::connect",
        generics: &[],
        params: &["&str"],
        ret: "Result<TcpStream, Error>",
        effects: NET_ALLOC,
    },
    BuiltinFnSig {
        path: "std::net::read_line",
        generics: &[],
        params: &["&mut TcpStream"],
        ret: "Result<Option<String>, Error>",
        effects: NET_ALLOC,
    },
    BuiltinFnSig {
        path: "std::net::read_exact",
        generics: &[],
        params: &["&mut TcpStream", "usize"],
        ret: "Result<String, Error>",
        effects: NET_ALLOC,
    },
    BuiltinFnSig {
        path: "std::net::write_str",
        generics: &[],
        params: &["&mut TcpStream", "&str"],
        ret: "Result<(), Error>",
        effects: NET_ALLOC,
    },
    // --- std::thread, std::sync (RFC-E11-CONCURRENCY §3) ---
    // `spawn` is typed by a dedicated rule (its first argument names a free
    // function, CONC-3.1); this row only registers the path and its effects.
    BuiltinFnSig {
        path: "std::thread::spawn",
        generics: &["A", "R"],
        params: &["fn(A) -> R", "A"],
        ret: "JoinHandle<R>",
        effects: THREAD_ALLOC,
    },
    BuiltinFnSig {
        path: "std::net::accept_async",
        generics: &[],
        params: &["&TcpListener"],
        ret: "Future<Result<TcpStream, Error>>",
        effects: NET_ASYNC,
    },
    BuiltinFnSig {
        path: "std::net::connect_async",
        generics: &[],
        params: &["&str"],
        ret: "Future<Result<TcpStream, Error>>",
        effects: NET_ASYNC,
    },
    BuiltinFnSig {
        path: "std::net::read_line_async",
        generics: &[],
        params: &["&mut TcpStream"],
        ret: "Future<Result<Option<String>, Error>>",
        effects: NET_ASYNC,
    },
    BuiltinFnSig {
        path: "std::net::write_str_async",
        generics: &[],
        params: &["&mut TcpStream", "&str"],
        ret: "Future<Result<(), Error>>",
        effects: NET_ASYNC,
    },
    BuiltinFnSig {
        path: "std::net::read_exact_async",
        generics: &[],
        params: &["&mut TcpStream", "usize"],
        ret: "Future<Result<String, Error>>",
        effects: NET_ASYNC,
    },
    BuiltinFnSig {
        path: "std::task::block_on",
        generics: &["T"],
        params: &["Future<T>"],
        ret: "T",
        effects: ASYNC,
    },
    BuiltinFnSig {
        path: "std::task::timeout",
        generics: &["T"],
        params: &["u64", "Future<T>"],
        ret: "Future<Option<T>>",
        effects: &[Effect::Async, Effect::Alloc, Effect::Nondet],
    },
    BuiltinFnSig {
        path: "std::task::spawn",
        generics: &[],
        params: &["Future<()>"],
        ret: "()",
        effects: ASYNC,
    },
    // RFC-ASYNC-NET-1 ANET-5: reads the monotonic clock, hence `nondet`
    // (RFC-DETERMINISTIC-PROFILE-1 DET-2: the first producer of the effect).
    BuiltinFnSig {
        path: "std::task::yield_now",
        generics: &[],
        params: &[],
        ret: "Future<()>",
        effects: ASYNC,
    },
    BuiltinFnSig {
        path: "std::thread::join",
        generics: &["T"],
        params: &["JoinHandle<T>"],
        ret: "T",
        effects: THREAD,
    },
    BuiltinFnSig {
        path: "std::sync::channel",
        generics: &["T"],
        params: &[],
        ret: "Channel<T>",
        effects: THREAD_ALLOC,
    },
    BuiltinFnSig {
        path: "std::sync::sender",
        generics: &["T"],
        params: &["&Channel<T>"],
        ret: "Sender<T>",
        effects: THREAD,
    },
    BuiltinFnSig {
        path: "std::sync::clone_sender",
        generics: &["T"],
        params: &["&Sender<T>"],
        ret: "Sender<T>",
        effects: THREAD,
    },
    BuiltinFnSig {
        path: "std::sync::receiver",
        generics: &["T"],
        params: &["Channel<T>"],
        ret: "Receiver<T>",
        effects: THREAD,
    },
    BuiltinFnSig {
        path: "std::sync::send",
        generics: &["T"],
        params: &["&Sender<T>", "T"],
        ret: "bool",
        effects: THREAD,
    },
    BuiltinFnSig {
        path: "std::sync::recv",
        generics: &["T"],
        params: &["&Receiver<T>"],
        ret: "Option<T>",
        effects: THREAD,
    },
    // --- std::http (RFC-E11-NET-HTTP §4) ---
    BuiltinFnSig {
        path: "std::http::read_request_async",
        generics: &[],
        params: &["&mut TcpStream"],
        ret: "Future<Result<HttpRequest, Error>>",
        effects: NET_ASYNC,
    },
    BuiltinFnSig {
        path: "std::http::respond_async",
        generics: &[],
        params: &["&mut TcpStream", "u16", "&str", "&str"],
        ret: "Future<Result<(), Error>>",
        effects: NET_ASYNC,
    },
    BuiltinFnSig {
        path: "std::http::read_request",
        generics: &[],
        params: &["&mut TcpStream"],
        ret: "Result<HttpRequest, Error>",
        effects: NET_ALLOC,
    },
    BuiltinFnSig {
        path: "std::http::method",
        generics: &[],
        params: &["&HttpRequest"],
        ret: "String",
        effects: ALLOC,
    },
    BuiltinFnSig {
        path: "std::http::path",
        generics: &[],
        params: &["&HttpRequest"],
        ret: "String",
        effects: ALLOC,
    },
    BuiltinFnSig {
        path: "std::http::header",
        generics: &[],
        params: &["&HttpRequest", "&str"],
        ret: "Option<String>",
        effects: ALLOC,
    },
    BuiltinFnSig {
        path: "std::http::body",
        generics: &[],
        params: &["&HttpRequest"],
        ret: "String",
        effects: ALLOC,
    },
    BuiltinFnSig {
        path: "std::http::respond",
        generics: &[],
        params: &["&mut TcpStream", "u16", "&str", "&str"],
        ret: "Result<(), Error>",
        effects: NET_ALLOC,
    },
    // RFC-ASYNC-NET-1 ANET-2: same semantics as the blocking forms.
    BuiltinFnSig {
        path: "std::http::get",
        generics: &[],
        params: &["&str"],
        ret: "Result<HttpResponse, Error>",
        effects: NET_ALLOC,
    },
    BuiltinFnSig {
        path: "std::http::post",
        generics: &[],
        params: &["&str", "&str", "&str"],
        ret: "Result<HttpResponse, Error>",
        effects: NET_ALLOC,
    },
    // ANET-2 / V2-N2: the whole request runs on a runtime helper thread.
    BuiltinFnSig {
        path: "std::http::get_async",
        generics: &[],
        params: &["&str"],
        ret: "Future<Result<HttpResponse, Error>>",
        effects: NET_ASYNC,
    },
    BuiltinFnSig {
        path: "std::http::post_async",
        generics: &[],
        params: &["&str", "&str", "&str"],
        ret: "Future<Result<HttpResponse, Error>>",
        effects: NET_ASYNC,
    },
    BuiltinFnSig {
        path: "std::http::status",
        generics: &[],
        params: &["&HttpResponse"],
        ret: "u16",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::http::response_body",
        generics: &[],
        params: &["&HttpResponse"],
        ret: "String",
        effects: ALLOC,
    },
    // --- std::ffi (RFC-E11-SQLITE NAT-3.3) ---
    BuiltinFnSig {
        path: "std::ffi::null",
        generics: &[],
        params: &[],
        ret: "CHandle",
        effects: PURE,
    },
    BuiltinFnSig {
        path: "std::ffi::is_null",
        generics: &[],
        params: &["CHandle"],
        ret: "bool",
        effects: PURE,
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
    // STD-7.7 (RFC-UI-LANG-1 P4): `join(parts, sep)` concatenates the parts
    // with `sep` between consecutive ones.
    BuiltinFnSig {
        path: "std::string::join",
        generics: &[],
        params: &["&Vec<String>", "&str"],
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

/// Signature of a builtin function path produced by the resolver. The
/// resolver builds such a path only from a `BUILTIN_FNS` entry or from
/// `prelude_value_path` (test `every_resolvable_builtin_function_path_has_a_signature`),
/// so a miss is a defect upstream.
pub fn resolved_builtin_fn(path: &str) -> BuiltinFnSig {
    builtin_fn(path)
        .unwrap_or_else(|| panic!("invariant: the resolved builtin path `{path}` has a signature"))
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
        "TcpListener" => return Some(opaque_type(PRELUDE_TCP_LISTENER_DEF, "TcpListener")),
        "TcpStream" => return Some(opaque_type(PRELUDE_TCP_STREAM_DEF, "TcpStream")),
        "CHandle" => return Some(chandle_type()),
        "HttpRequest" => return Some(opaque_type(PRELUDE_HTTP_REQUEST_DEF, "HttpRequest")),
        "HttpResponse" => return Some(opaque_type(PRELUDE_HTTP_RESPONSE_DEF, "HttpResponse")),
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
        path: "Arena::new",
        generics: &["T"],
        params: &[],
        ret: "Arena<T>",
        effects: ALLOC,
    },
    BuiltinAssocFn {
        path: "Map::new",
        generics: &["K", "V"],
        params: &[],
        ret: "Map<K, V>",
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
    Map,
    Arena,
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
/// Methods whose receiver is `&mut self` per `spec/language/stdlib.md`.
///
/// The signature table does not carry receiver mutability (it was added later),
/// so the closed list is stated once, here, and checked by the type checker:
/// calling one of these on an immutable place or a temporary is
/// `E-OWN-IMMUTABLE` (OWN-7.2/7.4).
pub fn builtin_method_takes_exclusive_receiver(kind: MethodSelfKind, name: &str) -> bool {
    match kind {
        MethodSelfKind::String => matches!(name, "push_str" | "push"),
        MethodSelfKind::Vec => matches!(name, "push" | "pop" | "clear"),
        MethodSelfKind::Map => matches!(name, "insert" | "remove" | "get_mut"),
        MethodSelfKind::Arena => matches!(name, "insert" | "remove" | "get_mut"),
        _ => false,
    }
}
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
const MAP_KINDS: &[MethodSelfKind] = &[MethodSelfKind::Map];
const ARENA_KINDS: &[MethodSelfKind] = &[MethodSelfKind::Arena];
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
    // STD-9.13..9.19 (RFC-UI-LANG-1 P4): positions are UTF-8 byte offsets.
    method!(
        STRING_KINDS,
        "find",
        "String::find",
        ["&str"],
        "Option<usize>",
        PURE
    ),
    method!(
        STRING_KINDS,
        "split",
        "String::split",
        ["&str"],
        "Vec<String>",
        ALLOC
    ),
    method!(
        STRING_KINDS,
        "strip_prefix",
        "String::strip_prefix",
        ["&str"],
        "Option<String>",
        ALLOC
    ),
    method!(
        STRING_KINDS,
        "strip_suffix",
        "String::strip_suffix",
        ["&str"],
        "Option<String>",
        ALLOC
    ),
    method!(
        STRING_KINDS,
        "substring",
        "String::substring",
        ["usize", "usize"],
        "String",
        ALLOC
    ),
    method!(
        STRING_KINDS,
        "chars",
        "String::chars",
        [],
        "Vec<char>",
        ALLOC
    ),
    method!(
        STRING_KINDS,
        "is_char_boundary",
        "String::is_char_boundary",
        ["usize"],
        "bool",
        PURE
    ),
    method!(
        STR_KINDS,
        "find",
        "str::find",
        ["&str"],
        "Option<usize>",
        PURE
    ),
    method!(
        STR_KINDS,
        "split",
        "str::split",
        ["&str"],
        "Vec<String>",
        ALLOC
    ),
    method!(
        STR_KINDS,
        "strip_prefix",
        "str::strip_prefix",
        ["&str"],
        "Option<String>",
        ALLOC
    ),
    method!(
        STR_KINDS,
        "strip_suffix",
        "str::strip_suffix",
        ["&str"],
        "Option<String>",
        ALLOC
    ),
    method!(
        STR_KINDS,
        "substring",
        "str::substring",
        ["usize", "usize"],
        "String",
        ALLOC
    ),
    method!(STR_KINDS, "chars", "str::chars", [], "Vec<char>", ALLOC),
    method!(
        STR_KINDS,
        "is_char_boundary",
        "str::is_char_boundary",
        ["usize"],
        "bool",
        PURE
    ),
    method!(VEC_KINDS, "len", "Vec::len", [], "usize", PURE),
    // STD-21 (RFC-UI-LANG-2): `Arena<T>`. `get` / `get_mut` rend une vue
    // d'instruction (HDL-6, OWN-6.8) ; un handle périmé ou étranger rend
    // `None` (HDL-2).
    method!(ARENA_KINDS, "len", "Arena::len", [], "usize", PURE),
    method!(ARENA_KINDS, "is_empty", "Arena::is_empty", [], "bool", PURE),
    method!(
        ARENA_KINDS,
        "insert",
        "Arena::insert",
        ["T"],
        "Handle<T>",
        ALLOC
    ),
    method!(
        ARENA_KINDS,
        "contains",
        "Arena::contains",
        ["Handle<T>"],
        "bool",
        PURE
    ),
    method!(
        ARENA_KINDS,
        "get",
        "Arena::get",
        ["Handle<T>"],
        "Option<&T>",
        PURE
    ),
    method!(
        ARENA_KINDS,
        "get_mut",
        "Arena::get_mut",
        ["Handle<T>"],
        "Option<&mut T>",
        PURE
    ),
    method!(
        ARENA_KINDS,
        "remove",
        "Arena::remove",
        ["Handle<T>"],
        "Option<T>",
        PURE
    ),
    // STD-20 (RFC-UI-LANG-1 P3): `Map<K, V>`. `get` / `get_mut` rend une
    // **vue d'instruction** (OWN-6.8) : bornée, jamais stockable.
    method!(MAP_KINDS, "len", "Map::len", [], "usize", PURE),
    method!(MAP_KINDS, "is_empty", "Map::is_empty", [], "bool", PURE),
    method!(
        MAP_KINDS,
        "insert",
        "Map::insert",
        ["K", "V"],
        "Option<V>",
        ALLOC
    ),
    method!(
        MAP_KINDS,
        "remove",
        "Map::remove",
        ["&K"],
        "Option<V>",
        PURE
    ),
    method!(
        MAP_KINDS,
        "contains_key",
        "Map::contains_key",
        ["&K"],
        "bool",
        PURE
    ),
    method!(MAP_KINDS, "get", "Map::get", ["&K"], "Option<&V>", PURE),
    method!(
        MAP_KINDS,
        "get_mut",
        "Map::get_mut",
        ["&K"],
        "Option<&mut V>",
        PURE
    ),
    method!(VEC_KINDS, "is_empty", "Vec::is_empty", [], "bool", PURE),
    method!(VEC_KINDS, "push", "Vec::push", ["T"], "()", ALLOC),
    method!(VEC_KINDS, "pop", "Vec::pop", [], "Option<T>", PURE),
    method!(VEC_KINDS, "clear", "Vec::clear", [], "()", PURE),
    method!(VEC_KINDS, "clone", "Vec::clone", [], "Vec<T>", ALLOC),
    // OWN-6.14: structural clones, resolved by the checker (no receiver kind:
    // never found by name lookup). `Clone::copy` is the clone of a type
    // without heap component, which allocates nothing.
    method!(&[], "clone", "Clone::clone", [], "Self", ALLOC),
    method!(&[], "clone", "Clone::copy", [], "Self", &[]),
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
    // RFC-RT-TRAPS-1: explicit integer forms accepted in profile `rt`
    // (PENDING_HUMAN_APPROVAL). `checked_*` returns `Option<Self>`.
    method!(
        INT_KINDS,
        "wrapping_add",
        "int::wrapping_add",
        ["Self"],
        "Self",
        PURE
    ),
    method!(
        INT_KINDS,
        "wrapping_sub",
        "int::wrapping_sub",
        ["Self"],
        "Self",
        PURE
    ),
    method!(
        INT_KINDS,
        "wrapping_mul",
        "int::wrapping_mul",
        ["Self"],
        "Self",
        PURE
    ),
    method!(
        INT_KINDS,
        "saturating_add",
        "int::saturating_add",
        ["Self"],
        "Self",
        PURE
    ),
    method!(
        INT_KINDS,
        "saturating_sub",
        "int::saturating_sub",
        ["Self"],
        "Self",
        PURE
    ),
    method!(
        INT_KINDS,
        "saturating_mul",
        "int::saturating_mul",
        ["Self"],
        "Self",
        PURE
    ),
    method!(
        INT_KINDS,
        "checked_add",
        "int::checked_add",
        ["Self"],
        "Option<Self>",
        PURE
    ),
    method!(
        INT_KINDS,
        "checked_sub",
        "int::checked_sub",
        ["Self"],
        "Option<Self>",
        PURE
    ),
    method!(
        INT_KINDS,
        "checked_mul",
        "int::checked_mul",
        ["Self"],
        "Option<Self>",
        PURE
    ),
    method!(
        INT_KINDS,
        "checked_div",
        "int::checked_div",
        ["Self"],
        "Option<Self>",
        PURE
    ),
    method!(
        INT_KINDS,
        "checked_rem",
        "int::checked_rem",
        ["Self"],
        "Option<Self>",
        PURE
    ),
    method!(
        SIGNED_KINDS,
        "wrapping_neg",
        "int::wrapping_neg",
        [],
        "Self",
        PURE
    ),
    method!(
        SIGNED_KINDS,
        "saturating_neg",
        "int::saturating_neg",
        [],
        "Self",
        PURE
    ),
    method!(
        SIGNED_KINDS,
        "checked_neg",
        "int::checked_neg",
        [],
        "Option<Self>",
        PURE
    ),
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
    if let Some(inner) = t.strip_prefix("Map<").and_then(|s| s.strip_suffix('>')) {
        let parts = split_top_level(inner, ',')?;
        if parts.len() != 2 {
            return None;
        }
        return Some(map_type(
            type_from_str(parts[0], env)?,
            type_from_str(parts[1], env)?,
        ));
    }
    if let Some(inner) = t.strip_prefix("Arena<").and_then(|s| s.strip_suffix('>')) {
        return Some(arena_type(type_from_str(inner, env)?));
    }
    if let Some(inner) = t.strip_prefix("Handle<").and_then(|s| s.strip_suffix('>')) {
        return Some(handle_type(type_from_str(inner, env)?));
    }
    for (name, _) in PRELUDE_GENERIC_OPAQUES {
        if let Some(inner) = t
            .strip_prefix(*name)
            .and_then(|s| s.strip_prefix('<'))
            .and_then(|s| s.strip_suffix('>'))
        {
            return generic_opaque(name, type_from_str(inner, env)?);
        }
    }
    for (prefix, wrap) in [("Vec<", 0usize), ("Option<", 1usize), ("Result<", 2usize)] {
        if let Some(inner) = t.strip_prefix(prefix).and_then(|s| s.strip_suffix('>')) {
            let parts = split_top_level(inner, ',')?;
            let types: Option<Vec<Type>> = parts.iter().map(|p| type_from_str(p, env)).collect();
            let types = types?;
            return match (wrap, types.as_slice()) {
                (0, [elem]) => Some(Type::Vec(Box::new(elem.clone()))),
                (1, [inner]) => Some(Type::Option(Box::new(inner.clone()))),
                (2, [ok, err]) => Some(Type::Result(Box::new(ok.clone()), Box::new(err.clone()))),
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

    /// The resolver produces `ValuePath::BuiltinFn` only from a `BUILTIN_FNS`
    /// entry (`sig.path`) or from `prelude_value_path`; every such path has a
    /// signature, so typeck's "unknown builtin function" arm cannot be taken.
    #[test]
    fn every_resolvable_builtin_function_path_has_a_signature() {
        for name in PRELUDE_VALUES {
            if let Some(path) = prelude_value_path(name) {
                assert!(builtin_fn(path).is_some(), "prelude `{name}` -> `{path}`");
            }
        }
        for sig in BUILTIN_FNS {
            assert!(builtin_fn(sig.path).is_some(), "{}", sig.path);
        }
    }

    /// Every opaque prelude type resolves by name, carries its reserved id,
    /// and maps back to the same Rust name; the reserved ids are distinct.
    #[test]
    fn every_opaque_prelude_type_round_trips() {
        let plain = [
            ("TcpListener", PRELUDE_TCP_LISTENER_DEF),
            ("TcpStream", PRELUDE_TCP_STREAM_DEF),
            ("CHandle", PRELUDE_CHANDLE_DEF),
            ("HttpRequest", PRELUDE_HTTP_REQUEST_DEF),
            ("HttpResponse", PRELUDE_HTTP_RESPONSE_DEF),
        ];
        let mut ids = vec![PRELUDE_ERROR_DEF];
        for (name, def) in plain {
            assert!(PRELUDE_TYPES.contains(&name), "{name}");
            assert_eq!(
                prelude_type(name),
                Some(Type::Nominal {
                    def,
                    name: name.to_string(),
                    args: Vec::new(),
                }),
                "{name}"
            );
            assert_eq!(prelude_opaque_name(def), Some(name));
            ids.push(def);
        }
        for (name, def) in PRELUDE_GENERIC_OPAQUES {
            assert!(PRELUDE_TYPES.contains(name), "{name}");
            assert_eq!(prelude_opaque_name(*def), Some(*name));
            let ty = generic_opaque(name, Type::I64).expect(name);
            assert!(
                matches!(ty, Type::Nominal { def: d, ref args, .. } if d == *def && args == &vec![Type::I64])
            );
            ids.push(*def);
        }
        assert_eq!(generic_opaque("Vec", Type::I64), None);
        assert_eq!(prelude_opaque_name(0), None);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "reserved ids must be distinct");
        assert_eq!(chandle_type(), prelude_type("CHandle").unwrap());
    }

    #[test]
    fn every_builtin_fn_parses() {
        assert_eq!(SPECIAL_FORM_FNS, &["std::thread::spawn"]);
        for sig in BUILTIN_FNS {
            if SPECIAL_FORM_FNS.contains(&sig.path) {
                // Its result still parses; its first parameter is a function name.
                assert!(fn_ret_type(sig) != Type::Error, "{}", sig.path);
                continue;
            }
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

    /// `type_from_str` is the only reader of the signature table; every
    /// compound form it accepts here is a shape a builtin signature may take.
    #[test]
    fn type_from_str_parses_the_compound_forms() {
        assert_eq!(type_from_str("()", &[]), Some(Type::Unit));
        assert_eq!(type_from_str("!", &[]), Some(Type::Never));
        assert_eq!(
            type_from_str("&i32", &[]),
            Some(Type::Ref {
                mutable: false,
                inner: Box::new(Type::I32)
            })
        );
        assert_eq!(
            type_from_str("&mut i32", &[]),
            Some(Type::Ref {
                mutable: true,
                inner: Box::new(Type::I32)
            })
        );
        assert_eq!(
            type_from_str("[u8]", &[]),
            Some(Type::Slice(Box::new(Type::U8)))
        );
        assert_eq!(
            type_from_str("(i32, bool)", &[]),
            Some(Type::Tuple(vec![Type::I32, Type::Bool]))
        );
        assert_eq!(
            type_from_str("Vec<Option<i32>>", &[]),
            Some(Type::Vec(Box::new(Type::Option(Box::new(Type::I32)))))
        );
        assert_eq!(
            type_from_str("Result<i32, Error>", &[]),
            Some(Type::Result(Box::new(Type::I32), Box::new(error_type())))
        );
        assert_eq!(
            type_from_str(" ( i32 , bool ) ", &[]),
            Some(Type::Tuple(vec![Type::I32, Type::Bool])),
            "surrounding and inner spaces are accepted"
        );
        assert_eq!(
            type_from_str("( )", &[]),
            Some(Type::Unit),
            "a whitespace-only unit is still the unit type"
        );
    }

    #[test]
    fn type_from_str_resolves_names_from_the_environment() {
        let env = [("T", Type::I32), ("Self", Type::String)];
        assert_eq!(type_from_str("T", &env), Some(Type::I32));
        assert_eq!(type_from_str("Self", &env), Some(Type::String));
        assert_eq!(type_from_str("str", &[]), Some(Type::Str));
        assert_eq!(type_from_str("String", &[]), Some(Type::String));
        assert_eq!(type_from_str("U", &env), None, "unbound name stays unknown");
    }

    /// Malformed or arity-inconsistent signature text must fail closed: the
    /// caller turns `None` into `Type::Error`, which the generator never emits.
    #[test]
    fn type_from_str_parses_map() {
        // STD-20: `Map<K, V>` in a builtin signature, with names from the
        // environment (the shape of `Map::new`'s return type).
        let env = [("K", Type::String), ("V", Type::I64)];
        assert_eq!(
            type_from_str("Map<K, V>", &env),
            Some(map_type(Type::String, Type::I64))
        );
        assert!(is_map_key_type(&Type::String) && is_map_key_type(&Type::U8));
        assert!(!is_map_key_type(&Type::F64));
        assert_eq!(prelude_opaque_name(PRELUDE_MAP_DEF), Some("Map"));
    }

    #[test]
    fn type_from_str_parses_arena_and_handle() {
        // STD-21 (RFC-UI-LANG-2): the shapes of `Arena::new` and `insert`.
        let env = [("T", Type::I64)];
        assert_eq!(type_from_str("Arena<T>", &env), Some(arena_type(Type::I64)));
        assert_eq!(
            type_from_str("Handle<T>", &env),
            Some(handle_type(Type::I64))
        );
        assert!(is_arena_type(&arena_type(Type::Bool)) && !is_arena_type(&Type::Bool));
        assert!(
            is_handle_type(&handle_type(Type::Bool)) && !is_handle_type(&arena_type(Type::Bool))
        );
        // HDL-9: a handle is a `Map` key; an arena is not.
        assert!(is_map_key_type(&handle_type(Type::String)));
        assert!(!is_map_key_type(&arena_type(Type::String)));
        assert_eq!(prelude_opaque_name(PRELUDE_ARENA_DEF), Some("Arena"));
        assert_eq!(prelude_opaque_name(PRELUDE_HANDLE_DEF), Some("Handle"));
        for p in ["Map::get", "Map::get_mut", "Arena::get", "Arena::get_mut"] {
            assert!(is_view_method(p), "{p}");
        }
        assert!(!is_view_method("Arena::remove") && !is_view_method("Vec::len"));
        assert!(
            is_exclusive_view_method("Arena::get_mut") && is_exclusive_view_method("Map::get_mut")
        );
        assert!(!is_exclusive_view_method("Arena::get"));
        assert!(builtin_method_takes_exclusive_receiver(
            MethodSelfKind::Arena,
            "insert"
        ));
        assert!(!builtin_method_takes_exclusive_receiver(
            MethodSelfKind::Arena,
            "get"
        ));
    }

    #[test]
    fn type_from_str_refuses_malformed_text() {
        for text in [
            "",
            "   ",
            "&",
            "&mut",
            "[i32",
            "(i32, (bool)",
            "(i32,",
            "Vec<i32, i32>",
            "Result<i32>",
            "Vec<>",
            "Option<i32, i32>",
            "Map<i32>",
            "Map<i32, i32, i32>",
            "Map<i32, Inconnu>",
            "Arena<Inconnu>",
            "Handle<Inconnu>",
            "Inconnu",
            "[Inconnu]",
            "(i32, Inconnu)",
            "&mut Inconnu",
            "Vec<(i32>",
        ] {
            assert_eq!(type_from_str(text, &[]), None, "accepted {text:?}");
        }
    }

    #[test]
    fn every_prelude_module_has_a_canonical_path() {
        for name in PRELUDE_MODULES {
            let path = builtin_root_module(name).expect("a prelude module");
            assert!(
                path == "std" || path == format!("std::{name}"),
                "{name} -> {path}"
            );
        }
        assert_eq!(builtin_root_module("nowhere"), None);
    }

    #[test]
    #[should_panic(expected = "invariant: the resolved builtin path `nowhere::f` has a signature")]
    fn a_builtin_path_without_a_signature_is_an_invariant_failure() {
        let _ = resolved_builtin_fn("nowhere::f");
    }
}
