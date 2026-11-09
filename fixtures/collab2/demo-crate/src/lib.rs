//! Demo crate for COLLAB-2: two independent functions, compiled for real.

fn alpha() -> u32 {
    1
}

fn beta() -> u32 {
    2
}

#[cfg(test)]
mod tests {
    #[test]
    fn functions_stay_positive() {
        assert!(super::alpha() + super::beta() > 0);
    }
}
