//! 10 tests de comportement de la fixture S8.
//!
//! Toutes les tâches du corpus sont conçues pour laisser ces tests verts : les
//! éditions sont des réécritures équivalentes sur le plan observable, sauf le
//! cas de signature (`t3`) qui met aussi à jour ses deux sites d'appel.

use s8_fixture::{alpha, beta, delta, gamma};

#[test]
fn alpha_add_sums() {
    assert_eq!(alpha::alpha_add(2, 3), 5);
}

#[test]
fn alpha_double_doubles() {
    assert_eq!(alpha::alpha_double(-4), -8);
}

#[test]
fn beta_pad_pads_to_width() {
    assert_eq!(beta::beta_pad("ab", 5), "ab   ");
}

#[test]
fn beta_pad_keeps_long_text() {
    assert_eq!(beta::beta_pad("abcdef", 3), "abcdef");
}

#[test]
fn beta_trim_all_trims() {
    assert_eq!(
        beta::beta_trim_all(&[" a ", "b"]),
        vec!["a".to_string(), "b".to_string()]
    );
}

#[test]
fn gamma_prefix_takes_chars() {
    assert_eq!(gamma::gamma_prefix("héllo", 3), "hél");
}

#[test]
fn gamma_prefix_short_input() {
    assert_eq!(gamma::gamma_prefix("hi", 5), "hi");
}

#[test]
fn gamma_join_joins() {
    assert_eq!(gamma::gamma_join(&["a", "b", "c"], "-"), "a-b-c");
}

#[test]
fn delta_count_counts() {
    assert_eq!(delta::delta_count(&[1, 2, 3]), 3);
}

#[test]
fn delta_max_finds_max() {
    assert_eq!(delta::delta_max(&[4, 1, 9]), Some(9));
}
