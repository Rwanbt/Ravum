//! U9 (données, sans rendu) : ce dont les surfaces audio ont besoin, à
//! l'écart du thread audio.
//!
//! - [`Ring`] : file un producteur, un consommateur, capacité fixe allouée à
//!   la construction. `push` (côté audio) **n'alloue pas, ne bloque pas** et
//!   ne fait que des opérations atomiques (INV-05) ; plein, il refuse et
//!   compte la perte au lieu d'attendre. Aucun `unsafe` : les échantillons
//!   sont stockés en bits dans des `AtomicU32`.
//! - [`Peaks`] : pyramide min/max pour dessiner une forme d'onde à n'importe
//!   quel zoom sans relire tous les échantillons ; résultat égal au calcul
//!   direct.
//! - [`Timeline`] : temps ↔ pixels, zoom autour d'un point qui reste fixe.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

pub struct Ring {
    buf: Vec<AtomicU32>,
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicU64,
}

impl Ring {
    pub fn new(capacity: usize) -> Ring {
        assert!(capacity > 0);
        Ring {
            buf: (0..capacity + 1).map(|_| AtomicU32::new(0)).collect(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    /// Côté producteur (thread audio) : jamais d'allocation ni d'attente.
    pub fn push(&self, x: f32) -> bool {
        let t = self.tail.load(Ordering::Relaxed);
        let next = (t + 1) % self.buf.len();
        if next == self.head.load(Ordering::Acquire) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.buf[t].store(x.to_bits(), Ordering::Relaxed);
        self.tail.store(next, Ordering::Release);
        true
    }

    /// Côté consommateur (thread UI).
    pub fn pop(&self) -> Option<f32> {
        let h = self.head.load(Ordering::Relaxed);
        if h == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let x = f32::from_bits(self.buf[h].load(Ordering::Relaxed));
        self.head.store((h + 1) % self.buf.len(), Ordering::Release);
        Some(x)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Pyramide min/max : le niveau `k` résume des blocs de `2^k` échantillons.
pub struct Peaks {
    levels: Vec<Vec<(f32, f32)>>,
    len: usize,
}

fn merge(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0.min(b.0), a.1.max(b.1))
}

impl Peaks {
    pub fn build(samples: &[f32]) -> Peaks {
        let mut levels = vec![samples.iter().map(|&x| (x, x)).collect::<Vec<_>>()];
        while levels.last().unwrap().len() > 1 {
            let prev = levels.last().unwrap();
            levels.push(
                prev.chunks(2)
                    .map(|c| c.iter().copied().reduce(merge).unwrap())
                    .collect(),
            );
        }
        Peaks {
            levels,
            len: samples.len(),
        }
    }

    /// Min et max de `[a, b)` en O(log n) blocs.
    pub fn range(&self, mut a: usize, b: usize) -> Option<(f32, f32)> {
        let b = b.min(self.len);
        let mut acc: Option<(f32, f32)> = None;
        while a < b {
            // Le plus grand bloc aligné sur `a` qui tient dans [a, b).
            let mut k = 0;
            while k + 1 < self.levels.len()
                && a.is_multiple_of(1 << (k + 1))
                && a + (1 << (k + 1)) <= b
            {
                k += 1;
            }
            let v = self.levels[k][a >> k];
            acc = Some(acc.map_or(v, |x| merge(x, v)));
            a += 1 << k;
        }
        acc
    }

    /// Une paire (min, max) par pixel pour `[start, end)` sur `width` pixels.
    pub fn columns(&self, start: usize, end: usize, width: usize) -> Vec<Option<(f32, f32)>> {
        let span = end.saturating_sub(start) as f64;
        (0..width)
            .map(|px| {
                let a = start + (span * px as f64 / width as f64).floor() as usize;
                let b = start + (span * (px + 1) as f64 / width as f64).floor() as usize;
                self.range(a, b.max(a + 1))
            })
            .collect()
    }
}

/// Fenêtre de temps visible : `start` secondes au pixel 0, `px_per_s`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timeline {
    pub start: f64,
    pub px_per_s: f64,
    pub min_px_per_s: f64,
    pub max_px_per_s: f64,
}

impl Timeline {
    pub fn to_px(&self, t: f64) -> f64 {
        (t - self.start) * self.px_per_s
    }
    pub fn to_time(&self, px: f64) -> f64 {
        self.start + px / self.px_per_s
    }
    /// Zoom de facteur `f` autour du pixel `px` : le temps sous le curseur
    /// ne bouge pas ; le zoom est borné, et `start` ne passe pas sous 0.
    pub fn zoom(&mut self, px: f64, f: f64) {
        if !(f.is_finite() && f > 0.0) {
            return;
        }
        let anchor = self.to_time(px);
        self.px_per_s = (self.px_per_s * f).clamp(self.min_px_per_s, self.max_px_per_s);
        self.start = (anchor - px / self.px_per_s).max(0.0);
    }
}
