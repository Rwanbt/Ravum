//! Module delta : agrégats sur des entiers.

/// Nombre d'éléments.
pub fn delta_count(items: &[i32]) -> usize {
    items.len()
}

/// Maximum des éléments, si non vide.
pub fn delta_max(items: &[i32]) -> Option<i32> {
    items.iter().copied().max()
}
