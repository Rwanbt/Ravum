//! Module text : transformations de chaînes.

/// Transforme un titre en slug ASCII minuscule.
///
/// Règles : les caractères ASCII alphanumériques sont conservés (minusculisés),
/// tout le reste devient un séparateur ; les séparateurs consécutifs sont
/// fusionnés et les bords sont retirés.
pub fn slugify(title: &str) -> String {
    // TODO(s8-mvp0c): implémenter la transformation en slug.
    title.to_string()
}
