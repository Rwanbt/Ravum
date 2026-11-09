//! Module beta : formatage de texte.

/// Complète `text` jusqu'à `width` caractères avec des espaces.
pub fn beta_pad(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

/// Supprime les espaces de début et de fin de chaque élément.
pub fn beta_trim_all(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.trim().to_string()).collect()
}
