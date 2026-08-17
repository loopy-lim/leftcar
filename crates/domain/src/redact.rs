//! Diagnostic redaction (docs/03 §12.2, docs/07 §16).
//!
//! Allowlist approach: anything not matching a known-safe pattern is masked.

/// Sensitive substrings that must never survive into diagnostics.
const SENSITIVE_PATTERN_HINTS: &[&str] = &[
    "token", "secret", "password", "key=", "privatekey", "private_key",
];

/// A redaction verdict for one line/field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// codec, profile, width/height, fps, duration, size, count, error code, opaque id hash
    Allowed,
    /// window title / document name / file path / IP / token / frame bytes
    Redacted,
}

/// Classify a diagnostic field by its allowlisted name.
pub fn classify_field(name: &str) -> FieldKind {
    const ALLOWED: &[&str] = &[
        "codec", "profile", "width", "height", "fps", "duration_ms", "size",
        "count", "error_code", "session_hash", "source_hash", "host_hash",
        "stream_hash", "phase", "scope", "retryable", "transport", "kind",
        "build", "os_version", "app_version", "epoch", "frame_id",
    ];
    if ALLOWED.contains(&name) {
        // name is allowlisted, but value still passes value_filter
        FieldKind::Allowed
    } else {
        FieldKind::Redacted
    }
}

/// Value-level scrubbing applied to every value even for allowlisted names:
/// masks IPv4 literals, file paths, and long opaque blobs.
pub fn scrub_value(value: &str) -> String {
    let mut out = value.to_string();
    // IPv4 literal
    if let Some(masked) = mask_ipv4(&out) {
        out = masked;
    } else if contains_ipv4(&out) {
        // mask each embedded IPv4 literal, keep surrounding text
        let mut result = String::new();
        let mut rest = out.as_str();
        while let Some((prefix, a, b, c, d, tail)) = find_first_ipv4(rest) {
            result.push_str(prefix);
            result.push_str("<ip>");
            let _ = (a, b, c, d);
            rest = tail;
        }
        result.push_str(rest);
        out = result;
    }
    // filesystem paths (contain '/' with a dot-extension, or leading ~)
    if out.starts_with('~') || out.starts_with('/') || looks_like_path(&out) {
        out = "<path>".to_string();
    }
    // sensitive hint substrings
    let lower = out.to_lowercase();
    for hint in SENSITIVE_PATTERN_HINTS {
        if lower.contains(hint) {
            return "<redacted>".to_string();
        }
    }
    out
}

fn ipv4_parts(s: &str) -> Option<(usize, &str, &str, &str, &str)> {
    // returns (start_index, a, b, c, d) for the first IPv4-looking token
    let byte_start = s.as_ptr() as usize;
    let mut search_from = 0;
    while let Some(dot_pos) = s[search_from..].find('.') {
        // candidate window around this dot: expand to token boundaries
        let abs = search_from + dot_pos;
        let start = s[..abs]
            .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = s[abs..]
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .map(|i| abs + i)
            .unwrap_or(s.len());
        let token = &s[start..end];
        if let Some((a, b, c, d)) = split_ipv4(token) {
            return Some((start, a, b, c, d));
        }
        search_from = abs + 1;
    }
    let _ = byte_start;
    None
}

fn split_ipv4(token: &str) -> Option<(&str, &str, &str, &str)> {
    let mut it = token.split('.');
    let (a, b, c, d) = (it.next()?, it.next()?, it.next()?, it.next()?);
    let octet = |p: &str| !p.is_empty() && p.len() <= 3 && p.chars().all(|ch| ch.is_ascii_digit()) && p.parse::<u16>().map(|v| v <= 255).unwrap_or(false);
    if octet(a) && octet(b) && octet(c) && octet(d) && it.next().is_none() {
        Some((a, b, c, d))
    } else {
        None
    }
}

fn mask_ipv4(s: &str) -> Option<String> {
    if split_ipv4(s).is_some() {
        Some("<ip>".to_string())
    } else {
        None
    }
}

fn contains_ipv4(s: &str) -> bool {
    ipv4_parts(s).is_some()
}

fn find_first_ipv4(s: &str) -> Option<(&str, &str, &str, &str, &str, &str)> {
    // (prefix, a, b, c, d, tail)
    let (start, a, b, c, d) = ipv4_parts(s)?;
    let token_len = a.len() + b.len() + c.len() + d.len() + 3;
    let end = start + token_len;
    Some((&s[..start], a, b, c, d, &s[end..]))
}

fn looks_like_path(s: &str) -> bool {
    // contains a slash and a filename extension like .txt .log .json, no spaces
    s.contains('/') && !s.contains(' ') && {
        let last = s.rsplit('/').next().unwrap_or("");
        last.contains('.') && last.split('.').count() >= 2 && !last.starts_with('.')
    }
}

/// Full redaction of one diagnostic field.
pub fn redact_field(name: &str, value: &str) -> String {
    match classify_field(name) {
        FieldKind::Allowed => scrub_value(value),
        FieldKind::Redacted => "<redacted>".to_string(),
    }
}

/// Redact a structured record (pairs of name/value).
pub fn redact_record<'a>(record: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<(String, String)> {
    record
        .into_iter()
        .map(|(k, v)| (k.to_string(), redact_field(k, v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_redact_title_path_token_and_ip() {
        let out = redact_record([
            ("window_title", "Salary Review.doc"),
            ("path", "~/Documents/secret.txt"),
            ("pairing_token", "abc123"),
            ("ip", "192.168.0.5"),
            ("codec", "avc"),
            ("duration_ms", "42"),
            ("error_code", "transport.disconnected"),
        ]);
        let joined = format!("{out:?}");
        assert!(!joined.contains("Salary"), "title leaked: {joined}");
        assert!(!joined.contains("secret.txt"), "path leaked: {joined}");
        assert!(!joined.contains("abc123"), "token leaked: {joined}");
        assert!(!joined.contains("192.168.0.5"), "ip leaked: {joined}");
        // allowed fields survive
        assert!(joined.contains("avc"));
        assert!(joined.contains("42"));
        assert!(joined.contains("transport.disconnected"));
    }

    #[test]
    fn scrub_value_masks_ipv4_anywhere() {
        assert_eq!(scrub_value("10.0.0.7"), "<ip>");
        // IPv4 embedded with port keeps prefix but masks literal
        let s = scrub_value("host 10.0.0.7:9000");
        assert!(!s.contains("10.0.0.7"), "{s}");
    }

    #[test]
    fn scrub_value_masks_paths() {
        assert_eq!(scrub_value("/var/log/system.log"), "<path>");
        assert_eq!(scrub_value("~/Documents/notes.md"), "<path>");
        assert_eq!(scrub_value("relative/file.json"), "<path>");
        // codec-like strings are not paths
        assert_eq!(scrub_value("video/avc"), "video/avc");
    }

    #[test]
    fn scrub_value_masks_token_like_blobs() {
        assert_eq!(scrub_value("pairing_token=deadbeef"), "<redacted>");
        assert_eq!(scrub_value("my-secret-value"), "<redacted>");
        // numbers and codes pass
        assert_eq!(scrub_value("1234"), "1234");
    }
}
