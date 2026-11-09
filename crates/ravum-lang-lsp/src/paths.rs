//! `file://` URI <-> filesystem path conversion.
//!
//! Editors speak URIs, the analyzer speaks paths. Both directions must be
//! lossless enough that a diagnostic produced for `C:\proj\src\main.rv` reaches
//! the buffer the user actually opened, including on Windows drive letters and
//! in paths containing spaces.

use std::path::{Path, PathBuf};

/// Decode `%XX` escapes. Returns `None` on a malformed escape.
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            let value = u8::from_str_radix(hex, 16).ok()?;
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Convert a `file://` URI to a path. Returns `None` for other schemes, which
/// the server reports as unsupported rather than guessing.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = percent_decode(rest)?;
    // `file:///C:/dir/file.rv` -> `/C:/dir/file.rv` -> `C:/dir/file.rv`
    let trimmed = decoded.strip_prefix('/').unwrap_or(&decoded);
    let looks_like_drive = trimmed.len() >= 2
        && trimmed.as_bytes()[0].is_ascii_alphabetic()
        && trimmed.as_bytes()[1] == b':';
    if looks_like_drive {
        return Some(PathBuf::from(trimmed));
    }
    // POSIX absolute path: keep the leading separator.
    Some(PathBuf::from(format!("/{}", trimmed)))
}

pub fn path_to_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{}", percent_encode(&text))
    } else {
        format!("file:///{}", percent_encode(&text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_drive_round_trip() {
        let path = PathBuf::from("C:/proj/src/main.rv");
        let uri = path_to_uri(&path);
        assert_eq!(uri, "file:///C:/proj/src/main.rv");
        assert_eq!(uri_to_path(&uri).unwrap(), path);
    }

    #[test]
    fn spaces_are_escaped_then_decoded() {
        let path = PathBuf::from("C:/my proj/main.rv");
        let uri = path_to_uri(&path);
        assert!(uri.contains("%20"), "uri was {uri}");
        assert_eq!(uri_to_path(&uri).unwrap(), path);
    }

    #[test]
    fn backslashes_become_forward_slashes() {
        let path = PathBuf::from("C:\\proj\\src\\main.rv");
        assert_eq!(path_to_uri(&path), "file:///C:/proj/src/main.rv");
    }

    #[test]
    fn posix_path_keeps_its_root() {
        assert_eq!(
            uri_to_path("file:///home/user/x.rv").unwrap(),
            PathBuf::from("/home/user/x.rv")
        );
    }

    #[test]
    fn non_file_scheme_is_rejected() {
        assert!(uri_to_path("https://example.com/x.rv").is_none());
        assert!(uri_to_path("untitled:Untitled-1").is_none());
    }

    #[test]
    fn malformed_escape_is_rejected() {
        assert!(uri_to_path("file:///tmp/%zz").is_none());
        assert!(uri_to_path("file:///tmp/%4").is_none());
    }
}
