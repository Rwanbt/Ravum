//! Ravum semantic types (the canonical type representation shared by the
//! checker, the ownership engine and the code generator).
//!
//! **Interface gelée**: the codegen matches on this enum exhaustively. Adding a
//! variant requires updating `codegen` and `spec/language/types.md`.

use ravum_lang_syntax::ast::NodeId;
use std::fmt;

/// True when `!` appears anywhere in `ty`.
///
/// G-5.11 / TYP-1.6 make `!` legal **only** as the whole return type of a
/// divergent function: it is neither a parameter type, nor a field type, nor a
/// `Vec`/generic argument. This helper is the single predicate used by the
/// resolver to enforce that restriction.
pub fn contains_never(ty: &Type) -> bool {
    match ty {
        Type::Never => true,
        Type::Ref { inner, .. } | Type::Slice(inner) | Type::Vec(inner) | Type::Option(inner) => {
            contains_never(inner)
        }
        Type::Array { elem, .. } => contains_never(elem),
        Type::Tuple(items) => items.iter().any(contains_never),
        Type::Result(ok, err) => contains_never(ok) || contains_never(err),
        Type::Nominal { args, .. } => args.iter().any(contains_never),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
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
    F32,
    F64,
    Bool,
    Char,
    Unit,
    /// Bottom type produced by `panic`, `exit` and `return`; coerces to any type.
    Never,
    /// Unsized UTF-8 text; only legal behind a reference in Core 1.0.
    Str,
    /// Owned UTF-8 text.
    String,
    Ref {
        mutable: bool,
        inner: Box<Type>,
    },
    Array {
        elem: Box<Type>,
        len: usize,
    },
    Slice(Box<Type>),
    Tuple(Vec<Type>),
    Vec(Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    /// User struct or enum, identified by its item `NodeId` in `Facts::item_paths`.
    Nominal {
        def: NodeId,
        name: String,
        args: Vec<Type>,
    },
    /// Generic parameter, index in the enclosing declaration.
    Param {
        name: String,
        index: usize,
    },
    /// Type of a recovered error; never code-generated.
    Error,
}

impl Type {
    pub fn i32() -> Type {
        Type::I32
    }

    pub fn unit() -> Type {
        Type::Unit
    }

    pub fn string() -> Type {
        Type::String
    }

    pub fn str_ref() -> Type {
        Type::Ref {
            mutable: false,
            inner: Box::new(Type::Str),
        }
    }

    pub fn vec(inner: Type) -> Type {
        Type::Vec(Box::new(inner))
    }

    pub fn option(inner: Type) -> Type {
        Type::Option(Box::new(inner))
    }

    pub fn result(ok: Type, err: Type) -> Type {
        Type::Result(Box::new(ok), Box::new(err))
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    /// Types whose values are copied instead of moved. References are `Copy`
    /// (they are non-escaping in Core 1.0); aggregates are `Copy` only when all
    /// their components are. Nominal user types are resolved by the checker,
    /// which fills `Facts::copy_types` with the definitive answer.
    pub fn is_int(&self) -> bool {
        matches!(
            self,
            Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::I128
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
                | Type::U128
                | Type::Isize
                | Type::Usize
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Type::F32 | Type::F64)
    }

    pub fn is_numeric(&self) -> bool {
        self.is_int() || self.is_float()
    }

    pub fn is_signed_int(&self) -> bool {
        matches!(
            self,
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 | Type::Isize
        )
    }

    pub fn is_reference(&self) -> bool {
        matches!(self, Type::Ref { .. })
    }

    pub fn int_bits(&self) -> Option<u32> {
        Some(match self {
            Type::I8 | Type::U8 => 8,
            Type::I16 | Type::U16 => 16,
            Type::I32 | Type::U32 => 32,
            Type::I64 | Type::U64 => 64,
            Type::I128 | Type::U128 => 128,
            Type::Isize | Type::Usize => 64,
            _ => return None,
        })
    }

    pub fn is_str_like(&self) -> bool {
        match self {
            Type::String | Type::Str => true,
            Type::Ref { inner, .. } => matches!(**inner, Type::Str),
            _ => false,
        }
    }

    /// Name used in human diagnostics (`&text` for the borrowed text type).
    pub fn display(&self) -> String {
        format!("{self}")
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::I8 => write!(f, "i8"),
            Type::I16 => write!(f, "i16"),
            Type::I32 => write!(f, "i32"),
            Type::I64 => write!(f, "i64"),
            Type::I128 => write!(f, "i128"),
            Type::U8 => write!(f, "u8"),
            Type::U16 => write!(f, "u16"),
            Type::U32 => write!(f, "u32"),
            Type::U64 => write!(f, "u64"),
            Type::U128 => write!(f, "u128"),
            Type::Isize => write!(f, "isize"),
            Type::Usize => write!(f, "usize"),
            Type::F32 => write!(f, "f32"),
            Type::F64 => write!(f, "f64"),
            Type::Bool => write!(f, "bool"),
            Type::Char => write!(f, "char"),
            Type::Unit => write!(f, "()"),
            Type::Never => write!(f, "!"),
            Type::Str => write!(f, "text"),
            Type::String => write!(f, "String"),
            Type::Ref { mutable, inner } => {
                if *mutable {
                    write!(f, "&mut {inner}")
                } else {
                    write!(f, "&{inner}")
                }
            }
            Type::Array { elem, len } => write!(f, "[{elem}; {len}]"),
            Type::Slice(inner) => write!(f, "[{inner}]"),
            Type::Tuple(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                if items.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }
            Type::Vec(inner) => write!(f, "Vec<{inner}>"),
            Type::Option(inner) => write!(f, "Option<{inner}>"),
            Type::Result(ok, err) => write!(f, "Result<{ok}, {err}>"),
            Type::Nominal { name, args, .. } => {
                write!(f, "{name}")?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            Type::Param { name, .. } => write!(f, "{name}"),
            Type::Error => write!(f, "<error>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integers() -> Vec<Type> {
        vec![
            Type::I8,
            Type::I16,
            Type::I32,
            Type::I64,
            Type::I128,
            Type::U8,
            Type::U16,
            Type::U32,
            Type::U64,
            Type::U128,
            Type::Isize,
            Type::Usize,
        ]
    }

    #[test]
    fn constructors_produce_the_expected_shape() {
        assert_eq!(Type::i32(), Type::I32);
        assert_eq!(Type::unit(), Type::Unit);
        assert_eq!(Type::string(), Type::String);
        assert_eq!(
            Type::str_ref(),
            Type::Ref {
                mutable: false,
                inner: Box::new(Type::Str)
            }
        );
        assert_eq!(Type::vec(Type::I32), Type::Vec(Box::new(Type::I32)));
        assert_eq!(Type::option(Type::I32), Type::Option(Box::new(Type::I32)));
        assert_eq!(
            Type::result(Type::I32, Type::String),
            Type::Result(Box::new(Type::I32), Box::new(Type::String))
        );
    }

    #[test]
    fn predicates_are_exact_for_every_variant() {
        for ty in integers() {
            assert!(ty.is_int(), "{ty} must be an integer");
            assert!(ty.is_numeric(), "{ty} must be numeric");
            assert!(!ty.is_float(), "{ty} is not a float");
            assert!(!ty.is_error(), "{ty} is not an error type");
            assert!(!ty.is_reference(), "{ty} is not a reference");
            assert!(!ty.is_str_like(), "{ty} is not text");
        }
        for ty in [Type::F32, Type::F64] {
            assert!(ty.is_float());
            assert!(ty.is_numeric());
            assert!(!ty.is_int());
            assert!(!ty.is_signed_int());
            assert!(!ty.is_error());
        }
        for ty in [
            Type::I8,
            Type::I16,
            Type::I32,
            Type::I64,
            Type::I128,
            Type::Isize,
        ] {
            assert!(ty.is_signed_int(), "{ty} must be signed");
        }
        for ty in [
            Type::U8,
            Type::U16,
            Type::U32,
            Type::U64,
            Type::U128,
            Type::Usize,
        ] {
            assert!(!ty.is_signed_int(), "{ty} must be unsigned");
        }
        for ty in [
            Type::Bool,
            Type::Char,
            Type::Unit,
            Type::Never,
            Type::Str,
            Type::String,
            Type::Ref {
                mutable: false,
                inner: Box::new(Type::I32),
            },
        ] {
            assert!(!ty.is_int(), "{ty} is not an integer");
            assert!(!ty.is_float(), "{ty} is not a float");
            assert!(!ty.is_numeric(), "{ty} is not numeric");
            assert!(!ty.is_signed_int(), "{ty} is not a signed integer");
            assert!(!ty.is_error(), "{ty} is not an error type");
        }
        assert!(Type::Error.is_error());
        assert!(Type::Error.is_error() && Type::Error.display() == "<error>");
        assert!(Type::String.is_str_like());
        assert!(Type::Str.is_str_like());
        assert!(Type::str_ref().is_str_like());
        assert!(Type::Ref {
            mutable: true,
            inner: Box::new(Type::Str)
        }
        .is_str_like());
        assert!(!Type::Ref {
            mutable: false,
            inner: Box::new(Type::I32)
        }
        .is_str_like());
        assert!(Type::Ref {
            mutable: false,
            inner: Box::new(Type::I32)
        }
        .is_reference());
        assert!(!Type::I32.is_reference());
    }

    #[test]
    fn int_bits_covers_every_integer_width() {
        let expected = [
            (Type::I8, 8),
            (Type::U8, 8),
            (Type::I16, 16),
            (Type::U16, 16),
            (Type::I32, 32),
            (Type::U32, 32),
            (Type::I64, 64),
            (Type::U64, 64),
            (Type::I128, 128),
            (Type::U128, 128),
            (Type::Isize, 64),
            (Type::Usize, 64),
        ];
        for (ty, bits) in expected {
            assert_eq!(ty.int_bits(), Some(bits), "width of {ty}");
        }
        for ty in [Type::Bool, Type::F32, Type::F64, Type::String, Type::Error] {
            assert_eq!(ty.int_bits(), None, "{ty} has no integer width");
        }
    }

    #[test]
    fn contains_never_recurses_through_every_compound() {
        let never_ref = Type::Ref {
            mutable: false,
            inner: Box::new(Type::Never),
        };
        let nominal_never = Type::Nominal {
            def: 0,
            name: "W".to_string(),
            args: vec![Type::Never],
        };
        for ty in [
            Type::Never,
            never_ref,
            Type::Slice(Box::new(Type::Never)),
            Type::Vec(Box::new(Type::Never)),
            Type::Option(Box::new(Type::Never)),
            Type::Array {
                elem: Box::new(Type::Never),
                len: 2,
            },
            Type::Tuple(vec![Type::I32, Type::Never]),
            Type::Result(Box::new(Type::Never), Box::new(Type::I32)),
            Type::Result(Box::new(Type::I32), Box::new(Type::Never)),
            nominal_never,
        ] {
            assert!(contains_never(&ty), "{ty} contains the never type");
        }
        for ty in [
            Type::I32,
            Type::Error,
            Type::Param {
                name: "T".to_string(),
                index: 0,
            },
            Type::Ref {
                mutable: false,
                inner: Box::new(Type::I32),
            },
            Type::Slice(Box::new(Type::I32)),
            Type::Vec(Box::new(Type::I32)),
            Type::Option(Box::new(Type::I32)),
            Type::Array {
                elem: Box::new(Type::I32),
                len: 2,
            },
            Type::Tuple(vec![Type::I32, Type::Bool]),
            Type::Result(Box::new(Type::I32), Box::new(Type::String)),
            Type::Nominal {
                def: 0,
                name: "Point".to_string(),
                args: vec![Type::I32],
            },
        ] {
            assert!(!contains_never(&ty), "{ty} does not contain the never type");
        }
    }

    #[test]
    fn display_is_stable_for_every_family() {
        let cases = [
            (Type::I8, "i8"),
            (Type::I16, "i16"),
            (Type::I32, "i32"),
            (Type::I64, "i64"),
            (Type::I128, "i128"),
            (Type::U8, "u8"),
            (Type::U16, "u16"),
            (Type::U32, "u32"),
            (Type::U64, "u64"),
            (Type::U128, "u128"),
            (Type::Isize, "isize"),
            (Type::Usize, "usize"),
            (Type::F32, "f32"),
            (Type::F64, "f64"),
            (Type::Bool, "bool"),
            (Type::Char, "char"),
            (Type::Unit, "()"),
            (Type::Never, "!"),
            (Type::Str, "text"),
            (Type::String, "String"),
            (Type::Error, "<error>"),
            (
                Type::Ref {
                    mutable: false,
                    inner: Box::new(Type::I32),
                },
                "&i32",
            ),
            (
                Type::Ref {
                    mutable: true,
                    inner: Box::new(Type::I32),
                },
                "&mut i32",
            ),
            (
                Type::Array {
                    elem: Box::new(Type::I32),
                    len: 3,
                },
                "[i32; 3]",
            ),
            (Type::Slice(Box::new(Type::I32)), "[i32]"),
            (Type::Tuple(vec![Type::I32]), "(i32,)"),
            (Type::Tuple(vec![Type::I32, Type::Bool]), "(i32, bool)"),
            (Type::Vec(Box::new(Type::I32)), "Vec<i32>"),
            (Type::Option(Box::new(Type::I32)), "Option<i32>"),
            (
                Type::Result(Box::new(Type::I32), Box::new(Type::String)),
                "Result<i32, String>",
            ),
            (
                Type::Nominal {
                    def: 0,
                    name: "Point".to_string(),
                    args: vec![],
                },
                "Point",
            ),
            (
                Type::Nominal {
                    def: 0,
                    name: "Pair".to_string(),
                    args: vec![Type::I32, Type::Bool],
                },
                "Pair<i32, bool>",
            ),
            (
                Type::Param {
                    name: "T".to_string(),
                    index: 0,
                },
                "T",
            ),
        ];
        for (ty, text) in cases {
            assert_eq!(ty.display(), text, "display of {ty:?}");
        }
    }
}
