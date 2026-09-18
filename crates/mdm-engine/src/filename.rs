//! Picking a file name for a download and making it safe for every OS.

use percent_encoding::percent_decode_str;
use url::Url;

/// Used when neither the server nor the URL names the file.
pub const DEFAULT_FILENAME: &str = "download.bin";

/// Longest name we produce, in bytes (Windows and most Linux FS allow 255).
pub const MAX_FILENAME_BYTES: usize = 200;

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Resolve a file name: `Content-Disposition` (`filename*`, then `filename`),
/// then the URL's last path segment, then [`DEFAULT_FILENAME`]. Always sanitised.
pub fn filename_from(content_disposition: Option<&str>, url: &Url) -> String {
    if let Some(cd) = content_disposition {
        if let Some(name) = from_content_disposition(cd) {
            return sanitize(&name);
        }
    }
    let from_url = url
        .path_segments()
        .and_then(|mut s| s.next_back().map(str::to_owned))
        .filter(|s| !s.is_empty())
        .map(|s| percent_decode_str(&s).decode_utf8_lossy().into_owned());
    match from_url {
        Some(name) => sanitize(&name),
        None => DEFAULT_FILENAME.to_owned(),
    }
}

fn from_content_disposition(cd: &str) -> Option<String> {
    let params: Vec<(String, String)> = cd
        .split(';')
        .skip(1)
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        })
        .collect();
    // RFC 5987: filename*=charset'lang'percent-encoded
    if let Some((_, v)) = params.iter().find(|(k, _)| k == "filename*") {
        let encoded = v.rsplit('\'').next().unwrap_or(v);
        let decoded = percent_decode_str(encoded).decode_utf8_lossy().into_owned();
        if !decoded.is_empty() {
            return Some(decoded);
        }
    }
    if let Some((_, v)) = params.iter().find(|(k, _)| k == "filename") {
        let v = v.trim_matches('"');
        if !v.is_empty() {
            return Some(v.to_owned());
        }
    }
    None
}

/// Make `name` safe: no path separators or control characters, no leading or
/// trailing dots and spaces, no Windows reserved device names, at most
/// [`MAX_FILENAME_BYTES`] (cut on a char boundary, keeping the extension).
pub fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    out = out.trim_matches(|c: char| c == '.' || c == ' ').to_owned();
    if out.is_empty() {
        return DEFAULT_FILENAME.to_owned();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    if out.len() > MAX_FILENAME_BYTES {
        out = truncate_keeping_extension(&out);
    }
    out
}

fn truncate_keeping_extension(name: &str) -> String {
    // Extension = everything after the first dot in the last 20 bytes, if any
    // ("x.tar.gz" keeps ".tar.gz"; a 100-byte "extension" is not one).
    let ext_start = name
        .char_indices()
        .filter(|(i, c)| *c == '.' && name.len() - *i <= 20)
        .map(|(i, _)| i)
        .next();
    let (stem, ext) = match ext_start {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    let budget = MAX_FILENAME_BYTES.saturating_sub(ext.len()).max(1);
    let mut cut = budget.min(stem.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{}", &stem[..cut], ext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn prefers_rfc5987_filename_star() {
        let cd = r#"attachment; filename="fallback.txt"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"#;
        assert_eq!(filename_from(Some(cd), &u("https://x/y.zip")), "résumé.pdf");
    }

    #[test]
    fn then_quoted_filename() {
        let cd = r#"attachment; filename="report 2026.pdf""#;
        assert_eq!(
            filename_from(Some(cd), &u("https://x/y.zip")),
            "report 2026.pdf"
        );
    }

    #[test]
    fn then_bare_filename() {
        assert_eq!(
            filename_from(Some("inline; filename=a.iso"), &u("https://x/y")),
            "a.iso"
        );
    }

    #[test]
    fn then_url_path_percent_decoded() {
        assert_eq!(
            filename_from(None, &u("https://x/dl/My%20File.tar.gz?token=1")),
            "My File.tar.gz"
        );
    }

    #[test]
    fn then_default() {
        assert_eq!(filename_from(None, &u("https://x/")), DEFAULT_FILENAME);
        assert_eq!(
            filename_from(Some("attachment"), &u("https://x")),
            DEFAULT_FILENAME
        );
    }

    #[test]
    fn sanitize_strips_separators_and_reserved_names() {
        assert_eq!(sanitize("a/b\\c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize("  ..hidden.. "), "hidden");
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("com1.txt"), "_com1.txt");
        assert_eq!(sanitize("bad\u{0}name\n"), "bad_name_");
        assert_eq!(sanitize(""), DEFAULT_FILENAME);
        assert_eq!(sanitize("..."), DEFAULT_FILENAME);
    }

    #[test]
    fn sanitize_caps_length_keeping_extension() {
        let long = format!("{}.tar.gz", "x".repeat(300));
        let out = sanitize(&long);
        assert!(out.len() <= MAX_FILENAME_BYTES);
        assert!(out.ends_with(".tar.gz"));
    }

    #[test]
    fn sanitize_cap_respects_utf8_boundaries() {
        let long = "é".repeat(150); // 300 bytes
        let out = sanitize(&long);
        assert!(out.len() <= MAX_FILENAME_BYTES);
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }
}
