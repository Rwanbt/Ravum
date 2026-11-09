//! `crate_c` — unité de contrôle indépendante du scénario COLLAB-4.
//!
//! Aucun lien (ni direct, ni par chemin) vers `crate_a` ou `crate_b` : elle ne
//! doit jamais être affectée par le scénario et doit apparaître explicitement
//! dans `skipped_units`.

pub fn identity(x: u32) -> u32 {
    x
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_holds() {
        assert_eq!(super::identity(7), 7);
    }
}
