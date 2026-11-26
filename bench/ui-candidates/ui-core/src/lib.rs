//! Prototype U2/U3 indépendant de la pile graphique (banc séparé, UI-D8).
//!
//! - **Arbre retained** à clés stables ; une description (`Desc`) est
//!   réconciliée par clé : un nœud dont la clé survit garde son identité
//!   et son état (focus, défilement).
//! - **Patch fail-closed** : une description invalide (clé dupliquée entre
//!   frères, taille négative ou non finie) est refusée et l'arbre précédent
//!   reste en place ; un patch construit sur une génération périmée est
//!   rejeté.
//! - **Layout** déterministe : rangées et colonnes, tailles fixes, `Fill`
//!   (partage du reste), `Auto` (contenu) ; texte mesuré par une métrique
//!   monospace fixe (le vrai moteur de texte vient avec la pile, U2).
//! - **Événements** : test de frappe sur le nœud le plus profond, focus,
//!   défilement.
//! - **Verify minimal** (U3) : débordement, sortie de conteneur,
//!   chevauchement entre frères ; diagnostics triés, donc déterministes.
//!
//! Aucune dépendance. Rien ici n'est adopté par un crate Ravum.

pub mod hot;
pub mod motion;
pub mod plugin;
pub mod responsive;
pub mod tokens;
pub mod verify;

use std::collections::{BTreeMap, BTreeSet};

pub const CHAR_W: f32 = 8.0;
pub const LINE_H: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Fixed(f32),
    Fill(f32),
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Row,
    Column,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub dir: Dir,
    pub width: Len,
    pub height: Len,
    pub padding: f32,
    pub gap: f32,
    /// Le contenu peut dépasser et défile (Verify ne signale pas le
    /// débordement dans l'axe principal).
    pub scroll: bool,
    pub focusable: bool,
    /// Décalage après placement (position relative) : la seule façon de
    /// faire se chevaucher deux frères, ce que Verify signale.
    pub offset: (f32, f32),
}

impl Default for Style {
    fn default() -> Self {
        Style {
            dir: Dir::Column,
            width: Len::Auto,
            height: Len::Auto,
            padding: 0.0,
            gap: 0.0,
            scroll: false,
            focusable: false,
            offset: (0.0, 0.0),
        }
    }
}

/// Description déclarative produite par la vue.
#[derive(Clone, Debug, PartialEq)]
pub struct Desc {
    pub key: u64,
    pub style: Style,
    pub text: Option<String>,
    pub children: Vec<Desc>,
}

impl Desc {
    pub fn new(key: u64, style: Style) -> Desc {
        Desc {
            key,
            style,
            text: None,
            children: Vec::new(),
        }
    }
    pub fn text(key: u64, s: &str) -> Desc {
        Desc {
            key,
            style: Style::default(),
            text: Some(s.to_string()),
            children: Vec::new(),
        }
    }
    pub fn with(mut self, c: Vec<Desc>) -> Desc {
        self.children = c;
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
    fn inside(&self, o: &Rect) -> bool {
        const E: f32 = 1e-3;
        self.x + E >= o.x
            && self.y + E >= o.y
            && self.x + self.w <= o.x + o.w + E
            && self.y + self.h <= o.y + o.h + E
    }
    fn overlaps(&self, o: &Rect) -> bool {
        const E: f32 = 1e-3;
        self.x + E < o.x + o.w
            && o.x + E < self.x + self.w
            && self.y + E < o.y + o.h
            && o.y + E < self.y + self.h
    }
}

/// Chemin de clés de la racine au nœud : l'identité stable d'un nœud.
pub type Path = Vec<u64>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchError {
    DuplicateKey { parent: Path, key: u64 },
    InvalidSize { path: Path },
    Stale { expected: u64, got: u64 },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeState {
    pub scroll: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    Overflow,
    OutOfContainer,
    Overlap,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Diagnostic {
    pub rule: Rule,
    pub path: Path,
    pub other: Option<Path>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Click { x: f32, y: f32 },
    Scroll { x: f32, y: f32, dy: f32 },
    FocusNext,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub created: usize,
    pub kept: usize,
    pub removed: usize,
}

pub struct Ui {
    root: Desc,
    generation: u64,
    states: BTreeMap<Path, NodeState>,
    focus: Option<Path>,
    rects: BTreeMap<Path, Rect>,
    clicks: Vec<Path>,
    viewport: (f32, f32),
}

fn validate(d: &Desc, path: &mut Path) -> Result<(), PatchError> {
    path.push(d.key);
    let bad = |l: Len| match l {
        Len::Fixed(v) | Len::Fill(v) => !v.is_finite() || v < 0.0,
        Len::Auto => false,
    };
    let s = &d.style;
    if bad(s.width)
        || bad(s.height)
        || !s.padding.is_finite()
        || s.padding < 0.0
        || !s.gap.is_finite()
        || s.gap < 0.0
        || !s.offset.0.is_finite()
        || !s.offset.1.is_finite()
    {
        return Err(PatchError::InvalidSize { path: path.clone() });
    }
    let mut seen = BTreeSet::new();
    for c in &d.children {
        if !seen.insert(c.key) {
            return Err(PatchError::DuplicateKey {
                parent: path.clone(),
                key: c.key,
            });
        }
    }
    for c in &d.children {
        validate(c, path)?;
    }
    path.pop();
    Ok(())
}

fn paths(d: &Desc, p: &mut Path, out: &mut BTreeSet<Path>) {
    p.push(d.key);
    out.insert(p.clone());
    for c in &d.children {
        paths(c, p, out);
    }
    p.pop();
}

fn first_focusable(d: &Desc, p: &mut Path, out: &mut Vec<Path>) {
    p.push(d.key);
    if d.style.focusable {
        out.push(p.clone());
    }
    for c in &d.children {
        first_focusable(c, p, out);
    }
    p.pop();
}

impl Ui {
    pub fn new(root: Desc, viewport: (f32, f32)) -> Result<Ui, PatchError> {
        validate(&root, &mut Vec::new())?;
        let mut ui = Ui {
            root: Desc::new(0, Style::default()),
            generation: 0,
            states: BTreeMap::new(),
            focus: None,
            rects: BTreeMap::new(),
            clicks: Vec::new(),
            viewport,
        };
        ui.install(root);
        Ok(ui)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Applique une nouvelle description construite sur la génération
    /// `base`. Fail-closed : en cas d'erreur, rien ne change.
    pub fn patch(&mut self, base: u64, root: Desc) -> Result<Stats, PatchError> {
        if base != self.generation {
            return Err(PatchError::Stale {
                expected: self.generation,
                got: base,
            });
        }
        validate(&root, &mut Vec::new())?;
        Ok(self.install(root))
    }

    fn install(&mut self, root: Desc) -> Stats {
        let mut old = BTreeSet::new();
        paths(&self.root, &mut Vec::new(), &mut old);
        let mut new = BTreeSet::new();
        paths(&root, &mut Vec::new(), &mut new);
        let stats = Stats {
            created: new.difference(&old).count(),
            kept: new.intersection(&old).count(),
            removed: if self.generation == 0 {
                0
            } else {
                old.difference(&new).count()
            },
        };
        self.states.retain(|p, _| new.contains(p));
        for p in &new {
            self.states.entry(p.clone()).or_default();
        }
        self.root = root;
        if let Some(f) = &self.focus {
            let mut fs = Vec::new();
            first_focusable(&self.root, &mut Vec::new(), &mut fs);
            if !fs.contains(f) {
                self.focus = None;
            }
        }
        self.generation += 1;
        self.relayout();
        stats
    }

    pub fn state(&self, p: &[u64]) -> Option<&NodeState> {
        self.states.get(p)
    }
    pub fn focus(&self) -> Option<&Path> {
        self.focus.as_ref()
    }
    pub fn rect(&self, p: &[u64]) -> Option<Rect> {
        self.rects.get(p).copied()
    }
    pub fn rects(&self) -> &BTreeMap<Path, Rect> {
        &self.rects
    }
    /// Nœuds cliqués, dans l'ordre (la « logique » de l'application les lit).
    pub fn take_clicks(&mut self) -> Vec<Path> {
        std::mem::take(&mut self.clicks)
    }

    pub fn viewport(&self) -> (f32, f32) {
        self.viewport
    }

    /// Reprend focus et défilement d'une autre instance (mêmes chemins).
    pub fn copy_states_from(&mut self, other: &Ui) {
        for (p, st) in &other.states {
            if let Some(mine) = self.states.get_mut(p) {
                *mine = st.clone();
            }
        }
        self.focus = other.focus.clone().filter(|f| self.states.contains_key(f));
        self.relayout();
    }

    pub fn resize(&mut self, viewport: (f32, f32)) {
        self.viewport = viewport;
        self.relayout();
    }

    // ---------- layout ----------

    fn intrinsic(d: &Desc) -> (f32, f32) {
        if let Some(t) = &d.text {
            return (t.chars().count() as f32 * CHAR_W, LINE_H);
        }
        let s = &d.style;
        let kids: Vec<(f32, f32)> = d
            .children
            .iter()
            .map(|c| {
                let (iw, ih) = Self::intrinsic(c);
                (fixed_or(c.style.width, iw), fixed_or(c.style.height, ih))
            })
            .collect();
        let gaps = s.gap * kids.len().saturating_sub(1) as f32;
        let (w, h) = match s.dir {
            Dir::Row => (
                kids.iter().map(|k| k.0).sum::<f32>() + gaps,
                kids.iter().map(|k| k.1).fold(0.0, f32::max),
            ),
            Dir::Column => (
                kids.iter().map(|k| k.0).fold(0.0, f32::max),
                kids.iter().map(|k| k.1).sum::<f32>() + gaps,
            ),
        };
        (w + 2.0 * s.padding, h + 2.0 * s.padding)
    }

    fn relayout(&mut self) {
        self.rects.clear();
        let root = self.root.clone();
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: self.viewport.0,
            h: self.viewport.1,
        };
        self.place(&root, r, &mut Vec::new());
    }

    fn place(&mut self, d: &Desc, r: Rect, path: &mut Path) {
        path.push(d.key);
        self.rects.insert(path.clone(), r);
        let s = &d.style;
        let inner = Rect {
            x: r.x + s.padding,
            y: r.y + s.padding,
            w: (r.w - 2.0 * s.padding).max(0.0),
            h: (r.h - 2.0 * s.padding).max(0.0),
        };
        let row = s.dir == Dir::Row;
        let main_avail = if row { inner.w } else { inner.h };
        let gaps = s.gap * d.children.len().saturating_sub(1) as f32;
        let mut fixed = 0.0;
        let mut weights = 0.0;
        let mut sizes = Vec::with_capacity(d.children.len());
        for c in &d.children {
            let (iw, ih) = Self::intrinsic(c);
            let (main, cross) = if row {
                (c.style.width, c.style.height)
            } else {
                (c.style.height, c.style.width)
            };
            let (imain, icross) = if row { (iw, ih) } else { (ih, iw) };
            let m = match main {
                Len::Fixed(v) => Some(v),
                Len::Auto => Some(imain),
                Len::Fill(_) => None,
            };
            if let Some(v) = m {
                fixed += v;
            } else if let Len::Fill(wt) = main {
                weights += wt;
            }
            let cr = match cross {
                Len::Fixed(v) => v,
                Len::Fill(_) => {
                    if row {
                        inner.h
                    } else {
                        inner.w
                    }
                }
                Len::Auto => icross,
            };
            sizes.push((m, main, cr));
        }
        let rest = (main_avail - fixed - gaps).max(0.0);
        let scroll = self.states.get(path.as_slice()).map_or(0.0, |st| st.scroll);
        let mut cursor = if row { inner.x } else { inner.y - scroll };
        for (c, (m, main, cr)) in d.children.iter().zip(sizes) {
            let m = m.unwrap_or_else(|| match main {
                Len::Fill(wt) if weights > 0.0 => rest * wt / weights,
                _ => 0.0,
            });
            let (ox, oy) = c.style.offset;
            let cr_rect = if row {
                Rect {
                    x: cursor + ox,
                    y: inner.y + oy,
                    w: m,
                    h: cr,
                }
            } else {
                Rect {
                    x: inner.x + ox,
                    y: cursor + oy,
                    w: cr,
                    h: m,
                }
            };
            self.place(c, cr_rect, path);
            cursor += m + s.gap;
        }
        path.pop();
    }

    // ---------- events ----------

    fn hit(&self, x: f32, y: f32) -> Option<Path> {
        // Le plus profond, puis le dernier dans l'ordre (dessiné au-dessus).
        let mut best: Option<&Path> = None;
        for (p, r) in &self.rects {
            if r.contains(x, y) && self.visible(p, x, y) && best.is_none_or(|b| p.len() >= b.len())
            {
                best = Some(p);
            }
        }
        best.cloned()
    }

    /// Un point dans un enfant d'un conteneur défilant n'est atteignable que
    /// s'il est aussi dans chaque ancêtre (découpage).
    fn visible(&self, p: &Path, x: f32, y: f32) -> bool {
        (1..p.len()).all(|n| self.rects.get(&p[..n]).is_some_and(|r| r.contains(x, y)))
    }

    pub fn dispatch(&mut self, e: &Event) {
        match *e {
            Event::Click { x, y } => {
                if let Some(p) = self.hit(x, y) {
                    let mut fs = Vec::new();
                    first_focusable(&self.root, &mut Vec::new(), &mut fs);
                    if let Some(f) = fs
                        .iter()
                        .filter(|f| p.starts_with(f))
                        .max_by_key(|f| f.len())
                    {
                        self.focus = Some(f.clone());
                    }
                    self.clicks.push(p);
                }
            }
            Event::Scroll { x, y, dy } => {
                let Some(mut p) = self.hit(x, y) else { return };
                loop {
                    if let Some(d) = self.find(&p) {
                        if d.style.scroll {
                            let max = self.scroll_max(&p, d);
                            let st = self.states.entry(p.clone()).or_default();
                            st.scroll = (st.scroll + dy).clamp(0.0, max);
                            self.relayout();
                            return;
                        }
                    }
                    if p.len() <= 1 {
                        return;
                    }
                    p.pop();
                }
            }
            Event::FocusNext => {
                let mut fs = Vec::new();
                first_focusable(&self.root, &mut Vec::new(), &mut fs);
                if fs.is_empty() {
                    return;
                }
                let i = self
                    .focus
                    .as_ref()
                    .and_then(|f| fs.iter().position(|x| x == f))
                    .map_or(0, |i| (i + 1) % fs.len());
                self.focus = Some(fs[i].clone());
            }
        }
    }

    fn scroll_max(&self, p: &Path, d: &Desc) -> f32 {
        let r = self.rects[p];
        let (_, ih) = Self::intrinsic(d);
        (ih - r.h).max(0.0)
    }

    pub fn find(&self, p: &[u64]) -> Option<&Desc> {
        let (first, rest) = p.split_first()?;
        if *first != self.root.key {
            return None;
        }
        let mut d = &self.root;
        for k in rest {
            d = d.children.iter().find(|c| c.key == *k)?;
        }
        Some(d)
    }

    // ---------- Verify minimal ----------

    /// Diagnostics triés : la même entrée donne toujours la même sortie.
    pub fn verify(&self) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        self.verify_node(&self.root, &mut Vec::new(), &mut out);
        out.sort();
        out
    }

    fn verify_node(&self, d: &Desc, path: &mut Path, out: &mut Vec<Diagnostic>) {
        path.push(d.key);
        let r = self.rects[path.as_slice()];
        if d.text.is_some() {
            let (iw, _) = Self::intrinsic(d);
            if iw > r.w + 1e-3 {
                out.push(Diagnostic {
                    rule: Rule::Overflow,
                    path: path.clone(),
                    other: None,
                });
            }
        }
        let s = &d.style;
        let inner = Rect {
            x: r.x + s.padding,
            y: r.y + s.padding,
            w: (r.w - 2.0 * s.padding).max(0.0),
            h: (r.h - 2.0 * s.padding).max(0.0),
        };
        let mut kids = Vec::new();
        for c in &d.children {
            path.push(c.key);
            let cr = self.rects[path.as_slice()];
            let ok = if s.scroll {
                // L'axe principal défile ; seul l'axe transverse est vérifié.
                match s.dir {
                    Dir::Column => {
                        cr.x + 1e-3 >= inner.x && cr.x + cr.w <= inner.x + inner.w + 1e-3
                    }
                    Dir::Row => cr.y + 1e-3 >= inner.y && cr.y + cr.h <= inner.y + inner.h + 1e-3,
                }
            } else {
                cr.inside(&inner)
            };
            if !ok {
                // Un texte trop long pour sa boîte est un débordement.
                let rule = if c.text.is_some() {
                    Rule::Overflow
                } else {
                    Rule::OutOfContainer
                };
                out.push(Diagnostic {
                    rule,
                    path: path.clone(),
                    other: None,
                });
            }
            kids.push((path.clone(), cr));
            path.pop();
        }
        for i in 0..kids.len() {
            for j in i + 1..kids.len() {
                if kids[i].1.w > 0.0
                    && kids[i].1.h > 0.0
                    && kids[j].1.w > 0.0
                    && kids[j].1.h > 0.0
                    && kids[i].1.overlaps(&kids[j].1)
                {
                    out.push(Diagnostic {
                        rule: Rule::Overlap,
                        path: kids[i].0.clone(),
                        other: Some(kids[j].0.clone()),
                    });
                }
            }
        }
        for c in &d.children {
            self.verify_node(c, path, out);
        }
        path.pop();
    }
}

fn fixed_or(l: Len, intrinsic: f32) -> f32 {
    match l {
        Len::Fixed(v) => v,
        _ => intrinsic,
    }
}
