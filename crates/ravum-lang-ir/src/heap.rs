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
    /// IR v1.8 (RFC-UI-LANG-2, STD-21): an arena of raw words. The `Map*`
    /// operations also apply to it, with handles as keys (see [`Arena`]).
    Arena(Arena),
}

/// STD-21.9: limits shared with the rustc path (`RV_ARENA_*` of the
/// prelude): a handle packs into one word, arena 20 bits, slot 20 bits,
/// generation 24 bits, so its numeric order is (arena, slot, generation)
/// (STD-21.11).
pub const ARENA_ID_MAX: u64 = (1 << 20) - 1;
pub const ARENA_SLOT_MAX: u64 = (1 << 20) - 1;
pub const ARENA_GEN_MAX: u64 = (1 << 24) - 1;

pub fn pack_handle(arena: u64, slot: u64, generation: u64) -> u64 {
    (arena << 44) | (slot << 24) | generation
}

pub fn unpack_handle(h: u64) -> (u64, u64, u64) {
    (h >> 44, (h >> 24) & ARENA_SLOT_MAX, h & ARENA_GEN_MAX)
}

/// An arena (STD-21.3..21.5): slots of (generation, element word); free
/// slots, the smallest reused first; a slot at the last generation is
/// retired (never in `free`).
#[derive(Debug, Clone, PartialEq)]
pub struct Arena {
    pub id: u64,
    pub slots: Vec<(u64, Option<u64>)>,
    pub free: std::collections::BTreeSet<u64>,
    pub len: usize,
}

impl Arena {
    /// The slot of a live handle of this arena (STD-21.3).
    fn live(&self, h: u64) -> Option<usize> {
        let (id, slot, g) = unpack_handle(h);
        let i = slot as usize;
        (id == self.id && i < self.slots.len() && self.slots[i].0 == g && self.slots[i].1.is_some())
            .then_some(i)
    }

    /// Live handles and words, in increasing slot order (STD-21.6).
    fn live_entries(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, (g, x))| x.map(|w| (pack_handle(self.id, i as u64, *g), w)))
    }
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
    /// IR v1.8: a language panic with a fixed text (STD-21.9: `too many
    /// arenas in this run`, `arena is full`).
    Panic(&'static str),
    /// A fault: invalid handle (a compiler bug) or the heap limit.
    Fault(&'static str),
}

#[derive(Debug, Default)]
pub struct Heap {
    objects: Vec<Object>,
    bytes: usize,
    /// STD-21.5: arenas created in this run (`ArenaNew` and clones).
    arenas: u64,
}

impl HeapStop {
    /// The `panic:`/`fault:` text of this stop.
    pub fn message(&self) -> String {
        match self {
            HeapStop::IndexOutOfBounds { len, index } => {
                format!("index out of bounds: the len is {len} but the index is {index}")
            }
            HeapStop::Panic(m) | HeapStop::Fault(m) => m.to_string(),
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
            Object::Arena(a) => a.slots.len() * 16 + a.free.len() * 8,
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
            ArenaNew => {
                let id = self.next_arena()?;
                self.alloc(Object::Arena(Arena {
                    id,
                    slots: Vec::new(),
                    free: Default::default(),
                    len: 0,
                }))?
            }
            ArenaInsert => {
                // STD-21.5: the smallest free slot, else a new one. Only a
                // new slot is allocated memory: reusing a freed slot is not
                // charged (a long run of insert/remove stays within the heap
                // limit, as on the rustc path).
                let reuse = self.arena(a)?.free.first().copied();
                if reuse.is_none() {
                    if self.arena(a)?.slots.len() as u64 > ARENA_SLOT_MAX {
                        return Err(HeapStop::Panic("arena is full"));
                    }
                    self.charge(16)?;
                }
                let ar = self.arena_mut(a)?;
                let slot = match reuse {
                    Some(s) => {
                        ar.free.remove(&s);
                        s
                    }
                    None => {
                        ar.slots.push((0, None));
                        ar.slots.len() as u64 - 1
                    }
                };
                ar.slots[slot as usize].1 = Some(b);
                ar.len += 1;
                pack_handle(ar.id, slot, ar.slots[slot as usize].0)
            }
            MapLen if self.is_arena(a) => self.arena(a)?.len as u64,
            MapKeys if self.is_arena(a) => {
                let v: Vec<u64> = self.arena(a)?.live_entries().map(|e| e.0).collect();
                self.alloc(Object::Vec(v))?
            }
            MapVals if self.is_arena(a) => {
                let v: Vec<u64> = self.arena(a)?.live_entries().map(|e| e.1).collect();
                self.alloc(Object::Vec(v))?
            }
            MapSetVals if self.is_arena(a) => {
                let v = self.vec(b)?.clone();
                let ar = self.arena_mut(a)?;
                if ar.len != v.len() {
                    return Err(HeapStop::Fault("arena values of a different length"));
                }
                for (slot, x) in ar.slots.iter_mut().filter_map(|s| s.1.as_mut()).zip(v) {
                    *slot = x;
                }
                0
            }
            MapClone if self.is_arena(a) => {
                // STD-21.8: same slots and generations, a new identifier.
                let mut copy = self.arena(a)?.clone();
                copy.id = self.next_arena()?;
                self.alloc(Object::Arena(copy))?
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
            StrPushChar => {
                let c = scalar(b)?;
                self.charge(c.len_utf8())?;
                self.str_mut(a)?.push(c);
                0
            }
            StrOfChar => {
                let c = scalar(a)?;
                self.text(c.encode_utf8(&mut [0; 4]))?
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
                    Object::Arena(_) => Object::Arena(Arena {
                        id: 0,
                        slots: Vec::new(),
                        free: Default::default(),
                        len: 0,
                    }),
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

    fn is_arena(&self, h: u64) -> bool {
        matches!(self.get(h), Ok(Object::Arena(_)))
    }

    fn arena(&self, h: u64) -> Result<&Arena, HeapStop> {
        match self.get(h)? {
            Object::Arena(a) => Ok(a),
            _ => Err(HeapStop::Fault("not an arena")),
        }
    }

    fn arena_mut(&mut self, h: u64) -> Result<&mut Arena, HeapStop> {
        match self.get_mut(h)? {
            Object::Arena(a) => Ok(a),
            _ => Err(HeapStop::Fault("not an arena")),
        }
    }

    /// STD-21.5 / STD-21.9: the next arena identifier of this run.
    fn next_arena(&mut self) -> Result<u64, HeapStop> {
        if self.arenas >= ARENA_ID_MAX {
            return Err(HeapStop::Panic("too many arenas in this run"));
        }
        self.arenas += 1;
        Ok(self.arenas)
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
        if self.is_arena(h) {
            // STD-21.3: a stale or foreign handle is absent.
            let ar = self.arena(h)?;
            return Ok(match ar.live(k) {
                Some(i) => (true, ar.slots[i].1.unwrap_or(0)),
                None => (false, 0),
            });
        }
        let key = self.key(h, k)?;
        Ok(match self.map(h)?.1.get(&key) {
            Some(v) => (true, *v),
            None => (false, 0),
        })
    }

    /// `MapInsert`: inserts or replaces; `(present, old value)`.
    pub fn map_insert(&mut self, h: u64, k: u64, v: u64) -> Result<(bool, u64), HeapStop> {
        if self.is_arena(h) {
            // On an arena: write back into a live slot (a `get_mut` view);
            // a stale handle changes nothing.
            let ar = self.arena_mut(h)?;
            return Ok(match ar.live(k) {
                Some(i) => (true, ar.slots[i].1.replace(v).unwrap_or(0)),
                None => (false, 0),
            });
        }
        let key = self.key(h, k)?;
        // Only a new entry allocates; replacing a value does not (as on the
        // rustc path, where a long run of replacements never grows memory).
        if !self.map(h)?.1.contains_key(&key) {
            let extra = match &key {
                MapKey::Text(t) => t.len(),
                _ => 0,
            };
            self.charge(24 + extra)?;
        }
        Ok(match self.map_mut(h)?.insert(key, v) {
            Some(old) => (true, old),
            None => (false, 0),
        })
    }

    /// `MapRemove`: `(present, old value)`.
    pub fn map_remove(&mut self, h: u64, k: u64) -> Result<(bool, u64), HeapStop> {
        if self.is_arena(h) {
            // STD-21.4: the generation goes up; the last one retires the slot.
            let ar = self.arena_mut(h)?;
            let Some(i) = ar.live(k) else {
                return Ok((false, 0));
            };
            let old = ar.slots[i].1.take().unwrap_or(0);
            ar.len -= 1;
            ar.slots[i].0 += 1;
            if ar.slots[i].0 < ARENA_GEN_MAX {
                ar.free.insert(i as u64);
            }
            return Ok((true, old));
        }
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

/// The `char` a word holds (IR v1.9). A word that is not a Unicode scalar
/// value is a fault: the checker and the lowering only produce scalars.
fn scalar(w: u64) -> Result<char, HeapStop> {
    u32::try_from(w)
        .ok()
        .and_then(char::from_u32)
        .ok_or(HeapStop::Fault("not a char"))
}
