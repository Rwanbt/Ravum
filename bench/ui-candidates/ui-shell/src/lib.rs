//! U2 : la partie de la coquille qui ne demande ni fenêtre ni GPU, donc
//! testable partout.
//!
//! - [`paint`] : liste d'ordres de dessin tirée de l'arbre placé par
//!   `ui-core` (fonds, cadre de focus, texte), avec le découpage des
//!   conteneurs défilants. Le rendu (Vello) ne fait que l'exécuter.
//! - [`App0`] : l'application de qualification 0 (PLAN §6) : compteur et
//!   liste de tâches, pilotée par les clics que `ui-core` rapporte.

use ui_core::{Desc, Dir, Len, Path, Rect, Style, Ui};

/// Un ordre de dessin, en pixels logiques.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// Fond d'un conteneur ; `level` = profondeur (teinte).
    Fill { rect: Rect, level: usize },
    /// Cadre du nœud qui a le focus clavier.
    Focus { rect: Rect },
    /// Texte, découpé par `clip` (intersection des ancêtres).
    Text {
        at: (f32, f32),
        clip: Rect,
        text: String,
    },
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    (x1 > x0 && y1 > y0).then_some(Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// Ordres de dessin de l'arbre, parents avant enfants, frères dans l'ordre
/// de la description. Un nœud entièrement hors de ses ancêtres (défilé hors
/// de vue) n'est pas dessiné ; un nœud en partie visible est découpé.
pub fn paint(ui: &Ui) -> Vec<Op> {
    let (vw, vh) = ui.viewport();
    let screen = Rect {
        x: 0.0,
        y: 0.0,
        w: vw,
        h: vh,
    };
    let mut out = Vec::new();
    if let Some(root) = ui.find(&[]).or_else(|| first_root(ui)) {
        walk(ui, root, &mut Vec::new(), screen, 0, &mut out);
    }
    out
}

fn first_root(ui: &Ui) -> Option<&Desc> {
    let p = ui.rects().keys().next()?;
    ui.find(&p[..1])
}

fn walk(ui: &Ui, d: &Desc, p: &mut Path, clip: Rect, level: usize, out: &mut Vec<Op>) {
    p.push(d.key);
    if let Some(r) = ui.rect(p) {
        if let Some(vis) = intersect(r, clip) {
            match &d.text {
                Some(t) => out.push(Op::Text {
                    at: (r.x, r.y),
                    clip: vis,
                    text: t.clone(),
                }),
                None => out.push(Op::Fill { rect: vis, level }),
            }
            if ui.focus() == Some(p) {
                out.push(Op::Focus { rect: vis });
            }
            for c in &d.children {
                walk(ui, c, p, vis, level + 1, out);
            }
        }
    }
    p.pop();
}

/// Clés stables de l'application 0.
pub mod key {
    pub const ROOT: u64 = 1;
    pub const BAR: u64 = 2;
    pub const COUNT: u64 = 3;
    pub const PLUS: u64 = 4;
    pub const MINUS: u64 = 5;
    pub const ADD: u64 = 6;
    pub const LIST: u64 = 7;
    /// Tâche `i` : `TASK + i` ; clé stable à travers les ajouts et retraits.
    pub const TASK: u64 = 1_000;
}

/// Application de qualification 0 : compteur et liste de tâches.
#[derive(Clone, Debug, PartialEq)]
pub struct App0 {
    pub count: i64,
    /// (identifiant stable, libellé, faite).
    pub tasks: Vec<(u64, String, bool)>,
    next: u64,
}

impl Default for App0 {
    fn default() -> Self {
        let mut a = App0 {
            count: 0,
            tasks: Vec::new(),
            next: 0,
        };
        for _ in 0..30 {
            a.add_task();
        }
        a
    }
}

fn button(key: u64, label: &str) -> Desc {
    Desc::new(
        key,
        Style {
            width: Len::Fixed(96.0),
            height: Len::Fixed(32.0),
            padding: 8.0,
            focusable: true,
            ..Style::default()
        },
    )
    .with(vec![Desc::text(1, label)])
}

impl App0 {
    pub fn add_task(&mut self) {
        let id = self.next;
        self.next += 1;
        self.tasks.push((id, format!("tâche {id}"), false));
    }

    /// La vue : une fonction pure de l'état.
    pub fn view(&self) -> Desc {
        let bar = Desc::new(
            key::BAR,
            Style {
                dir: Dir::Row,
                width: Len::Fill(1.0),
                height: Len::Fixed(48.0),
                padding: 8.0,
                gap: 8.0,
                ..Style::default()
            },
        )
        .with(vec![
            Desc::new(
                key::COUNT,
                Style {
                    width: Len::Fixed(160.0),
                    height: Len::Fixed(32.0),
                    padding: 8.0,
                    ..Style::default()
                },
            )
            .with(vec![Desc::text(1, &format!("compteur : {}", self.count))]),
            button(key::PLUS, "+1"),
            button(key::MINUS, "-1"),
            button(key::ADD, "ajouter"),
        ]);
        let rows = self
            .tasks
            .iter()
            .map(|(id, label, done)| {
                Desc::new(
                    key::TASK + id,
                    Style {
                        width: Len::Fill(1.0),
                        height: Len::Fixed(28.0),
                        padding: 6.0,
                        focusable: true,
                        ..Style::default()
                    },
                )
                .with(vec![Desc::text(
                    1,
                    &format!("[{}] {label}", if *done { "x" } else { " " }),
                )])
            })
            .collect();
        let list = Desc::new(
            key::LIST,
            Style {
                width: Len::Fill(1.0),
                height: Len::Fill(1.0),
                padding: 8.0,
                gap: 4.0,
                scroll: true,
                ..Style::default()
            },
        )
        .with(rows);
        Desc::new(
            key::ROOT,
            Style {
                width: Len::Fill(1.0),
                height: Len::Fill(1.0),
                ..Style::default()
            },
        )
        .with(vec![bar, list])
    }

    /// Applique les clics rapportés par `ui-core` ; `true` si l'état change.
    pub fn on_clicks(&mut self, clicks: &[Path]) -> bool {
        let mut changed = false;
        for p in clicks {
            let hit = |k: u64| p.get(2) == Some(&k) && p.get(1) == Some(&key::BAR);
            if hit(key::PLUS) {
                self.count += 1;
                changed = true;
            } else if hit(key::MINUS) {
                self.count -= 1;
                changed = true;
            } else if hit(key::ADD) {
                self.add_task();
                changed = true;
            } else if p.get(1) == Some(&key::LIST) {
                if let Some(k) = p.get(2) {
                    if let Some(t) = self.tasks.iter_mut().find(|t| key::TASK + t.0 == *k) {
                        t.2 = !t.2;
                        changed = true;
                    }
                }
            }
        }
        changed
    }
}

/// Une boucle d'application sans fenêtre : événement → `ui-core` → clics →
/// état → nouvelle vue → patch. La fenêtre fait la même chose.
pub struct Shell {
    pub app: App0,
    pub ui: Ui,
}

impl Shell {
    pub fn new(viewport: (f32, f32)) -> Shell {
        let app = App0::default();
        let ui = Ui::new(app.view(), viewport).expect("app 0 view is valid");
        Shell { app, ui }
    }

    /// `true` si l'écran doit être redessiné.
    pub fn event(&mut self, e: &ui_core::Event) -> bool {
        let before = (self.ui.focus().cloned(), scroll_of(&self.ui));
        self.ui.dispatch(e);
        let clicks = self.ui.take_clicks();
        let mut redraw = before != (self.ui.focus().cloned(), scroll_of(&self.ui));
        if self.app.on_clicks(&clicks) {
            let g = self.ui.generation();
            // Fail-closed : une vue refusée laisse l'arbre actif intact.
            if self.ui.patch(g, self.app.view()).is_ok() {
                redraw = true;
            }
        }
        redraw
    }

    pub fn resize(&mut self, viewport: (f32, f32)) {
        self.ui.resize(viewport);
    }
}

fn scroll_of(ui: &Ui) -> f32 {
    ui.state(&[key::ROOT, key::LIST]).map_or(0.0, |s| s.scroll)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui_core::Event;

    fn center(ui: &Ui, p: &[u64]) -> (f32, f32) {
        let r = ui.rect(p).unwrap();
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    fn click(s: &mut Shell, p: &[u64]) -> bool {
        let (x, y) = center(&s.ui, p);
        s.event(&Event::Click { x, y })
    }

    #[test]
    fn the_view_passes_verify_on_every_size() {
        for (w, h) in [(1280.0, 800.0), (800.0, 600.0), (640.0, 480.0)] {
            let s = Shell::new((w, h));
            assert_eq!(s.ui.verify(), vec![], "{w}x{h}");
        }
    }

    #[test]
    fn buttons_change_the_counter_and_keep_identity() {
        let mut s = Shell::new((1280.0, 800.0));
        let plus = [key::ROOT, key::BAR, key::PLUS];
        assert!(click(&mut s, &plus));
        assert!(click(&mut s, &plus));
        assert!(click(&mut s, &[key::ROOT, key::BAR, key::MINUS]));
        assert_eq!(s.app.count, 1);
        // The clicked button still has the focus after the patches.
        assert_eq!(s.ui.focus(), Some(&vec![key::ROOT, key::BAR, key::MINUS]));
        assert!(paint(&s.ui)
            .iter()
            .any(|o| matches!(o, Op::Text { text, .. } if text == "compteur : 1")));
    }

    #[test]
    fn tasks_toggle_add_and_scroll_keeps_state() {
        let mut s = Shell::new((1280.0, 800.0));
        let t0 = [key::ROOT, key::LIST, key::TASK];
        assert!(click(&mut s, &t0));
        assert!(s.app.tasks[0].2);
        let (x, y) = center(&s.ui, &[key::ROOT, key::LIST]);
        assert!(s.event(&Event::Scroll { x, y, dy: 200.0 }));
        assert!(click(&mut s, &[key::ROOT, key::BAR, key::ADD]));
        assert_eq!(s.app.tasks.len(), 31);
        // The scroll position survived the patch that added a task.
        assert_eq!(s.ui.state(&[key::ROOT, key::LIST]).unwrap().scroll, 200.0);
        assert!(s.app.tasks[0].2);
    }

    #[test]
    fn scrolled_out_rows_are_not_painted_and_partial_rows_are_clipped() {
        let mut s = Shell::new((640.0, 300.0));
        let list = s.ui.rect(&[key::ROOT, key::LIST]).unwrap();
        let (x, y) = center(&s.ui, &[key::ROOT, key::LIST]);
        s.event(&Event::Scroll { x, y, dy: 45.0 });
        let ops = paint(&s.ui);
        let texts: Vec<_> = ops
            .iter()
            .filter_map(|o| match o {
                Op::Text { text, clip, .. } if text.contains("tâche") => {
                    Some((text.clone(), *clip))
                }
                _ => None,
            })
            .collect();
        assert!(
            !texts.iter().any(|(t, _)| t == "[ ] tâche 0"),
            "row 0 is scrolled out"
        );
        assert!(texts.len() < 30, "virtual: only visible rows");
        for (t, c) in &texts {
            assert!(
                c.y >= list.y - 1e-3 && c.y + c.h <= list.y + list.h + 1e-3,
                "{t} {c:?}"
            );
        }
    }

    #[test]
    fn clicks_outside_any_node_and_on_text_only_areas_change_nothing() {
        let mut s = Shell::new((1280.0, 800.0));
        let before = s.app.clone();
        s.event(&Event::Click { x: -5.0, y: -5.0 });
        s.event(&Event::Click { x: 1270.0, y: 20.0 });
        let (x, y) = center(&s.ui, &[key::ROOT, key::BAR, key::COUNT]);
        s.event(&Event::Click { x, y });
        assert_eq!(s.app, before);
    }

    #[test]
    fn tab_cycles_focus_and_paint_marks_it() {
        let mut s = Shell::new((1280.0, 800.0));
        assert!(s.event(&Event::FocusNext));
        assert_eq!(s.ui.focus(), Some(&vec![key::ROOT, key::BAR, key::PLUS]));
        assert_eq!(
            paint(&s.ui)
                .iter()
                .filter(|o| matches!(o, Op::Focus { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn a_thousand_events_are_deterministic() {
        let run = || {
            let mut s = Shell::new((1024.0, 700.0));
            let mut rng = 7u64;
            for _ in 0..1000 {
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let x = (rng >> 33) as f32 % 1024.0;
                let y = (rng >> 13) as f32 % 700.0;
                let e = match rng % 3 {
                    0 => Event::Click { x, y },
                    1 => Event::Scroll {
                        x,
                        y,
                        dy: ((rng >> 7) % 60) as f32 - 30.0,
                    },
                    _ => Event::FocusNext,
                };
                s.event(&e);
            }
            (s.app.clone(), paint(&s.ui))
        };
        assert_eq!(run(), run());
    }
}
