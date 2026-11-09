//! `grouping_unit` — fixture de mesure MVP-0c L2 (groupement de patches).
//!
//! Unité mono-bibliothèque de taille moyenne (fonctions pures indépendantes,
//! petit type `Rectangle`, tests unitaires). Aucune dépendance externe : le
//! module s'analyse avec `rustc --emit=metadata` et se vérifie avec
//! `cargo check --manifest-path fixtures/grouping/Cargo.toml`.
//!
//! Le harnais `ravum-daemon-bench --suite grouping` applique réellement les
//! éditions décrites dans `edits.json` (6 éditions compatibles ciblant des
//! corps de fonctions distinctes, 1 édition incompatible qui casse la signature
//! de `scale` utilisée par `compose`) et mesure le coût réel des deux
//! stratégies : N cycles séquentiels contre un lot groupé.
//!
//! Contenu volontairement déterministe : aucun `unsafe`, aucune horloge, aucun
//! I/O, aucune macro procédurale — la seule variable mesurée est le travail de
//! `rustc` sur des octets différents.

pub const VERSION: u32 = 1;

/// Plafond utilisé par plusieurs fonctions de sécurité numérique.
pub const LIMIT: u32 = 1_000;

// ---------------------------------------------------------------------------
// Arithmétique entière
// ---------------------------------------------------------------------------

/// Additionne deux entiers.
pub fn add(a: i64, b: i64) -> i64 {
    a + b
}

/// Soustrait `b` de `a`.
pub fn sub(a: i64, b: i64) -> i64 {
    a - b
}

/// Multiplie deux entiers en arithmétique enveloppante explicite.
pub fn mul(a: i64, b: i64) -> i64 {
    a.wrapping_mul(b)
}

/// Oppose une valeur.
pub fn negate(value: i64) -> i64 {
    -value
}

/// Différence absolue entre deux entiers non signés.
pub fn abs_diff(a: u64, b: u64) -> u64 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

/// Borne une valeur dans `[lo, hi]`.
pub fn clamp(value: i64, lo: i64, hi: i64) -> i64 {
    value.max(lo).min(hi)
}

/// Plus grand commun diviseur par l'algorithme d'Euclide.
pub fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Plus petit commun multiple, sans débordement intermédiaire évitable.
pub fn lcm(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 {
        return 0;
    }
    (a / gcd(a, b)).saturating_mul(b)
}

/// Vrai si `n` est pair.
pub fn is_even(n: i64) -> bool {
    n % 2 == 0
}

/// Factorielle itérative en `u64` (déborde au-delà de 20!, comportement documenté).
pub fn factorial(n: u64) -> u64 {
    (2..=n).product::<u64>()
}

/// `n`-ième nombre de Fibonacci (F0 = 0, F1 = 1), itératif.
pub fn fibonacci(n: u32) -> u64 {
    let mut a = 0u64;
    let mut b = 1u64;
    for _ in 0..n {
        let next = a.saturating_add(b);
        a = b;
        b = next;
    }
    a
}

/// Somme des chiffres décimaux d'un entier non signé.
pub fn digit_sum(mut n: u64) -> u64 {
    let mut sum = 0u64;
    while n > 0 {
        sum += n % 10;
        n /= 10;
    }
    sum
}

/// Addition saturante de deux entiers signés.
pub fn saturating_sum(a: i64, b: i64) -> i64 {
    a.saturating_add(b)
}

/// Produit saturant de deux entiers signés.
pub fn saturating_product(a: i64, b: i64) -> i64 {
    a.saturating_mul(b)
}

/// Conversion bornée d'un `u64` vers `u32` (l'échelle est ramenée à `LIMIT`).
pub fn bounded_u32(value: u64) -> u32 {
    u32::try_from(value.min(u64::from(LIMIT))).unwrap_or(LIMIT)
}

/// Applique l'échelle du module : `x * 2`.
///
/// Cette fonction est la cible de l'édition **incompatible** du scénario : le
/// harnais change sa signature (`scale(x, factor)`) sans toucher aux appelants,
/// ce que `rustc` doit refuser.
pub fn scale(x: u32) -> u32 {
    x * 2
}

/// Compose `scale` avec une décale : l'appelant qui doit casser si la signature
/// de `scale` change.
pub fn compose(x: u32) -> u32 {
    scale(x) + 1
}

// ---------------------------------------------------------------------------
// Tranches (slices)
// ---------------------------------------------------------------------------

/// Somme d'une tranche d'entiers.
pub fn sum_slice(values: &[i64]) -> i64 {
    values.iter().copied().sum()
}

/// Maximum d'une tranche, `i64::MIN` si vide.
pub fn max_slice(values: &[i64]) -> i64 {
    values.iter().copied().max().unwrap_or(i64::MIN)
}

/// Minimum d'une tranche, `i64::MAX` si vide.
pub fn min_slice(values: &[i64]) -> i64 {
    values.iter().copied().min().unwrap_or(i64::MAX)
}

/// Moyenne d'une tranche, `None` si vide.
pub fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let sum: f64 = values.iter().copied().sum();
    Some(sum / values.len() as f64)
}

/// Médiane approximative d'une tranche triée par index (sans trier la copie).
pub fn median_of_sorted(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values[(values.len() - 1) / 2])
}

/// Variance non biaisée d'une tranche, `None` si moins de deux valeurs.
pub fn variance(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let m = mean(values)?;
    let acc: f64 = values.iter().map(|value| (value - m).powi(2)).sum();
    Some(acc / (values.len() - 1) as f64)
}

/// Nombre d'éléments strictement positifs.
pub fn count_positive(values: &[i64]) -> usize {
    values.iter().filter(|value| **value > 0).count()
}

/// Index de la première occurrence de `target`, `None` sinon.
pub fn find_index(values: &[i64], target: i64) -> Option<usize> {
    values.iter().position(|value| *value == target)
}

/// Moyenne glissante de fenêtre `window` (fenêtre tronquée en fin de tranche).
pub fn moving_average(values: &[f64], window: usize) -> Vec<f64> {
    if values.is_empty() || window == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(values.len());
    for index in 0..values.len() {
        let start = index.saturating_sub(window - 1);
        let slice = &values[start..=index];
        let sum: f64 = slice.iter().copied().sum();
        out.push(sum / slice.len() as f64);
    }
    out
}

// ---------------------------------------------------------------------------
// Géométrie flottante
// ---------------------------------------------------------------------------

/// Carré de la distance euclidienne.
pub fn distance_sq(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

/// Distance de Manhattan.
pub fn manhattan(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    (ax - bx).abs() + (ay - by).abs()
}

/// Produit scalaire de deux points.
pub fn dot(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    ax * bx + ay * by
}

/// Composante `z` du produit vectoriel 2D.
pub fn cross_z(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    ax * by - ay * bx
}

/// Interpolation linéaire entre `a` et `b` par `t`.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Interpolation lissée (smoothstep) entre `a` et `b`.
pub fn smoothstep(a: f64, b: f64, t: f64) -> f64 {
    let clamped = t.max(0.0).min(1.0);
    let smooth = clamped * clamped * (3.0 - 2.0 * clamped);
    a + (b - a) * smooth
}

/// Borne une valeur dans `[0, 1]`.
pub fn clamp_unit(value: f64) -> f64 {
    value.max(0.0).min(1.0)
}

/// Normalise un vecteur 2D ; renvoie `(0, 0)` pour le vecteur nul.
pub fn normalize(x: f64, y: f64) -> (f64, f64) {
    let norm = (x * x + y * y).sqrt();
    if norm == 0.0 {
        (0.0, 0.0)
    } else {
        (x / norm, y / norm)
    }
}

// ---------------------------------------------------------------------------
// Type `Rectangle`
// ---------------------------------------------------------------------------

/// Rectangle aligné sur les axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rectangle {
    /// Construit un rectangle normalisé (largeur/hauteur positives).
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width: width.abs(),
            height: height.abs(),
        }
    }

    /// Aire, toujours positive ou nulle.
    pub fn area(&self) -> f64 {
        self.width * self.height
    }

    /// Teste l'intersection non vide avec un autre rectangle.
    pub fn intersects(&self, other: &Rectangle) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }

    /// Plus petit rectangle contenant les deux rectangles.
    pub fn union(&self, other: &Rectangle) -> Rectangle {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Rectangle::new(x, y, right - x, bottom - y)
    }

    /// Contient-il un point ?
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }

    /// Intersection avec un autre rectangle, `None` si vide.
    pub fn intersection(&self, other: &Rectangle) -> Option<Rectangle> {
        if !self.intersects(other) {
            return None;
        }
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        Some(Rectangle::new(x, y, right - x, bottom - y))
    }
}

/// Ramène un rectangle dans une boîte englobante.
pub fn clamp_rect(rect: &Rectangle, bounds: &Rectangle) -> Rectangle {
    let width = rect.width.min(bounds.width);
    let height = rect.height.min(bounds.height);
    let x = rect.x.max(bounds.x).min(bounds.x + bounds.width - width);
    let y = rect.y.max(bounds.y).min(bounds.y + bounds.height - height);
    Rectangle::new(x, y, width, height)
}

// ---------------------------------------------------------------------------
// Bits et hachage
// ---------------------------------------------------------------------------

/// Rotation circulaire à gauche d'un `u32`.
pub fn rotate_left(value: u32, amount: u32) -> u32 {
    value.rotate_left(amount % 32)
}

/// Nombre de bits à 1.
pub fn popcount(value: u64) -> u32 {
    value.count_ones()
}

/// Inversion des bits d'un `u32`.
pub fn reverse_bits(value: u32) -> u32 {
    value.reverse_bits()
}

/// Hachage FNV-1a 32 bits d'une tranche d'octets.
pub fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(2166136261u32, |acc, byte| {
        (acc ^ u32::from(*byte)).wrapping_mul(16777619)
    })
}

/// Somme de contrôle maison, utilisée comme mesure de similarité interne.
pub fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(2166136261u32, |acc, byte| {
        (acc ^ u32::from(*byte)).wrapping_mul(16777619)
    })
}

/// Hachage stable d'un couple d'entiers.
pub fn hash_pair(a: u64, b: u64) -> u64 {
    a.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(31)
        .wrapping_add(b)
}

// ---------------------------------------------------------------------------
// Chaînes de caractères
// ---------------------------------------------------------------------------

/// Formate un couple `clé: valeur`.
pub fn format_pair(key: &str, value: i64) -> String {
    format!("{key}:{value}")
}

/// Parse un entier décimal, avec l'erreur standard de `str::parse`.
pub fn parse_i64(text: &str) -> Result<i64, std::num::ParseIntError> {
    text.trim().parse::<i64>()
}

/// Découpe une chaîne sur les espaces (tranches non vides).
pub fn tokenize(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

/// Compte les mots d'un texte.
pub fn word_count(text: &str) -> usize {
    tokenize(text).len()
}

/// Mot le plus long (premier en cas d'égalité), `None` si vide.
pub fn longest_word(text: &str) -> Option<&str> {
    let mut best: Option<&str> = None;
    for token in tokenize(text) {
        if best.map_or(true, |current| token.len() > current.len()) {
            best = Some(token);
        }
    }
    best
}

/// Met une chaîne en `kebab-case` approximatif (ASCII).
pub fn to_kebab_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, character) in text.chars().enumerate() {
        if character == ' ' || character == '_' {
            out.push('-');
        } else if character.is_ascii_uppercase() && index > 0 {
            out.push('-');
            out.push(character.to_ascii_lowercase());
        } else {
            out.push(character.to_ascii_lowercase());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests unitaires (non compilés par l'analyse `--emit=metadata` sans cfg test)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_helpers_are_consistent() {
        assert_eq!(add(2, 3), 5);
        assert_eq!(sub(5, 3), 2);
        assert_eq!(mul(4, 3), 12);
        assert_eq!(gcd(12, 18), 6);
        assert_eq!(lcm(4, 6), 12);
    }

    #[test]
    fn scale_and_compose_stay_consistent() {
        assert_eq!(scale(3), 6);
        assert_eq!(compose(3), 7);
    }

    #[test]
    fn slice_helpers_handle_empty_inputs() {
        assert_eq!(sum_slice(&[]), 0);
        assert_eq!(max_slice(&[]), i64::MIN);
        assert_eq!(min_slice(&[]), i64::MAX);
        assert_eq!(mean(&[]), None);
        assert!(moving_average(&[], 3).is_empty());
    }

    #[test]
    fn rectangles_intersect_and_union() {
        let a = Rectangle::new(0.0, 0.0, 10.0, 10.0);
        let b = Rectangle::new(5.0, 5.0, 10.0, 10.0);
        assert!(a.intersects(&b));
        assert_eq!(a.area(), 100.0);
        let union = a.union(&b);
        assert_eq!(union.area(), 225.0);
    }

    #[test]
    fn string_helpers_are_simple() {
        assert_eq!(format_pair("score", 7), "score:7");
        assert_eq!(parse_i64(" 42 ").unwrap(), 42);
        assert_eq!(word_count("deux mots"), 2);
        assert_eq!(longest_word("a bbb cc"), Some("bbb"));
        assert_eq!(to_kebab_case("mon Texte"), "mon-texte");
    }
}
