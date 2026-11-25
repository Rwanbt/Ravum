//! U1-B, tour 3 : protocole commun aux applications de chaque pile.
//!
//! Scène : `R3_ROWS` lignes (défaut 10 000), chacune un libellé
//! `param {i}: {v:.3}` et un slider ; la pile virtualise avec ses propres
//! moyens. À chaque frame, un paramètre **visible** change (comme un
//! glissement de curseur) et un redessin est demandé. Après `R3_FRAMES`
//! frames (défaut 600), l'application écrit `R3_OUT` (JSON) et se termine.
//!
//! Mesuré ici : temps du démarrage du processus (début de `main`) à la
//! première frame, et intervalle entre frames successives. Le temps CPU et
//! le pic de mémoire du processus sont mesurés par le lanceur, de
//! l'extérieur, de la même façon pour toutes les piles.

use std::time::{Duration, Instant};

pub struct Scene {
    pub params: Vec<f32>,
    frame: u64,
}

impl Scene {
    pub fn new() -> Scene {
        let n = env_usize("R3_ROWS", 10_000);
        Scene {
            params: (0..n).map(|i| i as f32 * 0.001).collect(),
            frame: 0,
        }
    }
    /// One update per frame: a visible parameter (among the first 20) moves.
    pub fn step(&mut self) {
        let k = (self.frame % 20) as usize;
        if let Some(p) = self.params.get_mut(k) {
            *p = (self.frame as f32 * 0.01) % 1.0;
        }
        self.frame += 1;
    }
    pub fn label(&self, i: usize) -> String {
        format!("param {i}: {:.3}", self.params[i])
    }
}

impl Default for Scene {
    fn default() -> Self {
        Scene::new()
    }
}

pub struct Recorder {
    start: Instant,
    first: Option<Duration>,
    last: Option<Instant>,
    intervals: Vec<Duration>,
    target: usize,
    stack: &'static str,
    written: bool,
}

impl Recorder {
    /// Call first thing in `main`.
    pub fn new(stack: &'static str) -> Recorder {
        Recorder {
            start: Instant::now(),
            first: None,
            last: None,
            intervals: Vec::new(),
            target: env_usize("R3_FRAMES", 600),
            stack,
            written: false,
        }
    }

    /// Call once per presented frame. Returns `true` when the run is over
    /// (the report has been written; the app should close).
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        match self.last {
            None => self.first = Some(now - self.start),
            Some(l) => self.intervals.push(now - l),
        }
        self.last = Some(now);
        if self.intervals.len() >= self.target && !self.written {
            self.write();
            self.written = true;
        }
        self.written
    }

    fn write(&self) {
        let mut v = self.intervals.clone();
        v.sort();
        let q = |p: f64| v[((v.len() - 1) as f64 * p) as usize].as_secs_f64() * 1e3;
        let json = format!(
            "{{\"schema\":\"ravum.ui.round3/v1\",\"stack\":\"{}\",\"rows\":{},\"frames\":{},\
             \"first_frame_ms\":{:.3},\"interval_ms\":{{\"p50\":{:.3},\"p95\":{:.3},\"p99\":{:.3},\"max\":{:.3}}},\
             \"os\":\"{}\",\"arch\":\"{}\"}}\n",
            self.stack,
            env_usize("R3_ROWS", 10_000),
            v.len(),
            self.first.unwrap_or_default().as_secs_f64() * 1e3,
            q(0.5),
            q(0.95),
            q(0.99),
            q(1.0),
            std::env::consts::OS,
            std::env::consts::ARCH,
        );
        match std::env::var("R3_OUT") {
            Ok(p) => std::fs::write(p, json).expect("cannot write R3_OUT"),
            Err(_) => print!("{json}"),
        }
    }
}

fn env_usize(k: &str, d: usize) -> usize {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}
