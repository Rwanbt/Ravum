#![allow(dead_code)]

/// Returns the quotient; refuses a zero divisor.
fn checked_divide(value: u32, divisor: u32) -> u32 {
    assert!(divisor != 0, "divisor must not be zero");
    value / divisor
}

#[cfg(test)]
mod tests {
    #[test]
    fn zero_divisor_is_refused() {
        let outcome = std::panic::catch_unwind(|| super::checked_divide(4, 0));
        assert!(outcome.is_err(), "a zero divisor must not be silently accepted");
    }
}
