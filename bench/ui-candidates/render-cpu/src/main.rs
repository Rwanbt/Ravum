//! U1-B, tour 2, partie 3 : rendu CPU, `vello_cpu` contre `tiny-skia`.
//!
//! La même scène (géométrie produite une seule fois par `kurbo`, puis donnée
//! aux deux moteurs) est rendue à 1280 × 800, anticrénelée, en RGBA8
//! prémultiplié, mono-thread :
//! - « liste » : 50 lignes visibles d'une liste virtualisée (fond arrondi
//!   alterné, piste et curseur de slider, bordure), environ 200 formes ;
//! - « dense » : 5 000 petites formes arrondies qui se chevauchent.
//!
//! Mesures : temps par frame (p50, p95, p99), allocations et octets alloués
//! par frame (allocateur compteur), pic de mémoire résidente du processus.
//! Contrôle : les deux images doivent être proches (écart absolu moyen par
//! canal borné, l'anticrénelage différant) et non vides.
//! Texte exclu : son rendu dépend du shaping et des polices (tour suivant).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};
use vello_cpu::kurbo::{BezPath, Circle, PathEl, Rect, RoundedRect, Shape};

struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(l.size() as u64, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(n as u64, Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

const W: u16 = 1280;
const H: u16 = 800;
const RUNS: usize = 100;

/// One filled shape: geometry and a straight (non-premultiplied) RGBA8 color.
struct Item {
    path: BezPath,
    rgba: [u8; 4],
}

fn list_scene() -> Vec<Item> {
    let mut v = vec![Item { path: Rect::new(0.0, 0.0, W as f64, H as f64).to_path(0.1), rgba: [24, 26, 30, 255] }];
    for r in 0..50 {
        let y = r as f64 * 16.0;
        let bg = if r % 2 == 0 { [36, 39, 45, 255] } else { [30, 33, 38, 255] };
        v.push(Item { path: RoundedRect::new(4.0, y + 1.0, W as f64 - 4.0, y + 15.0, 4.0).to_path(0.1), rgba: bg });
        v.push(Item { path: RoundedRect::new(600.0, y + 6.0, 1200.0, y + 10.0, 2.0).to_path(0.1), rgba: [80, 86, 96, 255] });
        let x = 600.0 + (r as f64 * 37.0) % 600.0;
        v.push(Item { path: Circle::new((x, y + 8.0), 5.5).to_path(0.1), rgba: [90, 160, 250, 255] });
        v.push(Item { path: Rect::new(4.0, y + 15.0, W as f64 - 4.0, y + 16.0).to_path(0.1), rgba: [255, 255, 255, 20] });
    }
    v
}

fn dense_scene() -> Vec<Item> {
    let mut v = vec![Item { path: Rect::new(0.0, 0.0, W as f64, H as f64).to_path(0.1), rgba: [255, 255, 255, 255] }];
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15; // fixed seed: the same scene every run
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..5_000 {
        let (x, y) = (rnd() * (W as f64 - 40.0), rnd() * (H as f64 - 40.0));
        let (w, h) = (8.0 + rnd() * 32.0, 8.0 + rnd() * 32.0);
        let c = [(rnd() * 255.0) as u8, (rnd() * 255.0) as u8, (rnd() * 255.0) as u8, 160];
        v.push(Item { path: RoundedRect::new(x, y, x + w, y + h, 3.0).to_path(0.1), rgba: c });
    }
    v
}

fn to_skia(p: &BezPath) -> tiny_skia::Path {
    let mut b = tiny_skia::PathBuilder::new();
    for el in p.elements() {
        match *el {
            PathEl::MoveTo(a) => b.move_to(a.x as f32, a.y as f32),
            PathEl::LineTo(a) => b.line_to(a.x as f32, a.y as f32),
            PathEl::QuadTo(a, c) => b.quad_to(a.x as f32, a.y as f32, c.x as f32, c.y as f32),
            PathEl::CurveTo(a, c, d) => {
                b.cubic_to(a.x as f32, a.y as f32, c.x as f32, c.y as f32, d.x as f32, d.y as f32)
            }
            PathEl::ClosePath => b.close(),
        }
    }
    b.finish().expect("non-empty path")
}

fn stats(name: &str, mut v: Vec<Duration>) {
    v.sort();
    let q = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
    println!("{name}: p50={:?} p95={:?} p99={:?} max={:?}", q(0.5), q(0.95), q(0.99), v[v.len() - 1]);
}

fn counted<T>(f: impl FnOnce() -> T) -> (T, u64, u64) {
    let (a, b) = (ALLOCS.load(Relaxed), BYTES.load(Relaxed));
    let r = f();
    (r, ALLOCS.load(Relaxed) - a, BYTES.load(Relaxed) - b)
}

/// Renders with vello_cpu into `out` (RGBA8 premultiplied).
fn vello(scene: &[Item], ctx: &mut vello_cpu::RenderContext, res: &mut vello_cpu::Resources, pm: &mut vello_cpu::Pixmap) {
    use vello_cpu::color::AlphaColor;
    ctx.reset();
    for it in scene {
        let [r, g, b, a] = it.rgba;
        ctx.set_paint(AlphaColor::from_rgba8(r, g, b, a));
        ctx.fill_path(&it.path);
    }
    ctx.flush();
    ctx.render(&mut *pm, res);
}

fn skia(scene: &[(tiny_skia::Path, [u8; 4])], pm: &mut tiny_skia::Pixmap) {
    pm.fill(tiny_skia::Color::TRANSPARENT);
    let mut paint = tiny_skia::Paint { anti_alias: true, ..Default::default() };
    for (p, [r, g, b, a]) in scene {
        paint.set_color_rgba8(*r, *g, *b, *a);
        pm.fill_path(p, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
    }
}

fn peak_rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM:")).map(|l| l.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0)))
        .unwrap_or(0)
}

fn bench(name: &str, scene: Vec<Item>) {
    let shapes = scene.len();
    let sk: Vec<(tiny_skia::Path, [u8; 4])> = scene.iter().map(|i| (to_skia(&i.path), i.rgba)).collect();
    let mut ctx = vello_cpu::RenderContext::new(W, H);
    let mut res = vello_cpu::Resources::new();
    let mut vpm = vello_cpu::Pixmap::new(W, H);
    let mut spm = tiny_skia::Pixmap::new(W as u32, H as u32).unwrap();
    // Warm-up (first-frame allocations are reported separately).
    let (_, va0, vb0) = counted(|| vello(&scene, &mut ctx, &mut res, &mut vpm));
    let (_, sa0, sb0) = counted(|| skia(&sk, &mut spm));
    let (mut tv, mut ts) = (Vec::new(), Vec::new());
    let (mut va, mut vb, mut sa, mut sb) = (0, 0, 0, 0);
    for _ in 0..RUNS {
        let t = Instant::now();
        let (_, a, b) = counted(|| vello(&scene, &mut ctx, &mut res, &mut vpm));
        tv.push(t.elapsed());
        (va, vb) = (va + a, vb + b);
        let t = Instant::now();
        let (_, a, b) = counted(|| skia(&sk, &mut spm));
        ts.push(t.elapsed());
        (sa, sb) = (sa + a, sb + b);
    }
    // Same picture: mean absolute difference per channel, and not blank.
    let (v8, s8) = (vpm.data_as_u8_slice(), spm.data());
    assert_eq!(v8.len(), s8.len());
    let diff: u64 = v8.iter().zip(s8).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64).sum();
    let mad = diff as f64 / v8.len() as f64;
    let lit = v8.chunks(4).filter(|p| p[3] != 0).count();
    assert!(lit > (W as usize * H as usize) / 2, "{name}: vello picture is mostly empty");
    assert!(mad < 2.0, "{name}: the two renderers differ too much (mean abs diff {mad:.3})");
    println!("-- {name}: {shapes} filled shapes, {W}x{H}, anti-aliased, single thread");
    stats("vello_cpu 0.4.0 frame", tv);
    stats("tiny-skia 0.12.0 frame", ts);
    println!(
        "allocations per frame (steady): vello_cpu {:.1} ({} B), tiny-skia {:.1} ({} B); first frame: vello_cpu {va0} ({vb0} B), tiny-skia {sa0} ({sb0} B)",
        va as f64 / RUNS as f64,
        vb / RUNS as u64,
        sa as f64 / RUNS as f64,
        sb / RUNS as u64
    );
    println!("pixel check: mean abs diff per channel {mad:.4} (bound 2.0)");
}

fn main() {
    println!(
        "U1-B round 2 part 3 (CPU render) | runs={RUNS} | {} {} | cpus={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, |n| n.get())
    );
    bench("list (50 visible rows)", list_scene());
    bench("dense (5000 overlapping shapes)", dense_scene());
    println!("peak RSS of the process: {} KiB", peak_rss_kib());
}
