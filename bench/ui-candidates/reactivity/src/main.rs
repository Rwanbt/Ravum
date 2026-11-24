//! U1-B, tour 2 (UI-D13, UI-D16).
//!
//! 1. Réactivité : A (reconstruire la vue puis diff) contre B (dépendances
//!    granulaires), sur la même scène et le même changement. Les deux
//!    formatent le même texte pour les lignes qu'ils produisent ; seul le
//!    nombre de lignes produites diffère, ce qui est l'objet de la mesure.
//! 2. Taffy : de **vrais** changements de propriétés (taille, structure),
//!    avec et sans hauteur fixe des lignes (frontière de layout candidate).
//!
//! Mesures CPU headless, valables pour la machine et le commit cités.

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

// ---------- 1. Réactivité ----------

#[derive(Clone, PartialEq)]
enum Node {
    Label(String),
    Slider { id: u32, value: f32 },
}

struct Model {
    params: Vec<f32>,
}

fn row(i: usize, v: f32) -> [Node; 2] {
    [
        Node::Label(format!("param {i}: {v:.3}")),
        Node::Slider { id: i as u32, value: v },
    ]
}

/// A: rebuild rows `range` and diff against `old` (same layout: 2 nodes per row).
fn rebuild_and_diff(m: &Model, range: std::ops::Range<usize>, old: &mut Vec<Node>) -> usize {
    let mut new = Vec::with_capacity(range.len() * 2);
    for i in range {
        new.extend(row(i, m.params[i]));
    }
    let mut patches = 0;
    for (a, b) in old.iter_mut().zip(new.iter()) {
        if a != b {
            *a = b.clone();
            patches += 1;
        }
    }
    patches
}

/// B: the tree is kept; `deps[key]` lists the rows reading `params[key]`.
struct Granular {
    nodes: Vec<Node>,
    deps: Vec<Vec<u32>>,
}

impl Granular {
    fn new(m: &Model) -> Granular {
        let mut nodes = Vec::with_capacity(m.params.len() * 2);
        let mut deps = Vec::with_capacity(m.params.len());
        for (i, v) in m.params.iter().enumerate() {
            nodes.extend(row(i, *v));
            deps.push(vec![i as u32]);
        }
        Granular { nodes, deps }
    }
    fn set(&mut self, m: &mut Model, key: usize, v: f32) -> usize {
        m.params[key] = v;
        let mut patches = 0;
        for &r in &self.deps[key] {
            let r = r as usize;
            let [l, s] = row(r, m.params[r]);
            self.nodes[2 * r] = l;
            self.nodes[2 * r + 1] = s;
            patches += 2;
        }
        patches
    }
}

fn reactivity(n: usize) {
    let mut m = Model { params: (0..n).map(|i| i as f32 * 0.001).collect() };
    let mut tree: Vec<Node> = Vec::new();
    for i in 0..n {
        tree.extend(row(i, m.params[i]));
    }
    let visible = 50.min(n);
    let mut window: Vec<Node> = tree[..visible * 2].to_vec();
    let mut g = Granular::new(&m);

    let mut a_full = Vec::with_capacity(RUNS);
    let mut a_virt = Vec::with_capacity(RUNS);
    let mut b = Vec::with_capacity(RUNS);
    for r in 0..RUNS {
        let key = r % visible; // a visible parameter, as a slider drag would be
        let v = r as f32 * 0.5;
        m.params[key] = v;
        let t = Instant::now();
        black_box(rebuild_and_diff(&m, 0..n, &mut tree));
        a_full.push(t.elapsed());
        let t = Instant::now();
        black_box(rebuild_and_diff(&m, 0..visible, &mut window));
        a_virt.push(t.elapsed());
        let t = Instant::now();
        black_box(g.set(&mut m, key, v));
        b.push(t.elapsed());
        assert!(tree[2 * key] == g.nodes[2 * key], "A and B disagree");
    }
    let deps_bytes: usize = g.deps.iter().map(|d| 24 + d.capacity() * 4).sum();
    println!("-- reactivity, {n} rows ({} nodes), one parameter changed per update", n * 2);
    stats("A rebuild+diff, all rows", a_full);
    stats("A rebuild+diff, 50 visible rows (virtualized)", a_virt);
    stats("B granular (deps), one row", b);
    println!("B dependency table: {deps_bytes} bytes ({} per row)", deps_bytes / n);
}

// ---------- 2. Taffy : vrais changements ----------

fn taffy_real(rows: usize, fixed_row_height: bool) {
    use taffy::prelude::*;
    let mut tree: TaffyTree<()> = TaffyTree::new();
    let cell = |w: f32| Style {
        size: Size { width: length(w), height: length(24.0) },
        ..Default::default()
    };
    let row_style = Style {
        flex_direction: FlexDirection::Row,
        gap: Size { width: length(8.0), height: zero() },
        size: Size {
            width: auto(),
            height: if fixed_row_height { length(32.0) } else { auto() },
        },
        ..Default::default()
    };
    let mut rows_ids = Vec::new();
    let mut cells0 = Vec::new();
    for _ in 0..rows {
        let cs: Vec<NodeId> = (0..4).map(|_| tree.new_leaf(cell(100.0)).unwrap()).collect();
        cells0.push(cs[0]);
        rows_ids.push(tree.new_with_children(row_style.clone(), &cs).unwrap());
    }
    let root = tree
        .new_with_children(
            Style {
                flex_direction: FlexDirection::Column,
                size: Size { width: length(1280.0), height: auto() },
                ..Default::default()
            },
            &rows_ids,
        )
        .unwrap();
    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    let label = if fixed_row_height { "fixed-height rows" } else { "auto-height rows" };

    let mut resize = Vec::new();
    for r in 0..RUNS {
        let leaf = cells0[r % rows];
        let w = 100.0 + (r % 7) as f32;
        let t = Instant::now();
        tree.set_style(leaf, cell(w)).unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        resize.push(t.elapsed());
        assert_eq!(tree.layout(leaf).unwrap().size.width, w);
    }
    stats(&format!("taffy {label}, {} nodes, resize one cell", tree.total_node_count()), resize);

    let mut insert = Vec::new();
    for r in 0..RUNS {
        let parent = rows_ids[r % rows];
        let t = Instant::now();
        let c = tree.new_leaf(cell(50.0)).unwrap();
        tree.add_child(parent, c).unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        insert.push(t.elapsed());
        tree.remove_child(parent, c).unwrap();
        tree.remove(c).unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    }
    stats(&format!("taffy {label}, insert one cell (structure)"), insert);

    let mut full = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        for id in &rows_ids {
            tree.mark_dirty(*id).unwrap();
        }
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        full.push(t.elapsed());
    }
    stats(&format!("taffy {label}, full relayout"), full);
}

fn main() {
    println!(
        "U1-B round 2 | runs={RUNS} | {} {} | cpus={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, |n| n.get())
    );
    for n in [1_000, 10_000] {
        reactivity(n);
    }
    for rows in [2_000] {
        taffy_real(rows, false);
        taffy_real(rows, true);
    }
}
