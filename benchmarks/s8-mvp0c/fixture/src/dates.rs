//! Module dates : analyse de dates ISO.

/// Décompose une date ISO `YYYY-MM-DD` valide.
///
/// Renvoie `Some((année, mois, jour))` si le format est exactement
/// `AAAA-MM-JJ` (4 chiffres, tiret, 2 chiffres, tiret, 2 chiffres) et si le mois
/// est dans `1..=12` et le jour dans `1..=31`. Sinon `None`.
pub fn parse_iso_date(text: &str) -> Option<(u16, u8, u8)> {
    // TODO(s8-mvp0c): implémenter l'analyse de date ISO.
    let _ = text;
    None
}
