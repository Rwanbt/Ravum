/// `[a-z][a-z0-9_-]*`
pub fn is_valid_package_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// `MAJOR.MINOR.PATCH`, decimal integers without leading zeros, no pre-release.
pub fn is_valid_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part.len() == 1 || !part.starts_with('0'))
            && part.parse::<u64>().is_ok()
    })
}

pub(super) fn is_module_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Windows-reserved path component: `CON`, `PRN`, `AUX`, `NUL`, `COM1..COM9`,
/// `LPT1..LPT9` with or without an extension, and any component with a trailing
/// dot or space (Win32 strips them, which breaks deterministic resolution).
/// Refused even on filesystems that accept them (Core 1.0 reproducibility).
pub fn is_reserved_windows_component(component: &str) -> bool {
    let trimmed = component.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || trimmed.len() != component.len() {
        // Empty component or trailing dot/space.
        return true;
    }
    let base = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = base.as_bytes();
    bytes.len() == 4
        && (base.starts_with("COM") || base.starts_with("LPT"))
        && bytes[3].is_ascii_digit()
        && bytes[3] != b'0'
}

// ---------------------------------------------------------------------------
// Dependency graph
// ---------------------------------------------------------------------------
