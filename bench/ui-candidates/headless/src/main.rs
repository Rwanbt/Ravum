//! U1-B, premier tour : coûts CPU **headless** (sans GPU ni écran) des
//! candidats pour la mise en page, le texte et une frame d'UI immédiate.
//! Ces mesures valent pour la machine et le commit qui les produisent ; ce
//! ne sont pas des mesures de rendu GPU.

use std::hint::black_box;
use std::time::{Duration, Instant};

const RUNS: usize = 30;

fn stats(mut v: Vec<Duration>) -> String {
    v.sort();
    let q = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
    format!("p50={:?} p95={:?} max={:?}", q(0.5), q(0.95), v[v.len() - 1])
}

fn time<F: FnMut()>(mut f: F) -> Vec<Duration> {
    f(); // warm-up
    (0..RUNS)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .collect()
}

/// Taffy: a column of `rows` flex rows of 4 cells each (≈ 5 nodes per row).
fn taffy_bench(rows: usize) {
    use taffy::prelude::*;
    let mut tree: TaffyTree<()> = TaffyTree::new();
    let cell = Style {
        flex_grow: 1.0,
        size: Size { width: auto(), height: length(24.0) },
        ..Default::default()
    };
    let mut row_ids = Vec::with_capacity(rows);
    let mut first_cell = None;
    for _ in 0..rows {
        let cells: Vec<NodeId> = (0..4).map(|_| tree.new_leaf(cell.clone()).unwrap()).collect();
        first_cell.get_or_insert(cells[0]);
        let row = tree
            .new_with_children(
                Style {
                    flex_direction: FlexDirection::Row,
                    gap: Size { width: length(8.0), height: zero() },
                    ..Default::default()
                },
                &cells,
            )
            .unwrap();
        row_ids.push(row);
    }
    let root = tree
        .new_with_children(
            Style {
                flex_direction: FlexDirection::Column,
                size: Size { width: length(1280.0), height: auto() },
                ..Default::default()
            },
            &row_ids,
        )
        .unwrap();
    let nodes = tree.total_node_count();
    let full = time(|| {
        tree.mark_dirty(root).unwrap();
        for r in &row_ids {
            tree.mark_dirty(*r).unwrap();
        }
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    });
    let leaf = first_cell.unwrap();
    let incremental = time(|| {
        tree.mark_dirty(leaf).unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    });
    black_box(tree.layout(root).unwrap().size);
    println!("taffy nodes={nodes} full: {}", stats(full));
    println!("taffy nodes={nodes} one-leaf-dirty: {}", stats(incremental));
}

/// Parley: shape and break `n` lines of mixed text at 300 px.
fn parley_bench(n: usize) {
    use parley::{FontContext, Layout, LayoutContext, StyleProperty};
    let mut fcx = FontContext::new();
    let mut lcx: LayoutContext<()> = LayoutContext::new();
    let line = "Ravum UI — mise en page déterministe, naïve façade, 100 % natif. ";
    let text: String = line.repeat(n);
    let v = time(|| {
        let mut b = lcx.ranged_builder(&mut fcx, &text, 1.0, true);
        b.push_default(StyleProperty::FontSize(16.0));
        let mut layout: Layout<()> = b.build(&text);
        layout.break_all_lines(Some(300.0));
        black_box(layout.height());
    });
    println!("parley chars={} width=300: {}", text.chars().count(), stats(v));
}

/// egui: one frame (run + tessellate) of `n` labelled buttons in a scroll area.
fn egui_bench(n: usize) {
    let ctx = egui::Context::default();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };
    let mut frame = || {
        let out = ctx.run_ui(input(), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for i in 0..n {
                    ui.horizontal(|ui| {
                        ui.label(format!("item {i}"));
                        let _ = ui.button("ok");
                    });
                }
            });
        });
        let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
        black_box(prims.len());
    };
    let v = time(&mut frame);
    println!("egui widgets={} frame(run+tessellate): {}", n * 2, stats(v));
}

fn main() {
    println!(
        "U1-B headless | runs={RUNS} | {} {} | cpus={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, |n| n.get())
    );
    for rows in [200, 2_000] {
        taffy_bench(rows);
    }
    for n in [10, 200] {
        parley_bench(n);
    }
    for n in [100, 1_000] {
        egui_bench(n);
    }
}
