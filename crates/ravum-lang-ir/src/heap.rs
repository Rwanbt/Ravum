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
    /// Elements as raw 64-bit words (scalars only in IR v1.3).
    Vec(Vec<u64>),
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

    /// A new string holding `text`.
    pub fn text(&mut self, text: &str) -> Result<u64, HeapStop> {
        self.alloc(Object::Str(text.to_string()))
    }
}
