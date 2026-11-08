//! Module gamma : découpage de chaînes.

/// Garde les `n` premiers caractères.
pub fn gamma_prefix(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Joint des éléments avec un séparateur.
pub fn gamma_join(items: &[&str], sep: &str) -> String {
    items.join(sep)
}
