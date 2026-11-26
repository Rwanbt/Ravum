//! U7 (logique) : Motion Contracts sur une horloge virtuelle.
//!
//! Chaque animation a un **rôle** qui borne sa durée et son amplitude, et dit
//! si elle peut être interrompue. Une animation hors contrat est refusée
//! avant de démarrer. Le reciblage part de la valeur **et** de la vitesse
//! courantes (pas de saut). La réduction du mouvement amène directement à la
//! cible. Une animation hors écran est suspendue (son temps n'avance plus).
//! Le nombre d'animations simultanées est borné. Tout est calculé en temps
//! virtuel (millisecondes entières), donc déterministe.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// Réponse immédiate à une action (appui, survol).
    Feedback,
    /// Changement d'état ou de vue.
    Transition,
    /// Attirer l'attention.
    Emphasis,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contract {
    pub max_ms: u64,
    pub max_amplitude: f32,
    pub interruptible: bool,
    /// Dépassement autorisé au-delà de la cible, en fraction de l'amplitude.
    pub max_overshoot: f32,
}

pub fn contract(role: Role) -> Contract {
    match role {
        Role::Feedback => Contract {
            max_ms: 150,
            max_amplitude: 64.0,
            interruptible: true,
            max_overshoot: 0.0,
        },
        Role::Transition => Contract {
            max_ms: 500,
            max_amplitude: 2000.0,
            interruptible: true,
            max_overshoot: 0.1,
        },
        Role::Emphasis => Contract {
            max_ms: 1000,
            max_amplitude: 200.0,
            interruptible: false,
            max_overshoot: 0.25,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Linear {
        ms: u64,
    },
    /// Courbe de Bézier cubique CSS (x1, y1, x2, y2), x dans [0, 1].
    Bezier {
        ms: u64,
        p: [f32; 4],
    },
    /// Ressort amorti ; durée = temps pour se stabiliser.
    Spring {
        stiffness: f32,
        damping: f32,
    },
}

#[derive(Debug, PartialEq)]
pub enum MotionError {
    TooLong { ms: u64, max: u64 },
    TooLarge { amplitude: f32, max: f32 },
    Overshoot { peak: f32, max: f32 },
    NotInterruptible,
    Budget { max: usize },
    InvalidCurve,
}

#[derive(Clone, Copy, Debug)]
struct Anim {
    from: f32,
    to: f32,
    v0: f32,
    curve: Curve,
    /// Temps écoulé de l'animation (hors suspension), en ms.
    elapsed: u64,
    role: Role,
    /// Durée calculée une fois au lancement.
    dur: u64,
}

const SETTLE: f32 = 1e-3;
const SPRING_LIMIT_MS: u64 = 10_000;

/// Ressort intégré pas à pas (1 ms, Euler semi-implicite) : position relative
/// à la cible `x`, vitesse `v` en unités par ms.
fn spring_at(k: f32, c: f32, x0: f32, v0: f32, t: u64) -> (f32, f32) {
    let (mut x, mut v) = (x0, v0);
    for _ in 0..t {
        let a = -k * x - c * v;
        v += a;
        x += v;
    }
    (x, v)
}

/// Temps de stabilisation et pic de dépassement (en fraction de l'amplitude).
fn spring_profile(k: f32, c: f32, x0: f32, v0: f32) -> Option<(u64, f32)> {
    let amp = x0.abs().max(1e-6);
    let (mut x, mut v) = (x0, v0);
    let mut peak = 0.0f32;
    for t in 0..SPRING_LIMIT_MS {
        if x.abs() <= SETTLE * amp && v.abs() <= SETTLE * amp {
            return Some((t, peak));
        }
        let a = -k * x - c * v;
        v += a;
        x += v;
        if x0 != 0.0 && x.signum() != x0.signum() && x != 0.0 {
            peak = peak.max(x.abs() / amp);
        }
    }
    None
}

fn bezier(p: [f32; 4], u: f32) -> f32 {
    let b = |a: f32, c: f32, t: f32| {
        3.0 * (1.0 - t) * (1.0 - t) * t * a + 3.0 * (1.0 - t) * t * t * c + t * t * t
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if b(p[0], p[2], mid) < u {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    b(p[1], p[3], 0.5 * (lo + hi))
}

impl Anim {
    fn duration(&self) -> u64 {
        self.dur
    }

    fn compute_duration(&self) -> u64 {
        match self.curve {
            Curve::Linear { ms } | Curve::Bezier { ms, .. } => ms,
            Curve::Spring { stiffness, damping } => {
                spring_profile(stiffness, damping, self.from - self.to, self.v0)
                    .map_or(u64::MAX, |p| p.0)
            }
        }
    }

    /// (valeur, vitesse en unités par ms) au temps écoulé `t`.
    fn sample(&self, t: u64) -> (f32, f32) {
        let d = self.duration();
        if t >= d {
            return (self.to, 0.0);
        }
        match self.curve {
            Curve::Spring { stiffness, damping } => {
                let (x, v) = spring_at(stiffness, damping, self.from - self.to, self.v0, t);
                (self.to + x, v)
            }
            _ => {
                let f = |t: u64| {
                    let u = t as f32 / d as f32;
                    let e = match self.curve {
                        Curve::Bezier { p, .. } => bezier(p, u),
                        _ => u,
                    };
                    self.from + (self.to - self.from) * e
                };
                (f(t), f(t + 1) - f(t))
            }
        }
    }
}

pub struct Motion {
    anims: BTreeMap<u64, Anim>,
    values: BTreeMap<u64, f32>,
    hidden: BTreeMap<u64, bool>,
    pub reduced_motion: bool,
    pub max_concurrent: usize,
    now: u64,
}

impl Default for Motion {
    fn default() -> Self {
        Motion {
            anims: BTreeMap::new(),
            values: BTreeMap::new(),
            hidden: BTreeMap::new(),
            reduced_motion: false,
            max_concurrent: 16,
            now: 0,
        }
    }
}

impl Motion {
    pub fn now(&self) -> u64 {
        self.now
    }

    pub fn set(&mut self, key: u64, v: f32) {
        self.anims.remove(&key);
        self.values.insert(key, v);
    }

    pub fn value(&self, key: u64) -> Option<f32> {
        match self.anims.get(&key) {
            Some(a) => Some(a.sample(a.elapsed).0),
            None => self.values.get(&key).copied(),
        }
    }

    pub fn velocity(&self, key: u64) -> f32 {
        self.anims.get(&key).map_or(0.0, |a| a.sample(a.elapsed).1)
    }

    pub fn running(&self) -> usize {
        self.anims.len()
    }

    /// Lance (ou recible) une animation vers `to`. Refusée si elle sort du
    /// contrat de son rôle ; dans ce cas rien ne change.
    pub fn animate(
        &mut self,
        key: u64,
        role: Role,
        curve: Curve,
        to: f32,
    ) -> Result<(), MotionError> {
        let c = contract(role);
        let current = self.value(key).unwrap_or(to);
        let running = self.anims.get(&key).copied();
        if let Some(r) = running {
            if !contract(r.role).interruptible {
                return Err(MotionError::NotInterruptible);
            }
        } else if self.anims.len() >= self.max_concurrent {
            return Err(MotionError::Budget {
                max: self.max_concurrent,
            });
        }
        let amplitude = (to - current).abs();
        if !to.is_finite() || amplitude > c.max_amplitude {
            return Err(MotionError::TooLarge {
                amplitude,
                max: c.max_amplitude,
            });
        }
        let v0 = running.map_or(0.0, |r| r.sample(r.elapsed).1);
        let mut a = Anim {
            from: current,
            to,
            v0,
            curve,
            elapsed: 0,
            role,
            dur: 0,
        };
        match curve {
            Curve::Linear { ms } | Curve::Bezier { ms, .. } => {
                if ms == 0 {
                    return Err(MotionError::InvalidCurve);
                }
                if let Curve::Bezier { p, .. } = curve {
                    if !(0.0..=1.0).contains(&p[0])
                        || !(0.0..=1.0).contains(&p[2])
                        || p.iter().any(|x| !x.is_finite())
                    {
                        return Err(MotionError::InvalidCurve);
                    }
                }
            }
            Curve::Spring { stiffness, damping } => {
                if !(stiffness > 0.0
                    && damping > 0.0
                    && stiffness.is_finite()
                    && damping.is_finite())
                {
                    return Err(MotionError::InvalidCurve);
                }
            }
        }
        let d = a.compute_duration();
        a.dur = d;
        if d > c.max_ms {
            return Err(MotionError::TooLong {
                ms: d,
                max: c.max_ms,
            });
        }
        if let Curve::Spring { stiffness, damping } = curve {
            if amplitude > 0.0 {
                let (_, peak) = spring_profile(stiffness, damping, a.from - a.to, a.v0)
                    .expect("settles, checked above");
                if peak > c.max_overshoot {
                    return Err(MotionError::Overshoot {
                        peak,
                        max: c.max_overshoot,
                    });
                }
            }
        }
        if self.reduced_motion || amplitude == 0.0 && v0 == 0.0 {
            self.set(key, to);
            return Ok(());
        }
        self.anims.insert(key, a);
        Ok(())
    }

    /// Un nœud hors écran suspend son animation ; elle reprend où elle était.
    pub fn set_visible(&mut self, key: u64, visible: bool) {
        self.hidden.insert(key, !visible);
    }

    /// Avance l'horloge virtuelle.
    pub fn advance(&mut self, ms: u64) {
        self.now += ms;
        let mut done = Vec::new();
        for (k, a) in self.anims.iter_mut() {
            if self.hidden.get(k).copied().unwrap_or(false) {
                continue;
            }
            a.elapsed = a.elapsed.saturating_add(ms);
            if a.elapsed >= a.duration() {
                done.push((*k, a.to));
            }
        }
        for (k, v) in done {
            self.set(k, v);
        }
    }
}
