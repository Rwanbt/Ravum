//! Small shared helpers: repository discovery, host OS, path sanitization,
//! output truncation and UTC timestamps. All are `std`-only.

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Host operating system name as used by `skip_on` in `case.json`.
pub fn host_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "other"
    }
}

/// Walks ancestors of `start` looking for the workspace root: the outermost
/// ancestor whose `Cargo.toml` declares `[workspace]`. A crate directory also
/// has a `Cargo.toml`, so the nearest manifest is only a fallback.
pub fn find_repo_root(start: &Path) -> PathBuf {
    let mut dir = start.to_path_buf();
    let mut nearest = None;
    loop {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            if nearest.is_none() {
                nearest = Some(dir.clone());
            }
            if let Ok(text) = std::fs::read_to_string(&manifest) {
                if text.contains("[workspace]") {
                    return dir;
                }
            }
        }
        if !dir.pop() {
            break;
        }
    }
    nearest.unwrap_or_else(|| start.to_path_buf())
}

/// True when `path` is a safe *relative* path: no root, no drive prefix, no
/// `..` component. `.` and empty components are accepted (`"."` means "the
/// case directory itself").
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return false;
    }
    candidate
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

/// True when `arg` looks like an absolute path on any supported platform.
/// Used to refuse CLI arguments that could write outside the isolated copy.
pub fn looks_absolute(arg: &str) -> bool {
    let bytes = arg.as_bytes();
    if arg.starts_with('/') || arg.starts_with('\\') {
        return true;
    }
    // `C:\...` or `C:/...`
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    false
}

/// Replaces host-specific absolute prefixes with stable placeholders so that
/// the JSON report never contains a user path.
#[derive(Debug, Clone)]
pub struct Sanitizer {
    prefixes: Vec<(String, &'static str)>,
    home: Option<String>,
}

impl Sanitizer {
    pub fn new(repo: &Path, temp_base: &Path, case_work_root: Option<&Path>) -> Sanitizer {
        let mut prefixes: Vec<(String, &'static str)> = Vec::new();
        // Longest first: the per-case copy lives under the temp base, so it
        // must be replaced before the temp base itself.
        if let Some(root) = case_work_root {
            prefixes.push((path_text(root), "<tmp>"));
        }
        prefixes.push((path_text(temp_base), "<tmp>"));
        prefixes.push((path_text(repo), "<repo>"));
        let mut sanitizer = Sanitizer {
            prefixes,
            home: None,
        };
        if let Some(home) = home_dir() {
            sanitizer.home = Some(path_text(&home));
            sanitizer.prefixes.push((path_text(&home), "<home>"));
        }
        sanitizer.prefixes.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        sanitizer
    }

    pub fn apply(&self, input: &str) -> String {
        let mut out = input.to_string();
        for (prefix, replacement) in &self.prefixes {
            if prefix.len() < 3 {
                continue;
            }
            out = out.replace(prefix, replacement);
            // Some tools print forward slashes on Windows; replace that form too.
            let alternate = prefix.replace('\\', "/");
            if alternate != *prefix {
                out = out.replace(&alternate, replacement);
            }
        }
        if let Some(home) = &self.home {
            let alternate = home.replace('\\', "/");
            if alternate != *home {
                out = out.replace(&alternate, "<home>");
            }
        }
        scrub_absolute_paths(&out)
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Best-effort removal of remaining absolute paths (drive-letter and common
/// Unix roots). This is intentionally conservative: it replaces the path
/// token, never invents content.
fn scrub_absolute_paths(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;
    while index < chars.len() {
        let boundary = index == 0
            || matches!(
                chars[index - 1],
                ' ' | '\t' | '\n' | '\r' | '"' | '\'' | '(' | '=' | ':' | ','
            );
        if boundary {
            let start = index;
            let mut end = index;
            let is_drive = index + 2 < chars.len()
                && chars[index].is_ascii_alphabetic()
                && chars[index + 1] == ':'
                && (chars[index + 2] == '\\' || chars[index + 2] == '/');
            let is_unix_home = index + 5 < chars.len()
                && chars[index] == '/'
                && (input[index..].starts_with("/home/")
                    || input[index..].starts_with("/Users/")
                    || input[index..].starts_with("/tmp/"));
            if is_drive || is_unix_home {
                while end < chars.len()
                    && !matches!(
                        chars[end],
                        ' ' | '\t' | '\n' | '\r' | '"' | '\'' | ')' | ',' | ';' | ']'
                    )
                {
                    end += 1;
                }
                out.push_str("<path>");
                index = end;
                continue;
            }
            let _ = start;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// Truncates a string to at most `max` characters (never splits a UTF-8
/// scalar) and appends an explicit marker so a truncated evidence string can
/// never be mistaken for the complete output.
pub fn truncate_chars(input: &str, max: usize) -> String {
    let count = input.chars().count();
    if count <= max {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max).collect();
    out.push_str(&format!("\n... [{count} chars total, truncated]"));
    out
}

/// Reads at most `cap` bytes from a file, returning the lossy text, whether
/// truncation happened, and the file size.
pub fn read_capped(path: &Path, cap: u64) -> std::io::Result<(String, bool, u64)> {
    let total = std::fs::metadata(path)?.len();
    let mut file = std::fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.by_ref().take(cap).read_to_end(&mut buffer)?;
    Ok((
        String::from_utf8_lossy(&buffer).into_owned(),
        total > cap,
        total,
    ))
}

/// Formats a `SystemTime` as `YYYY-MM-DDTHH:MM:SSZ` (UTC), without any crate.
pub fn format_utc(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's civil-from-days algorithm (proleptic Gregorian calendar).
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn safe_relative_paths() {
        for ok in [".", "app", "cases/app/src/main.rv", "a/./b"] {
            assert!(is_safe_relative_path(ok), "{ok}");
        }
        for bad in ["", "..", "../x", "a/../b", "/abs", "C:\\abs", "C:/abs"] {
            assert!(!is_safe_relative_path(bad), "{bad}");
        }
    }

    #[test]
    fn absolute_looking_cli_args() {
        assert!(looks_absolute("C:\\tmp\\x"));
        assert!(looks_absolute("/tmp/x"));
        assert!(!looks_absolute("proj"));
        assert!(!looks_absolute("check"));
    }

    #[test]
    fn sanitizer_replaces_repo_temp_and_home() {
        let repo = Path::new("D:\\App\\Ravum");
        let temp = Path::new("C:\\Temp\\ravum-conformance-1-2");
        let sanitizer = Sanitizer::new(repo, temp, None);
        let text = sanitizer.apply(
            "error at D:\\App\\Ravum\\conformance\\cases\\x and C:\\Temp\\ravum-conformance-1-2\\cases\\x",
        );
        assert!(text.contains("<repo>"));
        assert!(text.contains("<tmp>"));
        assert!(!text.contains("D:\\App\\Ravum"));
        assert!(!text.contains("C:\\Temp\\ravum-conformance"));
    }

    #[test]
    fn sanitizer_scrubs_unknown_absolute_paths() {
        let sanitizer = Sanitizer::new(Path::new("/nonexistent-repo"), Path::new("/tmp/xyz"), None);
        let text = sanitizer.apply("failed in D:\\Other\\user\\file.rv:12");
        assert!(text.contains("<path>"), "{text}");
        assert!(!text.contains("D:\\Other"));
    }

    #[test]
    fn truncation_never_splits_utf8() {
        let text = "héllo 😀 world";
        let truncated = truncate_chars(text, 3);
        assert!(truncated.starts_with("hél"));
        assert!(truncated.contains("truncated"));
        assert_eq!(truncate_chars("short", 10), "short");
    }

    #[test]
    fn utc_formatting_matches_known_instants() {
        let epoch = UNIX_EPOCH;
        assert_eq!(format_utc(epoch), "1970-01-01T00:00:00Z");
        assert_eq!(
            format_utc(epoch + Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00Z"
        );
        assert_eq!(
            format_utc(epoch + Duration::from_secs(1_700_000_000)),
            "2023-11-14T22:13:20Z"
        );
        assert_eq!(
            format_utc(epoch + Duration::from_secs(1_762_702_066)),
            "2025-11-09T15:27:46Z"
        );
    }

    #[test]
    fn repo_root_is_found_from_a_nested_directory() {
        let cwd = std::env::current_dir().unwrap();
        let root = find_repo_root(&cwd);
        assert!(root.join("Cargo.toml").is_file());
    }
}
