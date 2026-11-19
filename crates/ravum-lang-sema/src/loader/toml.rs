use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TomlValue {
    Str(String),
    Table(Vec<(String, String)>),
}

#[derive(Debug)]
pub(super) struct TomlSection {
    pub(super) span: Span,
    pub(super) keys: BTreeMap<String, (TomlValue, Span)>,
}

impl Default for TomlSection {
    fn default() -> Self {
        Self {
            span: Span::dummy(),
            keys: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct ParsedToml {
    pub(super) sections: BTreeMap<String, TomlSection>,
}

/// Parse the strict TOML subset of `ravum.toml`:
/// `[section]` headers, `key = "string"` pairs and one-line
/// `key = { inner = "value", ... }` inline tables. Comments (`#`), blank lines
/// and single-line basic strings with the usual escapes are supported. Arrays,
/// multiline strings/tables, numbers and booleans are rejected.
pub(super) fn parse_toml(text: &str, file: FileId) -> (ParsedToml, DiagnosticBag) {
    let mut diags = DiagnosticBag::new();
    let mut parsed = ParsedToml::default();
    let mut current = String::new();
    let mut seen_sections: BTreeSet<String> = BTreeSet::new();
    let mut offset = 0u32;
    let bytes = text.as_bytes();
    let mut line_no = 0u32;

    for raw_line in text.split_inclusive('\n') {
        line_no += 1;
        let line_start = offset;
        offset = offset.saturating_add(raw_line.len() as u32);
        let line = raw_line.trim_end_matches(['\n', '\r']);
        let body = strip_comment(line);
        let trimmed = body.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lead = body.len() - body.trim_start().len();
        let start = line_start.saturating_add(lead as u32);
        let end = start.saturating_add(trimmed.len() as u32);
        let line_span = Span::new(
            file,
            start.min(bytes.len() as u32),
            end.min(bytes.len() as u32),
        );

        if trimmed.starts_with('[') {
            if !trimmed.ends_with(']') || trimmed.len() < 2 {
                diags.push(Diagnostic::error(
                    "E-MOD-MANIFEST",
                    line_span,
                    format!("invalid section header on line {line_no}: `{trimmed}`"),
                ));
                continue;
            }
            let name = trimmed[1..trimmed.len() - 1].trim().to_string();
            if name.is_empty() || !name.chars().all(is_toml_bare_key_char) {
                diags.push(Diagnostic::error(
                    "E-MOD-MANIFEST",
                    line_span,
                    format!("invalid section name `{name}` on line {line_no}"),
                ));
                continue;
            }
            if !seen_sections.insert(name.clone()) {
                diags.push(Diagnostic::error(
                    "E-MOD-MANIFEST",
                    line_span,
                    format!("duplicate section `[{name}]` on line {line_no}"),
                ));
                continue;
            }
            parsed.sections.entry(name.clone()).or_default().span = line_span;
            current = name;
            continue;
        }

        let Some((key, value_text)) = split_key_value(trimmed) else {
            diags.push(Diagnostic::error(
                "E-MOD-MANIFEST",
                line_span,
                format!("expected `key = value` on line {line_no}"),
            ));
            continue;
        };
        let key = key.trim().to_string();
        if key.is_empty() || !key.chars().all(is_toml_bare_key_char) {
            diags.push(Diagnostic::error(
                "E-MOD-MANIFEST",
                line_span,
                format!("invalid key `{key}` on line {line_no}"),
            ));
            continue;
        }
        let value_text = value_text.trim();
        let value_start = start + (trimmed.len() - value_text.len()) as u32;
        let value_span = Span::new(
            file,
            value_start.min(bytes.len() as u32),
            (value_start as usize + value_text.len()).min(bytes.len()) as u32,
        );
        let Some(value) = parse_toml_value(value_text, file, value_start, &mut diags) else {
            continue;
        };
        let section = parsed.sections.entry(current.clone()).or_default();
        if section.keys.contains_key(&key) {
            diags.push(Diagnostic::error(
                "E-MOD-MANIFEST",
                line_span,
                format!("duplicate key `{key}` in section `[{current}]`"),
            ));
            continue;
        }
        section.keys.insert(key, (value, value_span));
    }

    (parsed, diags)
}

pub(super) fn is_toml_bare_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'
}

pub(super) fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '#' {
            return &line[..i];
        }
    }
    line
}

pub(super) fn split_key_value(s: &str) -> Option<(&str, &str)> {
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '=' {
            return Some((&s[..i], &s[i + 1..]));
        }
    }
    None
}

pub(super) fn parse_toml_value(
    s: &str,
    file: FileId,
    base: u32,
    diags: &mut DiagnosticBag,
) -> Option<TomlValue> {
    if s.starts_with('"') {
        let (value, _) = parse_basic_string(s, file, base, diags)?;
        return Some(TomlValue::Str(value));
    }
    if s.starts_with('{') {
        if !s.ends_with('}') || s.len() < 2 {
            diags.push(Diagnostic::error(
                "E-MOD-MANIFEST",
                Span::new(file, base, base + s.len() as u32),
                "inline table must be closed with `}` on the same line",
            ));
            return None;
        }
        let inner = &s[1..s.len() - 1];
        let mut entries = Vec::new();
        if !inner.trim().is_empty() {
            for part in split_top_level_commas(inner) {
                let part_start = base + 1 + inner.find(part).unwrap_or(0) as u32;
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Some((k, v)) = split_key_value(trimmed) else {
                    diags.push(Diagnostic::error(
                        "E-MOD-MANIFEST",
                        Span::new(file, part_start, part_start + part.len() as u32),
                        format!("invalid inline table entry `{trimmed}`"),
                    ));
                    return None;
                };
                let k = k.trim();
                let v = v.trim();
                if k.is_empty() || !k.chars().all(is_toml_bare_key_char) || !v.starts_with('"') {
                    diags.push(Diagnostic::error(
                        "E-MOD-MANIFEST",
                        Span::new(file, part_start, part_start + part.len() as u32),
                        format!("inline table entry `{trimmed}` must be `key = \"value\"`"),
                    ));
                    return None;
                }
                // `base` is only used for diagnostics; derive the value offset
                // from the original slice.
                let value_offset =
                    base + 1 + inner.find(part).unwrap_or(0) as u32 + (part.len() - v.len()) as u32;
                let (value, _) = parse_basic_string(v, file, value_offset, diags)?;
                if entries
                    .iter()
                    .any(|(existing, _): &(String, String)| existing == k)
                {
                    diags.push(Diagnostic::error(
                        "E-MOD-MANIFEST",
                        Span::new(file, part_start, part_start + part.len() as u32),
                        format!("duplicate inline table key `{k}`"),
                    ));
                    return None;
                }
                entries.push((k.to_string(), value));
            }
        }
        return Some(TomlValue::Table(entries));
    }
    diags.push(Diagnostic::error(
        "E-MOD-MANIFEST",
        Span::new(file, base, base + s.len() as u32),
        format!("unsupported TOML value `{s}`; only strings and inline tables are accepted"),
    ));
    None
}

pub(super) fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0usize;
    for (i, ch) in s.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == ',' {
            out.push(&s[start..i]);
            start = i + 1;
        }
    }
    out.push(&s[start..]);
    out
}

/// Parse a single-line TOML basic string starting at `s[0] == '"'`.
pub(super) fn parse_basic_string(
    s: &str,
    file: FileId,
    base: u32,
    diags: &mut DiagnosticBag,
) -> Option<(String, usize)> {
    debug_assert!(s.starts_with('"'));
    let mut value = String::new();
    let mut chars = s.char_indices().skip(1);
    while let Some((i, ch)) = chars.next() {
        match ch {
            '"' => return Some((value, i + 1)),
            '\\' => {
                let Some((_, esc)) = chars.next() else {
                    break;
                };
                match esc {
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    'r' => value.push('\r'),
                    other => {
                        diags.push(Diagnostic::error(
                            "E-MOD-MANIFEST",
                            Span::new(file, base, base + s.len() as u32),
                            format!("unsupported escape sequence `\\{other}` in string"),
                        ));
                        return None;
                    }
                }
            }
            c if c.is_control() => {
                diags.push(Diagnostic::error(
                    "E-MOD-MANIFEST",
                    Span::new(file, base, base + s.len() as u32),
                    "control character in string",
                ));
                return None;
            }
            other => value.push(other),
        }
    }
    diags.push(Diagnostic::error(
        "E-MOD-MANIFEST",
        Span::new(file, base, base + s.len() as u32),
        "unterminated string",
    ));
    None
}

/// Validate a parsed manifest against `spec/language/modules.md` §5 and §8.
pub(super) fn parse_manifest(text: &str, file: FileId) -> (Option<PackageManifest>, DiagnosticBag) {
    let (parsed, mut diags) = parse_toml(text, file);
    if diags.has_errors() {
        return (None, diags);
    }

    for (name, section) in &parsed.sections {
        match name.as_str() {
            "package" | "dependencies" | "native" => {}
            "build" | "scripts" => {
                diags.push(Diagnostic::error(
                    "E-MOD-BUILD-SCRIPT",
                    section.span,
                    format!(
                        "section `[{name}]` is not supported: Core 1.0 has no build scripts \
                         (spec/language/modules.md MOD-5.10)"
                    ),
                ));
            }
            other if other.starts_with("dependencies.") => {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    section.span,
                    format!(
                        "section `[{other}]` is not supported: declare dependencies as \
                         `name = {{ path = \"...\" }}` under `[dependencies]`"
                    ),
                ));
            }
            _ => {
                diags.push(Diagnostic::error(
                    "E-MOD-MANIFEST",
                    section.span,
                    format!("unknown section `[{name}]` (manifest is strict)"),
                ));
            }
        }
    }

    let package = parsed.sections.get("package");
    if package.is_none() {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            Span::new(file, 0, 0),
            "missing required `[package]` section",
        ));
        return (None, diags);
    }
    let package = package.expect("checked above");

    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut edition: Option<String> = None;
    for (key, (value, span)) in &package.keys {
        match key.as_str() {
            "name" | "version" | "edition" => {
                let TomlValue::Str(text) = value else {
                    diags.push(Diagnostic::error(
                        "E-MOD-MANIFEST",
                        *span,
                        format!("`package.{key}` must be a string"),
                    ));
                    continue;
                };
                let slot = match key.as_str() {
                    "name" => &mut name,
                    "version" => &mut version,
                    _ => &mut edition,
                };
                *slot = Some(text.clone());
            }
            other => diags.push(Diagnostic::error(
                "E-MOD-MANIFEST",
                *span,
                format!("unknown key `package.{other}` (Core 1.0 manifest is strict)"),
            )),
        }
    }

    let Some(name) = name else {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            package.span,
            "missing required `package.name`",
        ));
        return (None, diags);
    };
    if !is_valid_package_name(&name) {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            package.span,
            format!("invalid package name `{name}`: expected `[a-z][a-z0-9_-]*`"),
        ));
    }
    let Some(version) = version else {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            package.span,
            "missing required `package.version`",
        ));
        return (None, diags);
    };
    if !is_valid_version(&version) {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            package.span,
            format!(
                "invalid version `{version}`: expected `MAJOR.MINOR.PATCH` (decimal integers, \
                 no leading zeros, no pre-release)"
            ),
        ));
    }
    let Some(edition) = edition else {
        diags.push(Diagnostic::error(
            "E-MOD-MANIFEST",
            package.span,
            "missing required `package.edition`",
        ));
        return (None, diags);
    };
    if edition != MANIFEST_EDITION {
        diags.push(Diagnostic::error(
            "E-UNSUPPORTED-EDITION",
            package.span,
            format!(
                "unsupported edition `{edition}`: this toolchain accepts only \
                 `edition = \"{MANIFEST_EDITION}\"`"
            ),
        ));
    }

    let mut dependencies = Vec::new();
    if let Some(section) = parsed.sections.get("dependencies") {
        for (dep_name, (value, span)) in &section.keys {
            if !is_valid_package_name(dep_name) {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    *span,
                    format!("invalid dependency name `{dep_name}`: expected `[a-z][a-z0-9_-]*`"),
                ));
                continue;
            }
            let TomlValue::Table(entries) = value else {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    *span,
                    format!(
                        "dependency `{dep_name}` must be `{dep_name} = {{ path = \"...\" }}` \
                         (no registry, no version range, no build script)"
                    ),
                ));
                continue;
            };
            if entries.len() != 1 || entries[0].0 != "path" {
                diags.push(Diagnostic::error(
                    "E-MOD-DEPENDENCY",
                    *span,
                    format!(
                        "dependency `{dep_name}` must contain exactly one `path` key \
                         (local path dependencies only)"
                    ),
                ));
                continue;
            }
            // An inline-table value is a basic string by construction.
            let path = &entries[0].1;
            dependencies.push(DependencyDecl {
                name: dep_name.clone(),
                path: path.clone(),
                span: *span,
            });
        }
    }
    dependencies.sort_by(|a, b| a.name.cmp(&b.name));

    let mut native = Vec::new();
    if let Some(section) = parsed.sections.get("native") {
        for (lib_name, (value, span)) in &section.keys {
            if let Some(decl) = parse_native_entry(lib_name, value, *span, &mut diags) {
                native.push(decl);
            }
        }
    }
    native.sort_by(|a, b| a.name.cmp(&b.name));

    if diags.has_errors() {
        return (None, diags);
    }
    (
        Some(PackageManifest {
            name,
            version,
            edition,
            dependencies,
            native,
        }),
        diags,
    )
}
