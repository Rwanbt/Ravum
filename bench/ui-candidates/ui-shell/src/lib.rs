//! U2 : la partie de la coquille qui ne demande ni fenêtre ni GPU, donc
//! testable partout.
//!
//! - [`paint`] : liste d'ordres de dessin tirée de l'arbre placé par
//!   `ui-core` (fonds, cadre de focus, texte), avec le découpage des
//!   conteneurs défilants. Le rendu (Vello) ne fait que l'exécuter.
//! - [`App0`] : l'application de qualification 0 (PLAN §6) : compteur et
//!   liste de tâches, pilotée par les clics que `ui-core` rapporte.

use ui_core::hot::{classify, Level};
use ui_core::{verify, Desc, Dir, Len, Path, Rect, Style, Ui};

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

/// Habillage rechargeable à chaud de l'application 0 (U3, niveaux L0 et
/// L1) : dimensions et libellés, lus depuis un texte `clé = valeur`.
/// L'état de l'application n'en fait pas partie et n'est jamais touché.
#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    pub bar_height: f32,
    pub bar_gap: f32,
    pub button_width: f32,
    pub button_height: f32,
    pub row_height: f32,
    pub list_gap: f32,
    pub label_count: String,
    pub label_plus: String,
    pub label_minus: String,
    pub label_add: String,
}

impl Default for Skin {
    fn default() -> Self {
        Skin {
            bar_height: 48.0,
            bar_gap: 8.0,
            button_width: 96.0,
            button_height: 32.0,
            row_height: 28.0,
            list_gap: 4.0,
            label_count: "compteur".into(),
            label_plus: "+1".into(),
            label_minus: "-1".into(),
            label_add: "ajouter".into(),
        }
    }
}

/// Erreur de lecture d'un habillage ; la ligne est comptée à partir de 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkinError {
    Syntax {
        line: usize,
    },
    UnknownKey {
        line: usize,
        key: String,
    },
    DuplicateKey {
        line: usize,
        key: String,
    },
    /// Nombre non fini, négatif, ou hors de la plage admise.
    BadNumber {
        line: usize,
        key: String,
    },
    /// Libellé vide ou contenant un caractère de contrôle.
    BadLabel {
        line: usize,
        key: String,
    },
}

/// Bornes des dimensions (pixels logiques) : au-delà, c'est une faute de
/// frappe, pas un réglage.
const MAX_LEN: f32 = 4096.0;

impl Skin {
    /// Lecture fail-closed : la moindre ligne fautive refuse tout le
    /// texte. Une clé absente garde sa valeur par défaut ; `#` commente.
    pub fn parse(text: &str) -> Result<Skin, SkinError> {
        let mut skin = Skin::default();
        let mut seen: Vec<&str> = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = i + 1;
            let l = raw.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let (k, v) = l.split_once('=').ok_or(SkinError::Syntax { line })?;
            let (k, v) = (k.trim(), v.trim());
            if seen.contains(&k) {
                return Err(SkinError::DuplicateKey {
                    line,
                    key: k.into(),
                });
            }
            let num = |slot: &mut f32| -> Result<(), SkinError> {
                match v.parse::<f32>() {
                    Ok(n) if n.is_finite() && (0.0..=MAX_LEN).contains(&n) => {
                        *slot = n;
                        Ok(())
                    }
                    _ => Err(SkinError::BadNumber {
                        line,
                        key: k.into(),
                    }),
                }
            };
            let label = |slot: &mut String| -> Result<(), SkinError> {
                if v.is_empty() || v.chars().any(char::is_control) {
                    return Err(SkinError::BadLabel {
                        line,
                        key: k.into(),
                    });
                }
                *slot = v.into();
                Ok(())
            };
            match k {
                "bar.height" => num(&mut skin.bar_height)?,
                "bar.gap" => num(&mut skin.bar_gap)?,
                "button.width" => num(&mut skin.button_width)?,
                "button.height" => num(&mut skin.button_height)?,
                "row.height" => num(&mut skin.row_height)?,
                "list.gap" => num(&mut skin.list_gap)?,
                "label.count" => label(&mut skin.label_count)?,
                "label.plus" => label(&mut skin.label_plus)?,
                "label.minus" => label(&mut skin.label_minus)?,
                "label.add" => label(&mut skin.label_add)?,
                _ => {
                    return Err(SkinError::UnknownKey {
                        line,
                        key: k.into(),
                    })
                }
            }
            seen.push(k);
        }
        Ok(skin)
    }
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

fn button(skin: &Skin, key: u64, label: &str) -> Desc {
    Desc::new(
        key,
        Style {
            width: Len::Fixed(skin.button_width),
            height: Len::Fixed(skin.button_height),
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

    /// La vue : une fonction pure de l'état et de l'habillage.
    pub fn view(&self, skin: &Skin) -> Desc {
        let bar = Desc::new(
            key::BAR,
            Style {
                dir: Dir::Row,
                width: Len::Fill(1.0),
                height: Len::Fixed(skin.bar_height),
                padding: 8.0,
                gap: skin.bar_gap,
                ..Style::default()
            },
        )
        .with(vec![
            Desc::new(
                key::COUNT,
                Style {
                    width: Len::Fixed(160.0),
                    height: Len::Fixed(skin.button_height),
                    padding: 8.0,
                    ..Style::default()
                },
            )
            .with(vec![Desc::text(
                1,
                &format!("{} : {}", skin.label_count, self.count),
            )]),
            button(skin, key::PLUS, &skin.label_plus),
            button(skin, key::MINUS, &skin.label_minus),
            button(skin, key::ADD, &skin.label_add),
        ]);
        let rows = self
            .tasks
            .iter()
            .map(|(id, label, done)| {
                Desc::new(
                    key::TASK + id,
                    Style {
                        width: Len::Fill(1.0),
                        height: Len::Fixed(skin.row_height),
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
                gap: skin.list_gap,
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
    skin: Skin,
}

/// Refus d'un rechargement d'habillage ; l'habillage actif reste en place.
#[derive(Debug, PartialEq)]
pub enum ReloadError {
    Skin(SkinError),
    Patch(ui_core::PatchError),
    /// Le gate Verify refuse : le candidat ajoute des diagnostics.
    Verify(Vec<ui_core::verify::Finding>),
    RulesetTampered,
}

impl Shell {
    pub fn new(viewport: (f32, f32)) -> Shell {
        let app = App0::default();
        let skin = Skin::default();
        let ui = Ui::new(app.view(&skin), viewport).expect("app 0 view is valid");
        Shell { app, ui, skin }
    }

    pub fn skin(&self) -> &Skin {
        &self.skin
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
            if self.ui.patch(g, self.app.view(&self.skin)).is_ok() {
                redraw = true;
            }
        }
        redraw
    }

    /// Rechargement à chaud transactionnel d'un habillage (U3, L0/L1).
    /// Lecture, construction du candidat avec la mesure du texte active,
    /// gate Verify, puis activation ; au moindre refus, rien ne change
    /// (vue, habillage, état, focus, défilement).
    pub fn reload_skin(&mut self, text: &str) -> Result<Level, ReloadError> {
        let skin = Skin::parse(text).map_err(ReloadError::Skin)?;
        let view = self.app.view(&skin);
        let level = classify(&self.app.view(&self.skin), &view);
        if level == Level::None {
            self.skin = skin;
            return Ok(level);
        }
        let mut probe = Ui::new(view.clone(), self.ui.viewport()).map_err(ReloadError::Patch)?;
        if !probe.set_measure(self.ui.measure()) {
            return Err(ReloadError::Patch(ui_core::PatchError::InvalidSize {
                path: Vec::new(),
            }));
        }
        probe.copy_states_from(&self.ui);
        let ctx = verify::Context {
            tokens: None,
            input: ui_core::responsive::Input::Pointer,
        };
        match verify::activation_gate(&self.ui, &probe, &ctx) {
            Ok(()) => {}
            Err(verify::GateError::Regression(f)) => return Err(ReloadError::Verify(f)),
            Err(verify::GateError::Tampered(_)) => return Err(ReloadError::RulesetTampered),
        }
        let g = self.ui.generation();
        self.ui.patch(g, view).map_err(ReloadError::Patch)?;
        self.skin = skin;
        Ok(level)
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
    #[test]
    fn l0_and_l1_reloads_keep_state_focus_and_scroll() {
        let mut s = Shell::new((1024.0, 700.0));
        click(&mut s, &[key::ROOT, key::BAR, key::PLUS]);
        click(&mut s, &[key::ROOT, key::LIST, key::TASK + 2]);
        let (x, y) = center(&s.ui, &[key::ROOT, key::LIST]);
        s.event(&Event::Scroll { x, y, dy: 60.0 });
        let app = s.app.clone();
        let focus = s.ui.focus().cloned();

        assert_eq!(
            s.reload_skin("row.height = 32\nlist.gap = 6"),
            Ok(Level::Style)
        );
        assert_eq!(
            s.ui.rect(&[key::ROOT, key::LIST, key::TASK]).unwrap().h,
            32.0
        );
        assert_eq!(
            s.reload_skin("row.height = 32\nlist.gap = 6\nlabel.add = nouvelle"),
            Ok(Level::Structure)
        );
        assert!(paint(&s.ui)
            .iter()
            .any(|o| matches!(o, Op::Text { text, .. } if text == "nouvelle")));

        assert_eq!(s.app, app, "a reload never touches the app state");
        assert_eq!(s.ui.focus().cloned(), focus);
        assert_eq!(s.ui.state(&[key::ROOT, key::LIST]).unwrap().scroll, 60.0);
        assert_eq!(s.ui.verify(), vec![]);
        // The new skin stays active for the next state change.
        click(&mut s, &[key::ROOT, key::BAR, key::PLUS]);
        assert_eq!(s.app.count, 2);
        assert_eq!(
            s.ui.rect(&[key::ROOT, key::LIST, key::TASK]).unwrap().h,
            32.0
        );
    }

    #[test]
    fn an_identical_skin_is_level_none_and_changes_nothing() {
        let mut s = Shell::new((1024.0, 700.0));
        let g = s.ui.generation();
        assert_eq!(
            s.reload_skin("# rien\n\nbutton.width = 96"),
            Ok(Level::None)
        );
        assert_eq!(s.ui.generation(), g);
    }

    #[test]
    fn bad_skins_are_refused_line_by_line() {
        use SkinError::*;
        let k = |s: &str| s.to_string();
        let cases: Vec<(&str, SkinError)> = vec![
            ("row.height 30", Syntax { line: 1 }),
            (
                "\nrow.heigth = 30",
                UnknownKey {
                    line: 2,
                    key: k("row.heigth"),
                },
            ),
            (
                "bar.gap = 1\nbar.gap = 2",
                DuplicateKey {
                    line: 2,
                    key: k("bar.gap"),
                },
            ),
            (
                "bar.gap = -1",
                BadNumber {
                    line: 1,
                    key: k("bar.gap"),
                },
            ),
            (
                "bar.gap = NaN",
                BadNumber {
                    line: 1,
                    key: k("bar.gap"),
                },
            ),
            (
                "bar.gap = inf",
                BadNumber {
                    line: 1,
                    key: k("bar.gap"),
                },
            ),
            (
                "bar.gap = 1e9",
                BadNumber {
                    line: 1,
                    key: k("bar.gap"),
                },
            ),
            (
                "bar.gap = huit",
                BadNumber {
                    line: 1,
                    key: k("bar.gap"),
                },
            ),
            (
                "label.add =",
                BadLabel {
                    line: 1,
                    key: k("label.add"),
                },
            ),
            (
                "label.add = a\u{7}b",
                BadLabel {
                    line: 1,
                    key: k("label.add"),
                },
            ),
        ];
        for (text, want) in cases {
            assert_eq!(Skin::parse(text), Err(want), "{text:?}");
        }
    }

    #[test]
    fn a_refused_reload_leaves_everything_active() {
        let mut s = Shell::new((1024.0, 700.0));
        click(&mut s, &[key::ROOT, key::BAR, key::PLUS]);
        let (g, app, skin) = (s.ui.generation(), s.app.clone(), s.skin().clone());
        let ops = paint(&s.ui);
        // Parse error, then a skin that parses but makes the labels overflow
        // their buttons: the Verify gate refuses it.
        assert!(matches!(
            s.reload_skin("button.width = x"),
            Err(ReloadError::Skin(_))
        ));
        let e = s.reload_skin("button.width = 12").unwrap_err();
        assert!(
            matches!(&e, ReloadError::Verify(f) if !f.is_empty()),
            "{e:?}"
        );
        assert_eq!((s.ui.generation(), &s.app, s.skin()), (g, &app, &skin));
        assert_eq!(paint(&s.ui), ops);
        // And the next state change still uses the old, valid skin.
        click(&mut s, &[key::ROOT, key::BAR, key::PLUS]);
        assert_eq!(
            s.ui.rect(&[key::ROOT, key::BAR, key::PLUS]).unwrap().w,
            96.0
        );
    }

    /// Textes arbitraires : jamais de panique, et soit un refus sans
    /// effet, soit une vue active qui passe Verify.
    #[test]
    fn adversarial_skins_never_break_the_active_view() {
        let keys = [
            "bar.height",
            "bar.gap",
            "button.width",
            "button.height",
            "row.height",
            "list.gap",
            "label.add",
            "nope",
        ];
        let vals = [
            "0", "1", "8", "40", "200", "4096", "4097", "-0", "1e-3", "", "x", "NaN",
        ];
        let mut s = Shell::new((800.0, 600.0));
        let mut rng = 11u64;
        for _ in 0..400 {
            let mut text = String::new();
            for _ in 0..1 + rng % 3 {
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let k = keys[(rng >> 33) as usize % keys.len()];
                let v = vals[(rng >> 17) as usize % vals.len()];
                text.push_str(&format!("{k} = {v}\n"));
            }
            let before = (s.ui.generation(), s.skin().clone());
            match s.reload_skin(&text) {
                Ok(_) => assert_eq!(s.ui.verify(), vec![], "{text}"),
                Err(_) => assert_eq!((s.ui.generation(), s.skin().clone()), before, "{text}"),
            }
        }
    }
    /// Critère de sortie U3 (PLAN §5) sur l'application réelle : 10 000
    /// événements et 20 éditions d'habillage (acceptées ou refusées) sans
    /// perte. Les clics visent des nœuds par leur chemin, donc un jumeau
    /// sans rechargement doit finir dans le même état, avec le même focus.
    #[test]
    fn ten_thousand_events_and_twenty_edits_lose_nothing() {
        let skins = [
            "row.height = 40\nlist.gap = 10",
            "button.width = 12",
            "label.add = nouvelle tâche\nbutton.width = 140",
            "bar.gap = -1",
            "row.height = 24\nlabel.count = total",
        ];
        let mut a = Shell::new((1024.0, 700.0));
        let mut b = Shell::new((1024.0, 700.0));
        let (mut accepted, mut refused) = (0, 0);
        let mut rng = 3u64;
        for i in 0..10_000u64 {
            if (i + 1).is_multiple_of(500) {
                match a.reload_skin(skins[(i / 500) as usize % skins.len()]) {
                    Ok(_) => accepted += 1,
                    Err(_) => refused += 1,
                }
                assert_eq!(a.ui.verify(), vec![]);
            }
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let target: Vec<u64> = match (rng >> 40) % 5 {
                0 => vec![key::ROOT, key::BAR, key::PLUS],
                1 => vec![key::ROOT, key::BAR, key::MINUS],
                2 if a.app.tasks.len() < 60 => vec![key::ROOT, key::BAR, key::ADD],
                _ => {
                    let id = a.app.tasks[(rng >> 20) as usize % a.app.tasks.len()].0;
                    vec![key::ROOT, key::LIST, key::TASK + id]
                }
            };
            // A row can be visible in one twin and scrolled out or clipped
            // in the other (different heights): click only targets whose
            // centre lies inside the list (or the bar) in both.
            let visible = |s: &Shell| {
                let (Some(r), Some(c)) = (s.ui.rect(&target), s.ui.rect(&target[..2])) else {
                    return false;
                };
                let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                x > c.x && x < c.x + c.w && y > c.y && y < c.y + c.h
            };
            let focus = (rng >> 8).is_multiple_of(7);
            let hit = visible(&a) && visible(&b);
            for s in [&mut a, &mut b] {
                if focus {
                    s.event(&Event::FocusNext);
                } else if hit {
                    click(s, &target);
                }
            }
        }
        assert_eq!(accepted + refused, 20);
        assert!(accepted > 0 && refused > 0);
        assert_eq!(a.app, b.app);
        assert_eq!(a.ui.focus(), b.ui.focus());
        assert!(
            a.app.tasks.len() > 30 && a.app.tasks.iter().any(|t| t.2),
            "the run did click"
        );
    }
}
