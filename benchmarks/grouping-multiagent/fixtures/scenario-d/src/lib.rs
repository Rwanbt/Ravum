//! Dépendance non déclarée : `compose` appelle `helper`.

pub fn helper(x: u32) -> u32 {
    x + 1
}

pub fn compose(x: u32) -> u32 {
    helper(x) * 2
}

pub fn other(x: u32) -> u32 {
    x - 1
}
