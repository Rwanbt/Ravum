//! Ravum semantic types (the canonical type representation shared by the
//! checker, the ownership engine and the code generator).
//!
//! **Interface gelée**: the codegen matches on this enum exhaustively. Adding a
//! variant requires updating `codegen` and `spec/language/types.md`.

use ravum_lang_syntax::ast::NodeId;
use std::fmt;

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
    Param { name: String, index: usize },
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
