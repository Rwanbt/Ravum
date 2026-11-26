//! U6 (logique) : profils (PC, tablette, mobile), points de rupture et
//! balayage borné des largeurs, avec gates stricts par profil et sous-score
//! responsive sur le domaine déclaré.
//!
//! Un profil fixe la taille logique, l'échelle du texte, les zones système
//! (encoches, barres) et le type d'entrée. La vue reçoit le profil et le
//! point de rupture actif. Gates par profil : aucun diagnostic Verify, tout
//! le contenu dans la zone sûre, cibles tactiles d'au moins 44 px logiques
//! en entrée tactile. Le balayage n'est **pas** une analyse exacte : il teste
//! un pas fixe plus chaque frontière de point de rupture et ses voisines, et
//! publie sa couverture.

use crate::{Desc, Diagnostic, Path, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Pointer,
    Touch,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: &'static str,
    pub size: (f32, f32),
    pub text_scale: f32,
    /// haut, droite, bas, gauche.
    pub safe: [f32; 4],
    pub input: Input,
}

pub fn desktop() -> Profile {
    Profile {
        name: "desktop",
        size: (1280.0, 800.0),
        text_scale: 1.0,
        safe: [0.0; 4],
        input: Input::Pointer,
    }
}
pub fn tablet() -> Profile {
    Profile {
        name: "tablet",
        size: (820.0, 1180.0),
        text_scale: 1.0,
        safe: [24.0, 0.0, 20.0, 0.0],
        input: Input::Touch,
    }
}
pub fn phone() -> Profile {
    Profile {
        name: "phone",
        size: (390.0, 844.0),
        text_scale: 1.0,
        safe: [47.0, 0.0, 34.0, 0.0],
        input: Input::Touch,
    }
}

pub const MIN_TOUCH: f32 = 44.0;

/// Points de rupture : (largeur minimale, nom), triés par largeur croissante.
pub struct Breakpoints(pub Vec<(f32, &'static str)>);

impl Breakpoints {
    pub fn at(&self, width: f32) -> &'static str {
        self.0
            .iter()
            .rev()
            .find(|(w, _)| width >= *w)
            .or(self.0.first())
            .map_or("", |b| b.1)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    Verify(Diagnostic),
    OutsideSafeArea(Path),
    TouchTargetTooSmall(Path),
}

pub fn check<V: Fn(&Profile, &str) -> Desc>(
    view: &V,
    bp: &Breakpoints,
    p: &Profile,
) -> Vec<Failure> {
    let desc = view(p, bp.at(p.size.0));
    let Ok(ui) = Ui::new(desc, p.size) else {
        return vec![Failure::OutsideSafeArea(Vec::new())];
    };
    let mut out: Vec<Failure> = ui.verify().into_iter().map(Failure::Verify).collect();
    let [t, r, b, l] = p.safe;
    let (w, h) = p.size;
    for (path, rect) in ui.rects() {
        if path.len() < 2 || rect.w == 0.0 || rect.h == 0.0 {
            continue;
        }
        // Contenu de premier niveau : il doit tenir dans la zone sûre.
        if path.len() == 2
            && (rect.x < l || rect.y < t || rect.x + rect.w > w - r || rect.y + rect.h > h - b)
        {
            out.push(Failure::OutsideSafeArea(path.clone()));
        }
        if p.input == Input::Touch
            && ui.find(path).is_some_and(|d| d.style.focusable)
            && (rect.w < MIN_TOUCH || rect.h < MIN_TOUCH)
        {
            out.push(Failure::TouchTargetTooSmall(path.clone()));
        }
    }
    out
}

#[derive(Debug, PartialEq)]
pub struct Sweep {
    pub widths: Vec<f32>,
    pub failing: Vec<(f32, usize)>,
    /// Fraction des largeurs testées sans échec.
    pub score: f64,
}

/// Balayage borné de `min..=max` au pas `step`, plus chaque frontière de
/// point de rupture et ses voisines à ±1 px, à hauteur et profil donnés.
pub fn sweep<V: Fn(&Profile, &str) -> Desc>(
    view: &V,
    bp: &Breakpoints,
    base: &Profile,
    min: f32,
    max: f32,
    step: f32,
) -> Sweep {
    assert!(
        min > 0.0 && max >= min && step > 0.0,
        "invalid sweep domain"
    );
    let mut widths = Vec::new();
    let mut x = min;
    while x <= max {
        widths.push(x);
        x += step;
    }
    widths.push(max);
    for (w, _) in &bp.0 {
        for d in [-1.0, 0.0, 1.0] {
            let v = w + d;
            if (min..=max).contains(&v) {
                widths.push(v);
            }
        }
    }
    widths.sort_by(f32::total_cmp);
    widths.dedup();
    let mut failing = Vec::new();
    for &w in &widths {
        let p = Profile {
            size: (w, base.size.1),
            ..base.clone()
        };
        let n = check(view, bp, &p).len();
        if n > 0 {
            failing.push((w, n));
        }
    }
    let score = 1.0 - failing.len() as f64 / widths.len() as f64;
    Sweep {
        widths,
        failing,
        score,
    }
}
