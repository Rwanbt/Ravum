#![allow(dead_code)]

fn double(value: u32) -> u32 {
    value * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn doubling_is_correct() {
        assert_eq!(super::double(3), 6);
    }
}
