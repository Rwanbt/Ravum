//! Tests d'intégration invariants de la fixture `grouping-fixture` (L2.4).
//!
//! Ils doivent rester verts pour la base **et** pour toutes les éditions
//! compatibles cumulées (les éditions changent des corps de fonctions, pas les
//! propriétés testées ici). Ils portent le total de la fixture à 12 tests
//! (5 unitaires dans `src/lib.rs` + 7 ici), ce que la campagne batch-e2e exige
//! comme preuve de correction identique entre séquentiel et groupé.

use grouping_fixture::{
    add, checksum, compose, factorial, is_even, lerp, mul, scale, sub, sum_slice,
};

#[test]
fn addition_is_commutative() {
    for (a, b) in [(0i64, 0i64), (2, 3), (-5, 7), (11, -4)] {
        assert_eq!(add(a, b), add(b, a), "add({a},{b}) must be commutative");
    }
}

#[test]
fn multiplication_is_commutative() {
    for (a, b) in [(1i64, 1i64), (3, 4), (-2, 9), (0, 5)] {
        assert_eq!(mul(a, b), mul(b, a), "mul({a},{b}) must be commutative");
    }
}

#[test]
fn parity_agrees_with_modulo() {
    for n in [-4i64, -3, -2, -1, 0, 1, 2, 3, 4] {
        assert_eq!(is_even(n), n % 2 == 0, "is_even({n}) must match modulo");
    }
}

#[test]
fn factorial_has_the_expected_small_values() {
    assert_eq!(factorial(0), 1, "0! == 1");
    assert_eq!(factorial(1), 1, "1! == 1");
    assert_eq!(factorial(2), 2, "2! == 2");
}

#[test]
fn lerp_keeps_its_endpoints() {
    let (a, b) = (2.0f64, 6.0f64);
    assert_eq!(lerp(a, b, 0.0), a, "lerp(t=0) must be a");
    assert_eq!(lerp(a, b, 1.0), b, "lerp(t=1) must be b");
}

#[test]
fn empty_checksum_is_the_fnv_offset_basis() {
    assert_eq!(checksum(b""), 2_166_136_261, "FNV-1a offset basis");
}

#[test]
fn compose_still_uses_scale() {
    assert_eq!(scale(3), 6, "scale is untouched by compatible edits");
    assert_eq!(compose(3), 7, "compose(x) == scale(x) + 1");
    assert_eq!(sub(5, 3), 2);
    assert_eq!(sum_slice(&[1, 2, 3]), 6);
}
