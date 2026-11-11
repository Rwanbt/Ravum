//! Tests de comportement de la fixture S8 MVP-0c.
//!
//! Fichier PROTÉGÉ : aucun agent ne doit le modifier. Ces tests échouent avant
//! les éditions (implémentations incomplètes) et doivent tous passer après.

use s8mvp0c_fixture::{dates, stats, text};

// --- t1 : stats::mean -------------------------------------------------------

#[test]
fn mean_of_simple_values() {
    assert_eq!(stats::mean(&[4, 1, 3, 2]), Some(2));
}

#[test]
fn mean_of_negative_values() {
    assert_eq!(stats::mean(&[-4, -2]), Some(-3));
}

#[test]
fn mean_of_empty_is_none() {
    assert_eq!(stats::mean(&[]), None);
}

// --- t2 : text::slugify -----------------------------------------------------

#[test]
fn slugify_basic_title() {
    assert_eq!(text::slugify("Hello, World!"), "hello-world");
}

#[test]
fn slugify_collapses_separators_and_trims() {
    assert_eq!(text::slugify("  Rust--2024  "), "rust-2024");
}

#[test]
fn slugify_empty_is_empty() {
    assert_eq!(text::slugify(""), "");
}

// --- t3 : stats::median -----------------------------------------------------

#[test]
fn median_of_odd_list() {
    assert_eq!(stats::median(&[7, 1, 3]), Some(3));
}

#[test]
fn median_of_even_list() {
    assert_eq!(stats::median(&[4, 1, 3, 2]), Some(2));
}

#[test]
fn median_of_empty_is_none() {
    assert_eq!(stats::median(&[]), None);
}

// --- t4 : dates::parse_iso_date --------------------------------------------

#[test]
fn parse_iso_date_accepts_valid_date() {
    assert_eq!(dates::parse_iso_date("2026-11-09"), Some((2026, 11, 9)));
}

#[test]
fn parse_iso_date_rejects_invalid_month() {
    assert_eq!(dates::parse_iso_date("2026-13-01"), None);
}

#[test]
fn parse_iso_date_rejects_wrong_shape() {
    assert_eq!(dates::parse_iso_date("26-11-09"), None);
    assert_eq!(dates::parse_iso_date("2026-11-09x"), None);
}
