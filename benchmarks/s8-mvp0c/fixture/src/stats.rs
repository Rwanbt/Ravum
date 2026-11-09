//! Module stats : agrégats entiers.
//!
//! Deux fonctions sont incomplètes : `mean` et `median`.

/// Moyenne entière (division entière) d'une liste non vide.
pub fn mean(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    // TODO(s8-mvp0c): implémenter la moyenne entière.
    Some(0)
}

/// Médiane entière d'une liste non vide.
///
/// Si le nombre d'éléments est pair, renvoie la moyenne entière des deux
/// valeurs centrales.
pub fn median(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    // TODO(s8-mvp0c): implémenter la médiane entière.
    Some(0)
}
