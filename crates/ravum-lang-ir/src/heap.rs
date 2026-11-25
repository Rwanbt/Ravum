//! IR v1.3 run heap: `String` and `Vec<T>` objects of one run.
//!
//! An object is named by a handle (`Ty::Obj`, 8 bytes). Ownership is the
//! checker's (a moved value is never used again), so copying a handle is a
//! move, and a borrow (`&String`, `&mut Vec<T>`) is the handle itself. There
//! is **no drop during a run**: the whole heap is freed when the run ends.
//! A run that allocates past [`HEAP_LIMIT`] bytes stops with a fault
//! (fail-closed), never with an out-of-memory crash.
//!
//! The same code serves the interpreter and the native backend: the
//! independent oracle for these operations is the rustc path (Rust's own
//! `String`/`Vec`), not this module.

/// Largest number of bytes one run may allocate.
pub const HEAP_LIMIT: usize = 256 << 20;

/// Handle 0 is never a valid object (a zeroed slot is not an object).
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    Str(String),
    /// IR v1.6 (RFC-UI-LANG-1 P3): an ordered map. Keys are ordered by the
    /// spec's total order (numeric, or UTF-8 bytes for text); values are raw
    /// words (scalar bits or a handle).
    Map(MapKind, std::collections::BTreeMap<MapKey, u64>),
    /// Elements as raw 64-bit words (scalars only in IR v1.3).
    Vec(Vec<u64>),
}

/// How the keys of a map are read from their raw word (fixed at creation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapKind {
    /// Signed integers (two's complement words, sign-extended).
    Signed,
    /// Unsigned integers, `bool`, `char`.
    Unsigned,
    /// `String`: the word is a string handle; the key is its contents.
    Text,
}

impl MapKind {
    /// The `a` operand of `HeapOp::MapNew`.
    pub fn code(self) -> u64 {
        match self {
            MapKind::Signed => 0,
            MapKind::Unsigned => 1,
            MapKind::Text => 2,
        }
    }

    pub fn from_code(c: u64) -> Option<MapKind> {
        Some(match c {
            0 => MapKind::Signed,
            1 => MapKind::Unsigned,
            2 => MapKind::Text,
            _ => return None,
        })
    }
}

/// A key in its total order (STD map rules: numeric order, `false < true`,
/// code points for `char`, lexicographic UTF-8 bytes for text).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MapKey {
    Signed(i64),
    Unsigned(u64),
    Text(String),
}

/// Why a heap operation stopped the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapStop {
    /// A language trap: `index out of bounds: the len is {len} but the index
    /// is {index}` (EXPR-7.6, the text rustc prints).
    IndexOutOfBounds { len: u64, index: u64 },
    /// A fault: invalid handle (a compiler bug) or the heap limit.
    Fault(&'static str),
}

#[derive(Debug, Default)]
pub struct Heap {
    objects: Vec<Object>,
    bytes: usize,
}

impl HeapStop {
    /// The `panic:`/`fault:` text of this stop.
    pub fn message(&self) -> String {
        match self {
            HeapStop::IndexOutOfBounds { len, index } => {
                format!("index out of bounds: the len is {len} but the index is {index}")
            }
            HeapStop::Fault(m) => m.to_string(),
        }
    }
}

impl Heap {
    fn charge(&mut self, n: usize) -> Result<(), HeapStop> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > HEAP_LIMIT {
            return Err(HeapStop::Fault("heap limit"));
        }
        Ok(())
    }

    pub fn alloc(&mut self, o: Object) -> Result<u64, HeapStop> {
        let n = match &o {
            Object::Str(s) => s.len(),
            Object::Vec(v) => v.len() * 8,
            Object::Map(_, m) => m.len() * 24,
        };
        self.charge(n + 32)?;
        self.objects.push(o);
        Ok(self.objects.len() as u64)
    }

    fn get(&self, h: u64) -> Result<&Object, HeapStop> {
        h.checked_sub(1)
            .and_then(|i| self.objects.get(i as usize))
            .ok_or(HeapStop::Fault("invalid object handle"))
    }

    fn get_mut(&mut self, h: u64) -> Result<&mut Object, HeapStop> {
        h.checked_sub(1)
            .and_then(|i| self.objects.get_mut(i as usize))
            .ok_or(HeapStop::Fault("invalid object handle"))
    }

    pub fn str(&self, h: u64) -> Result<&str, HeapStop> {
        match self.get(h)? {
            Object::Str(s) => Ok(s),
            _ => Err(HeapStop::Fault("not a string")),
        }
    }

    fn str_mut(&mut self, h: u64) -> Result<&mut String, HeapStop> {
        match self.get_mut(h)? {
            Object::Str(s) => Ok(s),
            _ => Err(HeapStop::Fault("not a string")),
        }
    }

    fn vec(&self, h: u64) -> Result<&Vec<u64>, HeapStop> {
        match self.get(h)? {
            Object::Vec(v) => Ok(v),
            _ => Err(HeapStop::Fault("not a vector")),
        }
    }

    fn vec_mut(&mut self, h: u64) -> Result<&mut Vec<u64>, HeapStop> {
        match self.get_mut(h)? {
            Object::Vec(v) => Ok(v),
            _ => Err(HeapStop::Fault("not a vector")),
        }
    }

    /// One heap operation; `a`, `b` are raw 64-bit operands. Returns the raw
    /// result (0 when the operation has none).
    pub fn op(&mut self, op: crate::ir::HeapOp, a: u64, b: u64) -> Result<u64, HeapStop> {
        use crate::ir::HeapOp::*;
        Ok(match op {
            StrNew => self.alloc(Object::Str(String::new()))?,
            StrClone => {
                let s = self.str(a)?.to_string();
                self.alloc(Object::Str(s))?
            }
            StrPush => {
                let add = self.str(b)?.to_string();
                self.charge(add.len())?;
                self.str_mut(a)?.push_str(&add);
                0
            }
            StrLen => self.str(a)?.len() as u64,
            StrEq => (self.str(a)? == self.str(b)?) as u64,
            VecNew => self.alloc(Object::Vec(Vec::new()))?,
            VecPush => {
                self.charge(8)?;
                self.vec_mut(a)?.push(b);
                0
            }
            VecLen => self.vec(a)?.len() as u64,
            VecGet => {
                let v = self.vec(a)?;
                *v.get(b as usize).ok_or(HeapStop::IndexOutOfBounds {
                    len: v.len() as u64,
                    index: b,
                })?
            }
            VecClone => {
                let v = self.vec(a)?.clone();
                self.alloc(Object::Vec(v))?
            }
            MapNew => {
                let kind = MapKind::from_code(a).ok_or(HeapStop::Fault("invalid map kind"))?;
                self.alloc(Object::Map(kind, Default::default()))?
            }
            MapLen => self.map(a)?.1.len() as u64,
            MapKeys => {
                // A fresh vector of the keys in order; text keys are fresh
                // strings (the map keeps its own).
                let (kind, m) = self.map(a)?;
                let keys: Vec<MapKey> = m.keys().cloned().collect();
                let mut words = Vec::with_capacity(keys.len());
                for k in keys {
                    words.push(match (kind, k) {
                        (_, MapKey::Signed(x)) => x as u64,
                        (_, MapKey::Unsigned(x)) => x,
                        (_, MapKey::Text(t)) => self.text(&t)?,
                    });
                }
                self.alloc(Object::Vec(words))?
            }
            MapVals => {
                let v: Vec<u64> = self.map(a)?.1.values().copied().collect();
                self.alloc(Object::Vec(v))?
            }
            MapSetVals => {
                // The values of map `a`, in key order, become those of vector
                // `b` (same length, checked).
                let v = self.vec(b)?.clone();
                let m = self.map_mut(a)?;
                if m.len() != v.len() {
                    return Err(HeapStop::Fault("map values of a different length"));
                }
                for (slot, x) in m.values_mut().zip(v) {
                    *slot = x;
                }
                0
            }
            MapClone => {
                let (kind, m) = self.map(a)?;
                let m = m.clone();
                self.alloc(Object::Map(kind, m))?
            }
            StrFind => {
                let (h, n) = (self.str(a)?, self.str(b)?);
                h.find(n).map_or(u64::MAX, |i| i as u64)
            }
            StrStartsWith => self.str(a)?.starts_with(self.str(b)?) as u64,
            StrEndsWith => self.str(a)?.ends_with(self.str(b)?) as u64,
            StrSplit => {
                let (h, sep) = (self.str(a)?, self.str(b)?);
                if sep.is_empty() {
                    return Err(HeapStop::Fault("empty separator"));
                }
                let parts: Vec<String> = h.split(sep).map(str::to_string).collect();
                let mut words = Vec::with_capacity(parts.len());
                for t in parts {
                    words.push(self.text(&t)?);
                }
                self.alloc(Object::Vec(words))?
            }
            StrTail => {
                let h = self.str(a)?;
                let t = h
                    .get(b as usize..)
                    .ok_or(HeapStop::Fault("string index not on a char boundary"))?
                    .to_string();
                self.text(&t)?
            }
            StrTruncate => {
                let h = self.str_mut(a)?;
                if b as usize > h.len() || !h.is_char_boundary(b as usize) {
                    return Err(HeapStop::Fault("string index not on a char boundary"));
                }
                h.truncate(b as usize);
                0
            }
            StrChars => {
                let v: Vec<u64> = self.str(a)?.chars().map(|c| c as u64).collect();
                self.alloc(Object::Vec(v))?
            }
            StrIsCharBoundary => {
                let h = self.str(a)?;
                (usize::try_from(b).is_ok_and(|i| h.is_char_boundary(i))) as u64
            }
            StrJoin => {
                let sep = self.str(b)?.to_string();
                let mut out = String::new();
                for (i, w) in self.vec(a)?.iter().enumerate() {
                    if i > 0 {
                        out.push_str(&sep);
                    }
                    out.push_str(self.str(*w)?);
                }
                self.text(&out)?
            }
            Replace => {
                // `b` was moved into the assignment: take its contents
                // (no new allocation, nothing charged) and leave it empty.
                let empty = match self.get(b)? {
                    Object::Str(_) => Object::Str(String::new()),
                    Object::Vec(_) => Object::Vec(Vec::new()),
                    Object::Map(k, _) => Object::Map(*k, Default::default()),
                };
                if std::mem::discriminant(self.get(a)?) != std::mem::discriminant(&empty) {
                    return Err(HeapStop::Fault("replace between different kinds"));
                }
                let o = std::mem::replace(self.get_mut(b)?, empty);
                *self.get_mut(a)? = o;
                0
            }
        })
    }

    /// `VecSet`: three operands.
    pub fn vec_set(&mut self, h: u64, i: u64, x: u64) -> Result<(), HeapStop> {
        let v = self.vec_mut(h)?;
        let len = v.len() as u64;
        let slot = v
            .get_mut(i as usize)
            .ok_or(HeapStop::IndexOutOfBounds { len, index: i })?;
        *slot = x;
        Ok(())
    }

    /// `VecPop` into an `Option`: `(present, value)`.
    pub fn vec_pop(&mut self, h: u64) -> Result<(bool, u64), HeapStop> {
        Ok(match self.vec_mut(h)?.pop() {
            Some(x) => (true, x),
            None => (false, 0),
        })
    }

    fn map(&self, h: u64) -> Result<(MapKind, &std::collections::BTreeMap<MapKey, u64>), HeapStop> {
        match self.get(h)? {
            Object::Map(k, m) => Ok((*k, m)),
            _ => Err(HeapStop::Fault("not a map")),
        }
    }

    fn map_mut(
        &mut self,
        h: u64,
    ) -> Result<&mut std::collections::BTreeMap<MapKey, u64>, HeapStop> {
        match self.get_mut(h)? {
            Object::Map(_, m) => Ok(m),
            _ => Err(HeapStop::Fault("not a map")),
        }
    }

    /// The key of `map` that raw word `w` stands for.
    fn key(&self, map: u64, w: u64) -> Result<MapKey, HeapStop> {
        Ok(match self.map(map)?.0 {
            MapKind::Signed => MapKey::Signed(w as i64),
            MapKind::Unsigned => MapKey::Unsigned(w),
            MapKind::Text => MapKey::Text(self.str(w)?.to_string()),
        })
    }

    /// `MapGet`: `(present, value)` (value 0 when absent).
    pub fn map_get(&self, h: u64, k: u64) -> Result<(bool, u64), HeapStop> {
        let key = self.key(h, k)?;
        Ok(match self.map(h)?.1.get(&key) {
            Some(v) => (true, *v),
            None => (false, 0),
        })
    }

    /// `MapInsert`: inserts or replaces; `(present, old value)`.
    pub fn map_insert(&mut self, h: u64, k: u64, v: u64) -> Result<(bool, u64), HeapStop> {
        let key = self.key(h, k)?;
        let extra = match &key {
            MapKey::Text(t) => t.len(),
            _ => 0,
        };
        self.charge(24 + extra)?;
        Ok(match self.map_mut(h)?.insert(key, v) {
            Some(old) => (true, old),
            None => (false, 0),
        })
    }

    /// `MapRemove`: `(present, old value)`.
    pub fn map_remove(&mut self, h: u64, k: u64) -> Result<(bool, u64), HeapStop> {
        let key = self.key(h, k)?;
        Ok(match self.map_mut(h)?.remove(&key) {
            Some(old) => (true, old),
            None => (false, 0),
        })
    }

    /// A new string holding `text`.
    pub fn text(&mut self, text: &str) -> Result<u64, HeapStop> {
        self.alloc(Object::Str(text.to_string()))
    }
}
