//! U5 (logique) : Design Contract — jetons, références, thèmes, variantes,
//! import et export DTCG, et audit des écarts (Verify).
//!
//! Un jeton est une couleur, une dimension ou une référence `{nom}` vers un
//! autre jeton. Un thème remplace des jetons de base. La résolution suit les
//! références et refuse : jeton inconnu, cycle, type inattendu. Un style de
//! composant ne porte que des **noms** de jetons ; un composant de plugin
//! hérite donc de la charte et du thème de l'hôte. L'audit signale toute
//! dimension d'un arbre qui n'est pas une valeur de la charte.

use crate::{Desc, Len, Path, Style};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Color([u8; 4]),
    Dimension(f32),
    Ref(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Color,
    Dimension,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TokenError {
    Unknown(String),
    Cycle(Vec<String>),
    WrongKind { token: String, expected: Kind },
    Json(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tokens {
    pub base: BTreeMap<String, Value>,
    pub themes: BTreeMap<String, BTreeMap<String, Value>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resolved {
    Color([u8; 4]),
    Dimension(f32),
}

impl Tokens {
    pub fn resolve(&self, name: &str, theme: Option<&str>) -> Result<Resolved, TokenError> {
        let mut seen: Vec<String> = Vec::new();
        let mut cur = name.to_string();
        loop {
            if seen.contains(&cur) {
                seen.push(cur);
                return Err(TokenError::Cycle(seen));
            }
            seen.push(cur.clone());
            let v = theme
                .and_then(|t| self.themes.get(t))
                .and_then(|t| t.get(&cur))
                .or_else(|| self.base.get(&cur))
                .ok_or_else(|| TokenError::Unknown(cur.clone()))?;
            match v {
                Value::Color(c) => return Ok(Resolved::Color(*c)),
                Value::Dimension(d) => return Ok(Resolved::Dimension(*d)),
                Value::Ref(r) => cur = r.clone(),
            }
        }
    }

    pub fn dimension(&self, name: &str, theme: Option<&str>) -> Result<f32, TokenError> {
        match self.resolve(name, theme)? {
            Resolved::Dimension(d) => Ok(d),
            _ => Err(TokenError::WrongKind {
                token: name.into(),
                expected: Kind::Dimension,
            }),
        }
    }

    pub fn color(&self, name: &str, theme: Option<&str>) -> Result<[u8; 4], TokenError> {
        match self.resolve(name, theme)? {
            Resolved::Color(c) => Ok(c),
            _ => Err(TokenError::WrongKind {
                token: name.into(),
                expected: Kind::Color,
            }),
        }
    }

    /// Vérifie que tout jeton se résout, dans la base et dans chaque thème.
    pub fn check(&self) -> Result<(), TokenError> {
        let mut themes: Vec<Option<&str>> = vec![None];
        themes.extend(self.themes.keys().map(|t| Some(t.as_str())));
        for t in themes {
            for n in self.base.keys() {
                self.resolve(n, t)?;
            }
        }
        Ok(())
    }

    /// Les dimensions permises par la charte (tous thèmes confondus).
    pub fn allowed_dimensions(&self) -> BTreeSet<u32> {
        let mut out = BTreeSet::new();
        let mut themes: Vec<Option<&str>> = vec![None];
        themes.extend(self.themes.keys().map(|t| Some(t.as_str())));
        for t in themes {
            for n in self.base.keys() {
                if let Ok(Resolved::Dimension(d)) = self.resolve(n, t) {
                    out.insert(d.to_bits());
                }
            }
        }
        out
    }
}

/// Style de composant exprimé en noms de jetons, avec variantes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComponentStyle {
    /// propriété (`padding`, `gap`, `width`, `height`) → jeton.
    pub props: BTreeMap<String, String>,
    pub variants: BTreeMap<String, BTreeMap<String, String>>,
}

impl ComponentStyle {
    /// Applique les jetons (et la variante) sur un style de base.
    pub fn apply(
        &self,
        tokens: &Tokens,
        theme: Option<&str>,
        variant: Option<&str>,
        mut s: Style,
    ) -> Result<Style, TokenError> {
        let mut props = self.props.clone();
        if let Some(v) = variant {
            props.extend(self.variants.get(v).cloned().unwrap_or_default());
        }
        for (prop, tok) in &props {
            let d = tokens.dimension(tok, theme)?;
            match prop.as_str() {
                "padding" => s.padding = d,
                "gap" => s.gap = d,
                "width" => s.width = Len::Fixed(d),
                "height" => s.height = Len::Fixed(d),
                _ => return Err(TokenError::Unknown(format!("property {prop}"))),
            }
        }
        Ok(s)
    }
}

/// Écart à la charte : une dimension qui n'est pas une valeur de jeton.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deviation {
    pub path: Path,
    pub property: &'static str,
    pub value_bits: u32,
}

/// Audit déterministe : `padding` et `gap` non nuls, largeur et hauteur
/// fixes, doivent être des valeurs de la charte.
pub fn audit(root: &Desc, tokens: &Tokens) -> Vec<Deviation> {
    let allowed = tokens.allowed_dimensions();
    let mut out = Vec::new();
    fn walk(d: &Desc, p: &mut Path, allowed: &BTreeSet<u32>, out: &mut Vec<Deviation>) {
        p.push(d.key);
        let s = &d.style;
        let mut check = |property: &'static str, v: f32| {
            if v != 0.0 && !allowed.contains(&v.to_bits()) {
                out.push(Deviation {
                    path: p.clone(),
                    property,
                    value_bits: v.to_bits(),
                });
            }
        };
        check("padding", s.padding);
        check("gap", s.gap);
        if let Len::Fixed(w) = s.width {
            check("width", w);
        }
        if let Len::Fixed(h) = s.height {
            check("height", h);
        }
        for c in &d.children {
            walk(c, p, allowed, out);
        }
        p.pop();
    }
    walk(root, &mut Vec::new(), &allowed, &mut out);
    out.sort();
    out
}

// ---------- DTCG (Design Tokens Community Group), JSON minimal ----------

fn hex(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
}

fn value_json(v: &Value) -> String {
    match v {
        Value::Color(c) => format!("{{\"$type\":\"color\",\"$value\":\"{}\"}}", hex(*c)),
        Value::Dimension(d) => format!("{{\"$type\":\"dimension\",\"$value\":\"{d}px\"}}"),
        Value::Ref(r) => format!("{{\"$value\":\"{{{r}}}\"}}"),
    }
}

fn group_json(m: &BTreeMap<String, Value>) -> String {
    let items: Vec<String> = m
        .iter()
        .map(|(k, v)| format!("\"{k}\":{}", value_json(v)))
        .collect();
    format!("{{{}}}", items.join(","))
}

/// Export DTCG : `{"base": {...}, "$themes": {"dark": {...}}}`, noms plats
/// (`color.primary`), ordre trié donc sortie stable.
pub fn export(t: &Tokens) -> String {
    let themes: Vec<String> = t
        .themes
        .iter()
        .map(|(k, m)| format!("\"{k}\":{}", group_json(m)))
        .collect();
    format!(
        "{{\"base\":{},\"$themes\":{{{}}}}}",
        group_json(&t.base),
        themes.join(",")
    )
}

#[derive(Debug, PartialEq)]
enum J {
    Str(String),
    Obj(Vec<(String, J)>),
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn err<T>(&self, m: &str) -> Result<T, TokenError> {
        Err(TokenError::Json(format!("{m} at byte {}", self.i)))
    }
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Result<(), TokenError> {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            self.err(&format!("expected '{}'", c as char))
        }
    }
    fn string(&mut self) -> Result<String, TokenError> {
        self.eat(b'"')?;
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i] != b'"' {
            if self.s[self.i] == b'\\' {
                return self.err("escapes are not supported");
            }
            self.i += 1;
        }
        if self.i >= self.s.len() {
            return self.err("unterminated string");
        }
        let out = String::from_utf8(self.s[start..self.i].to_vec())
            .map_err(|_| TokenError::Json("utf-8".into()))?;
        self.i += 1;
        Ok(out)
    }
    fn value(&mut self, depth: usize) -> Result<J, TokenError> {
        if depth > 16 {
            return self.err("too deep");
        }
        self.ws();
        match self.s.get(self.i) {
            Some(b'"') => Ok(J::Str(self.string()?)),
            Some(b'{') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(J::Obj(items));
                }
                loop {
                    let k = self.string()?;
                    self.eat(b':')?;
                    items.push((k, self.value(depth + 1)?));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(J::Obj(items));
                        }
                        _ => return self.err("expected ',' or '}'"),
                    }
                }
            }
            _ => self.err("expected a string or an object"),
        }
    }
}

fn parse_color(s: &str) -> Option<[u8; 4]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 8 && h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?, if h.len() == 8 { b(6)? } else { 255 }])
}

fn token(name: &str, j: &J) -> Result<Value, TokenError> {
    let J::Obj(fields) = j else {
        return Err(TokenError::Json(format!("{name}: token must be an object")));
    };
    let get = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v);
    let Some(J::Str(v)) = get("$value") else {
        return Err(TokenError::Json(format!("{name}: missing $value")));
    };
    if let Some(r) = v.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        return Ok(Value::Ref(r.to_string()));
    }
    match get("$type") {
        Some(J::Str(t)) if t == "color" => parse_color(v)
            .map(Value::Color)
            .ok_or_else(|| TokenError::Json(format!("{name}: bad color"))),
        Some(J::Str(t)) if t == "dimension" => v
            .strip_suffix("px")
            .and_then(|n| n.parse::<f32>().ok())
            .filter(|d| d.is_finite() && *d >= 0.0)
            .map(Value::Dimension)
            .ok_or_else(|| TokenError::Json(format!("{name}: bad dimension"))),
        _ => Err(TokenError::Json(format!("{name}: unknown $type"))),
    }
}

fn group(j: &J) -> Result<BTreeMap<String, Value>, TokenError> {
    let J::Obj(items) = j else {
        return Err(TokenError::Json("group must be an object".into()));
    };
    let mut m = BTreeMap::new();
    for (k, v) in items {
        if m.insert(k.clone(), token(k, v)?).is_some() {
            return Err(TokenError::Json(format!("duplicate token {k}")));
        }
    }
    Ok(m)
}

/// Import DTCG (le sous-ensemble produit par [`export`]). Fail-closed : un
/// document invalide, ou des jetons qui ne se résolvent pas, sont refusés.
pub fn import(s: &str) -> Result<Tokens, TokenError> {
    let mut p = P {
        s: s.as_bytes(),
        i: 0,
    };
    let root = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return p.err("trailing data");
    }
    let J::Obj(items) = root else {
        return Err(TokenError::Json("root must be an object".into()));
    };
    let mut t = Tokens::default();
    for (k, v) in &items {
        match k.as_str() {
            "base" => t.base = group(v)?,
            "$themes" => {
                let J::Obj(ts) = v else {
                    return Err(TokenError::Json("$themes must be an object".into()));
                };
                for (name, g) in ts {
                    t.themes.insert(name.clone(), group(g)?);
                }
            }
            other => return Err(TokenError::Json(format!("unknown key {other}"))),
        }
    }
    t.check()?;
    Ok(t)
}
