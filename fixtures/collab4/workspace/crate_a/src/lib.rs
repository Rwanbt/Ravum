//! `crate_a` — unité sans dépendance du scénario COLLAB-4.
//!
//! Deux fonctions indépendantes, modifiées séparément par deux agents. Les
//! tests unitaires restent vrais après les deux modifications du scénario
//! (assertions de propriété, pas de valeur exacte) : le mode d'intégration
//! COLLAB-1 est textuel et n'exécute aucun test.

/// Double une valeur.
pub fn double(x: u32) -> u32 {
    x * 2
}

/// Décale une valeur.
pub fn shift(x: u32) -> u32 {
    x + 1
}

#[cfg(test)]
mod tests {
    #[test]
    fn both_functions_are_independent() {
        assert!(super::double(2) >= 4);
        assert!(super::shift(2) >= 3);
    }
}
