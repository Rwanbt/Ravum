//! Fixture S8 MVP-0c : petit crate Rust réel avec 4 tâches indépendantes.
//!
//! Les implémentations livrées sont volontairement INCOMPLÈTES : les tests de
//! `tests/behavior.rs` échouent avant les éditions et doivent passer après.
//! Les agents ne modifient que les modules `stats`, `text` et `dates` ; les
//! tests, `Cargo.toml` et `lib.rs` sont protégés.

pub mod dates;
pub mod stats;
pub mod text;
