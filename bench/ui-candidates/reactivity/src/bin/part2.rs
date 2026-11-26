//! U1-B, tour 2, partie 2 (UI-D13, UI-D16).
//!
//! 1. Dépendances **dynamiques** : une ligne lit `params[i]` ou
//!    `params[(i + 1) % n]` selon un drapeau `alt[i]`. B enregistre les
//!    lectures pendant l'évaluation d'une ligne et les remplace à chaque
//!    réévaluation. Chaque mise à jour est vérifiée contre une
//!    reconstruction complète (A) : même arbre, ligne par ligne.
//! 2. Changements de **structure** (insertion et retrait d'une ligne au
//!    milieu) : diff par position contre diff par clé. On compte les
//!    nœuds réécrits et on mesure le temps.
//! 3. **Frontières de layout** au-dessus de Taffy : chaque ligne de hauteur
//!    fixe est un arbre Taffy séparé, la racine empile les lignes. Un
//!    changement local ne recalcule que sa ligne. Le résultat est vérifié
//!    contre le layout global (même largeur et même `x` de chaque cellule).
//!
//! Mesures CPU headless, valables pour la machine et le commit cités.

use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::{Duration, Instant};

const RUNS: usize = 200;

fn stats(name: &str, mut v: Vec<Duration>) {
    v.sort();
    let q = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
    println!(
        "{name}: p50={:?} p95={:?} p99={:?} max={:?}",
        q(0.5),
        q(0.95),
        q(0.99),
        v[v.len() - 1]
    );
}

// ---------- 1. Dépendances dynamiques ----------

struct Model {
    params: Vec<f32>,
    alt: Vec<bool>,
}

/// The view of row `i`; `read` is called for every parameter it reads.
fn row_text(m: &Model, i: usize, read: &mut impl FnMut(usize)) -> String {
    let n = m.params.len();
    let k = if m.alt[i] { (i + 1) % n } else { i };
    read(k);
    format!("row {i}: {:.3}", m.params[k])
}

fn rebuild_all(m: &Model) -> Vec<String> {
    (0..m.params.len())
        .map(|i| row_text(m, i, &mut |_| {}))
        .collect()
}

/// B with run-time dependency tracking: `readers[k]` = rows that read
/// `params[k]` at their last evaluation; `reads[r]` = what row `r` read.
struct Tracked {
    rows: Vec<String>,
    reads: Vec<Vec<usize>>,
    readers: Vec<Vec<usize>>,
}

impl Tracked {
    fn new(m: &Model) -> Tracked {
        let n = m.params.len();
        let mut t = Tracked {
            rows: vec![String::new(); n],
            reads: vec![Vec::new(); n],
            readers: vec![Vec::new(); n],
        };
        for r in 0..n {
            t.eval(m, r);
        }
        t
    }

    fn eval(&mut self, m: &Model, r: usize) {
        for k in std::mem::take(&mut self.reads[r]) {
            self.readers[k].retain(|&x| x != r);
        }
        let mut got = Vec::new();
        self.rows[r] = row_text(m, r, &mut |k| got.push(k));
        for &k in &got {
            self.readers[k].push(r);
        }
        self.reads[r] = got;
    }

    /// A parameter changed: re-evaluate its current readers.
    fn param_changed(&mut self, m: &Model, k: usize) -> usize {
        let rs = self.readers[k].clone();
        for &r in &rs {
            self.eval(m, r);
        }
        rs.len()
    }

    /// The flag of row `r` changed (it is read by row `r` itself).
    fn flag_changed(&mut self, m: &Model, r: usize) -> usize {
        self.eval(m, r);
        1
    }
}

fn dynamic_deps(n: usize) {
    let mut m = Model {
        params: (0..n).map(|i| i as f32 * 0.001).collect(),
        alt: vec![false; n],
    };
    let mut t = Tracked::new(&m);
    let (mut a, mut b) = (Vec::with_capacity(RUNS), Vec::with_capacity(RUNS));
    let mut reevaluated = 0usize;
    for r in 0..RUNS {
        let i = (r * 7919) % n;
        let flip = r % 3 == 0;
        if flip {
            m.alt[i] = !m.alt[i];
        } else {
            m.params[i] = r as f32 * 0.25;
        }
        let t0 = Instant::now();
        let full = black_box(rebuild_all(&m));
        a.push(t0.elapsed());
        let t0 = Instant::now();
        reevaluated += if flip {
            t.flag_changed(&m, i)
        } else {
            t.param_changed(&m, i)
        };
        b.push(t0.elapsed());
        assert!(
            full == t.rows,
            "dynamic B disagrees with a full rebuild at update {r}"
        );
    }
    let edges: usize = t.readers.iter().map(Vec::len).sum();
    println!(
        "-- dynamic dependencies, {n} rows, 1/3 of the updates change which parameter a row reads"
    );
    stats("A full rebuild", a);
    stats("B tracked reads (re-registered on every evaluation)", b);
    println!(
        "B rows re-evaluated per update: {:.2}; live edges: {edges}",
        reevaluated as f64 / RUNS as f64
    );
}

// ---------- 2. Changements de structure ----------

#[derive(Clone, PartialEq)]
struct Item {
    key: u64,
    text: String,
}

/// Positional diff: element i against element i; returns rewritten nodes.
fn diff_positional(old: &mut Vec<Item>, new: &[Item]) -> usize {
    let mut w = 0;
    for (i, x) in new.iter().enumerate() {
        match old.get_mut(i) {
            Some(o) if o == x => {}
            Some(o) => {
                *o = x.clone();
                w += 1;
            }
            None => {
                old.push(x.clone());
                w += 1;
            }
        }
    }
    w += old.len().saturating_sub(new.len());
    old.truncate(new.len());
    w
}

/// Keyed diff: retained nodes are found by key; only new or changed keys
/// are written, removed keys are dropped. `order` is the new key order.
fn diff_keyed(old: &mut BTreeMap<u64, String>, order: &mut Vec<u64>, new: &[Item]) -> usize {
    let mut w = 0;
    let mut seen = std::collections::BTreeSet::new();
    for x in new {
        seen.insert(x.key);
        match old.get_mut(&x.key) {
            Some(t) if *t == x.text => {}
            Some(t) => {
                *t = x.text.clone();
                w += 1;
            }
            None => {
                old.insert(x.key, x.text.clone());
                w += 1;
            }
        }
    }
    let before = old.len();
    old.retain(|k, _| seen.contains(k));
    w += before - old.len();
    order.clear();
    order.extend(new.iter().map(|x| x.key));
    w
}

fn structure(n: usize) {
    let mut items: Vec<Item> = (0..n as u64)
        .map(|k| Item {
            key: k,
            text: format!("item {k}"),
        })
        .collect();
    let mut pos = items.clone();
    let mut keyed: BTreeMap<u64, String> = items.iter().map(|x| (x.key, x.text.clone())).collect();
    let mut order: Vec<u64> = items.iter().map(|x| x.key).collect();
    let mut next = n as u64;
    let (mut tp, mut tk) = (Vec::new(), Vec::new());
    let (mut wp, mut wk) = (0usize, 0usize);
    for r in 0..RUNS {
        let at = items.len() / 2;
        if r % 2 == 0 {
            items.insert(
                at,
                Item {
                    key: next,
                    text: format!("item {next}"),
                },
            );
            next += 1;
        } else {
            items.remove(at);
        }
        let t0 = Instant::now();
        wp += diff_positional(&mut pos, &items);
        tp.push(t0.elapsed());
        let t0 = Instant::now();
        wk += diff_keyed(&mut keyed, &mut order, &items);
        tk.push(t0.elapsed());
        assert!(pos == items, "positional diff is wrong");
        assert!(order
            .iter()
            .zip(&items)
            .all(|(k, x)| *k == x.key && keyed[k] == x.text));
    }
    println!("-- structure: insert/remove one row in the middle of {n}");
    stats("positional diff", tp);
    stats("keyed diff", tk);
    println!(
        "nodes rewritten per update: positional {:.1}, keyed {:.1}",
        wp as f64 / RUNS as f64,
        wk as f64 / RUNS as f64
    );
}

// ---------- 3. Frontières de layout au-dessus de Taffy ----------

fn layout_boundaries(rows: usize) {
    use taffy::prelude::*;
    const ROW_H: f32 = 32.0;
    let cell = |w: f32| Style {
        size: Size {
            width: length(w),
            height: length(24.0),
        },
        ..Default::default()
    };
    let row_style = Style {
        flex_direction: FlexDirection::Row,
        gap: Size {
            width: length(8.0),
            height: zero(),
        },
        size: Size {
            width: length(1280.0),
            height: length(ROW_H),
        },
        ..Default::default()
    };

    // Global tree (reference).
    let mut g: TaffyTree<()> = TaffyTree::new();
    let mut g_rows = Vec::new();
    let mut g_cells = Vec::new();
    for _ in 0..rows {
        let cs: Vec<NodeId> = (0..4).map(|_| g.new_leaf(cell(100.0)).unwrap()).collect();
        g_cells.push(cs.clone());
        g_rows.push(g.new_with_children(row_style.clone(), &cs).unwrap());
    }
    let root = g
        .new_with_children(
            Style {
                flex_direction: FlexDirection::Column,
                size: Size {
                    width: length(1280.0),
                    height: auto(),
                },
                ..Default::default()
            },
            &g_rows,
        )
        .unwrap();
    g.compute_layout(root, Size::MAX_CONTENT).unwrap();

    // Bounded: one tree per row; the root stacks rows at `i * ROW_H`.
    let mut sub: Vec<(TaffyTree<()>, NodeId, Vec<NodeId>)> = (0..rows)
        .map(|_| {
            let mut t: TaffyTree<()> = TaffyTree::new();
            let cs: Vec<NodeId> = (0..4).map(|_| t.new_leaf(cell(100.0)).unwrap()).collect();
            let r = t.new_with_children(row_style.clone(), &cs).unwrap();
            t.compute_layout(r, Size::MAX_CONTENT).unwrap();
            (t, r, cs)
        })
        .collect();

    let (mut tg, mut tb) = (Vec::new(), Vec::new());
    for r in 0..RUNS {
        let i = (r * 7919) % rows;
        let w = 100.0 + (r % 7) as f32;
        let t0 = Instant::now();
        g.set_style(g_cells[i][0], cell(w)).unwrap();
        g.compute_layout(root, Size::MAX_CONTENT).unwrap();
        tg.push(t0.elapsed());
        let t0 = Instant::now();
        let (t, rid, cs) = &mut sub[i];
        t.set_style(cs[0], cell(w)).unwrap();
        t.compute_layout(*rid, Size::MAX_CONTENT).unwrap();
        tb.push(t0.elapsed());
        // Same result as the global layout: sizes and x, and the row's y.
        for (gc, bc) in g_cells[i].iter().zip(cs.iter()) {
            let (a, b) = (g.layout(*gc).unwrap(), t.layout(*bc).unwrap());
            assert_eq!((a.size.width, a.location.x), (b.size.width, b.location.x));
        }
        assert_eq!(g.layout(g_rows[i]).unwrap().location.y, i as f32 * ROW_H);
        black_box(&sub);
    }
    println!(
        "-- layout boundaries over taffy, {rows} fixed-height rows ({} nodes)",
        g.total_node_count()
    );
    stats("global tree, resize one cell", tg);
    stats("one tree per row (boundary), resize one cell", tb);
}

fn main() {
    println!(
        "U1-B round 2 part 2 | runs={RUNS} | {} {} | cpus={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, |n| n.get())
    );
    for n in [1_000, 10_000] {
        dynamic_deps(n);
    }
    for n in [1_000, 10_000] {
        structure(n);
    }
    layout_boundaries(2_000);
}
