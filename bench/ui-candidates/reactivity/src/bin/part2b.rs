//! U1-B, tour 2, complément de la partie 2 : les deux réserves de §3.
//!
//! 1. Diff par clé **incrémental** : préfixe et suffixe communs comparés par
//!    clé (des entiers), puis table seulement pour le milieu. Contrôle :
//!    l'ordre et le contenu retenus égalent la nouvelle liste.
//! 2. Lignes à **hauteur automatique** derrière une frontière : chaque ligne
//!    est un arbre Taffy séparé ; quand une hauteur change, les positions
//!    `y` des lignes suivantes viennent d'un arbre de Fenwick (sommes
//!    préfixes en O(log n)). Contrôle : `y` et hauteur de chaque ligne égaux
//!    au layout global Taffy, vérifiés sur un échantillon à chaque mise à
//!    jour et sur toutes les lignes à la fin.
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

// ---------- 1. Diff par clé incrémental ----------

/// Retained keyed list: `order` (keys in display order) and `nodes`.
struct Keyed {
    order: Vec<u64>,
    nodes: BTreeMap<u64, String>,
}

impl Keyed {
    /// Reconcile with `new` (keys, texts). Common prefix and suffix are
    /// matched by key only; the middle goes through the table. Returns the
    /// nodes created or dropped. Texts of retained keys are owned by the
    /// granular update path (§3.1), not compared here.
    fn diff(&mut self, new: &[(u64, String)]) -> usize {
        let (o, n) = (&self.order, new);
        let mut p = 0;
        while p < o.len() && p < n.len() && o[p] == n[p].0 {
            p += 1;
        }
        let mut s = 0;
        while s < o.len() - p && s < n.len() - p && o[o.len() - 1 - s] == n[n.len() - 1 - s].0 {
            s += 1;
        }
        let old_mid: Vec<u64> = o[p..o.len() - s].to_vec();
        let new_mid = &n[p..n.len() - s];
        let mut w = 0;
        for k in &old_mid {
            if !new_mid.iter().any(|(nk, _)| nk == k) {
                self.nodes.remove(k);
                w += 1;
            }
        }
        for (k, t) in new_mid {
            if !self.nodes.contains_key(k) {
                self.nodes.insert(*k, t.clone());
                w += 1;
            }
        }
        let tail = self.order.len() - s;
        self.order.splice(p..tail, new_mid.iter().map(|(k, _)| *k));
        w
    }
}

fn keyed_incremental(n: usize) {
    let mut items: Vec<(u64, String)> = (0..n as u64).map(|k| (k, format!("item {k}"))).collect();
    let mut kd = Keyed {
        order: items.iter().map(|x| x.0).collect(),
        nodes: items.iter().cloned().collect(),
    };
    let mut next = n as u64;
    let (mut t, mut w) = (Vec::new(), 0usize);
    for r in 0..RUNS {
        let at = items.len() / 2;
        if r % 2 == 0 {
            items.insert(at, (next, format!("item {next}")));
            next += 1;
        } else {
            items.remove(at);
        }
        let t0 = Instant::now();
        w += black_box(kd.diff(&items));
        t.push(t0.elapsed());
        assert_eq!(kd.order.len(), items.len());
        assert!(kd
            .order
            .iter()
            .zip(&items)
            .all(|(k, x)| *k == x.0 && kd.nodes[k] == x.1));
        assert_eq!(kd.nodes.len(), items.len());
    }
    println!("-- incremental keyed diff: insert/remove one row in the middle of {n}");
    stats("prefix/suffix by key, table for the middle", t);
    println!(
        "nodes created or dropped per update: {:.1}",
        w as f64 / RUNS as f64
    );
}

// ---------- 2. Lignes à hauteur automatique ----------

/// Fenwick tree of row heights: `prefix(i)` = y of row `i`.
struct Fenwick(Vec<f32>);

impl Fenwick {
    fn new(n: usize) -> Fenwick {
        Fenwick(vec![0.0; n + 1])
    }
    fn add(&mut self, mut i: usize, d: f32) {
        i += 1;
        while i < self.0.len() {
            self.0[i] += d;
            i += i & i.wrapping_neg();
        }
    }
    fn prefix(&self, mut i: usize) -> f32 {
        let mut s = 0.0;
        while i > 0 {
            s += self.0[i];
            i -= i & i.wrapping_neg();
        }
        s
    }
}

fn auto_height(rows: usize) {
    use taffy::prelude::*;
    // Heights are whole pixels, so Fenwick sums are exact in f32.
    let cell = |h: f32| Style {
        size: Size {
            width: length(100.0),
            height: length(h),
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
            height: auto(),
        },
        ..Default::default()
    };

    let mut g: TaffyTree<()> = TaffyTree::new();
    let (mut g_rows, mut g_cells) = (Vec::new(), Vec::new());
    for _ in 0..rows {
        let cs: Vec<NodeId> = (0..4).map(|_| g.new_leaf(cell(24.0)).unwrap()).collect();
        g_cells.push(cs[0]);
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

    let mut sub: Vec<(TaffyTree<()>, NodeId, NodeId)> = Vec::with_capacity(rows);
    let mut fw = Fenwick::new(rows);
    let mut heights = vec![0.0f32; rows];
    for (i, h) in heights.iter_mut().enumerate() {
        let mut t: TaffyTree<()> = TaffyTree::new();
        let cs: Vec<NodeId> = (0..4).map(|_| t.new_leaf(cell(24.0)).unwrap()).collect();
        let r = t.new_with_children(row_style.clone(), &cs).unwrap();
        t.compute_layout(r, Size::MAX_CONTENT).unwrap();
        *h = t.layout(r).unwrap().size.height;
        fw.add(i, *h);
        sub.push((t, r, cs[0]));
    }

    let (mut tg, mut tb) = (Vec::new(), Vec::new());
    for r in 0..RUNS {
        let i = (r * 7919) % rows;
        let h = 24.0 + (r % 5) as f32 * 8.0; // the row's height changes
        let t0 = Instant::now();
        g.set_style(g_cells[i], cell(h)).unwrap();
        g.compute_layout(root, Size::MAX_CONTENT).unwrap();
        tg.push(t0.elapsed());

        let t0 = Instant::now();
        let (t, rid, c0) = &mut sub[i];
        t.set_style(*c0, cell(h)).unwrap();
        t.compute_layout(*rid, Size::MAX_CONTENT).unwrap();
        let nh = t.layout(*rid).unwrap().size.height;
        if nh != heights[i] {
            fw.add(i, nh - heights[i]);
            heights[i] = nh;
        }
        tb.push(t0.elapsed());

        // Sample: the changed row, the next one, the last one.
        for j in [i, (i + 1).min(rows - 1), rows - 1] {
            let gl = g.layout(g_rows[j]).unwrap();
            assert_eq!(gl.location.y, fw.prefix(j), "y of row {j}");
            assert_eq!(gl.size.height, heights[j], "height of row {j}");
        }
    }
    for (j, row) in g_rows.iter().enumerate() {
        assert_eq!(
            g.layout(*row).unwrap().location.y,
            fw.prefix(j),
            "final y of row {j}"
        );
    }
    println!(
        "-- auto-height rows behind a boundary, {rows} rows ({} nodes), one row's height changes",
        g.total_node_count()
    );
    stats("global tree", tg);
    stats("one tree per row + Fenwick offsets", tb);
}

fn main() {
    println!(
        "U1-B round 2 part 2b | runs={RUNS} | {} {} | cpus={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, |n| n.get())
    );
    for n in [1_000, 10_000] {
        keyed_incremental(n);
    }
    auto_height(2_000);
}
