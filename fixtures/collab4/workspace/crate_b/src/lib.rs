//! `crate_b` — dépend de `crate_a` par chemin (`[dependencies] path = "../crate_a"`).
//!
//! Le code de production reste local à l'unité : l'analyse du daemon est un
//! `--emit=metadata` d'une seule unité et ne lie pas les dépendances. L'usage
//! réel de `crate_a` vit dans le harnais de test (`#[cfg(test)]`), vérifié par
//! `cargo check --tests` / `cargo test`, jamais par l'unité d'analyse daemon.

pub fn triple(x: u32) -> u32 {
    x * 3
}

#[cfg(test)]
mod tests {
    #[test]
    fn composes_with_crate_a() {
        assert_eq!(super::triple(crate_a::double(2)), 12);
    }
}
