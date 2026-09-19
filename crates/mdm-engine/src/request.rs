//! Per-download request decorations: extra headers and browser cookies.

use reqwest::RequestBuilder;

/// A cookie the browser extension captured for the download URL.
#[derive(Clone, Debug)]
pub struct Cookie {
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
}

/// Headers and cookies applied to every request of one download.
#[derive(Clone, Debug, Default)]
pub struct RequestExtras {
    /// Sent verbatim (Referer, Authorization …).
    pub headers: Vec<(String, String)>,
    /// Joined into one `Cookie` header.
    pub cookies: Vec<Cookie>,
}

/// Credentials a redirect to another origin must never carry along.
const CREDENTIAL_HEADERS: [&str; 3] = ["cookie", "authorization", "proxy-authorization"];
/// Headers the engine sets itself; a caller-supplied value is never sent.
const ENGINE_HEADERS: [&str; 3] = ["range", "host", "content-length"];

impl RequestExtras {
    /// Decorate a request. Caller headers named `Range`, `Host` or
    /// `Content-Length` are skipped — the engine owns those.
    pub fn apply(&self, mut rb: RequestBuilder) -> RequestBuilder {
        for (k, v) in &self.headers {
            if ENGINE_HEADERS.iter().any(|h| k.eq_ignore_ascii_case(h)) {
                continue;
            }
            rb = rb.header(k, v);
        }
        if !self.cookies.is_empty() {
            let joined = self
                .cookies
                .iter()
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
                .join("; ");
            rb = rb.header(reqwest::header::COOKIE, joined);
        }
        rb
    }

    /// A copy that carries no credentials: used when a redirect left the
    /// origin the caller's cookies and auth headers were meant for.
    pub fn without_credentials(&self) -> RequestExtras {
        RequestExtras {
            headers: self
                .headers
                .iter()
                .filter(|(k, _)| !CREDENTIAL_HEADERS.iter().any(|c| k.eq_ignore_ascii_case(c)))
                .cloned()
                .collect(),
            cookies: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_headers_and_joins_cookies() {
        let x = RequestExtras {
            headers: vec![("Referer".into(), "https://a/".into())],
            cookies: vec![
                Cookie {
                    name: "s".into(),
                    value: "1".into(),
                },
                Cookie {
                    name: "t".into(),
                    value: "2".into(),
                },
            ],
        };
        let client = reqwest::Client::new();
        let req = x
            .apply(client.get("https://example.invalid/"))
            .build()
            .unwrap();
        assert_eq!(req.headers().get("referer").unwrap(), "https://a/");
        assert_eq!(req.headers().get("cookie").unwrap(), "s=1; t=2");
    }

    #[test]
    fn no_cookies_means_no_cookie_header() {
        let client = reqwest::Client::new();
        let req = RequestExtras::default()
            .apply(client.get("https://example.invalid/"))
            .build()
            .unwrap();
        assert!(req.headers().get("cookie").is_none());
    }

    #[test]
    fn without_credentials_strips_cookies_and_auth_headers() {
        let x = RequestExtras {
            headers: vec![
                ("Referer".into(), "https://a/".into()),
                ("authorization".into(), "Bearer t".into()),
                ("Proxy-Authorization".into(), "Basic p".into()),
                ("Cookie".into(), "raw=1".into()),
            ],
            cookies: vec![Cookie {
                name: "s".into(),
                value: "1".into(),
            }],
        };
        let y = x.without_credentials();
        assert_eq!(
            y.headers,
            vec![("Referer".to_string(), "https://a/".to_string())]
        );
        assert!(y.cookies.is_empty());
    }

    #[test]
    fn apply_never_sends_engine_owned_headers() {
        let x = RequestExtras {
            headers: vec![
                ("Range".into(), "bytes=0-1".into()),
                ("host".into(), "evil".into()),
                ("Content-Length".into(), "5".into()),
            ],
            cookies: vec![],
        };
        let req = x
            .apply(reqwest::Client::new().get("https://example.invalid/"))
            .build()
            .unwrap();
        assert!(req.headers().get("range").is_none());
        assert!(req.headers().get("content-length").is_none());
        assert_ne!(
            req.headers().get("host").map(|v| v.to_str().unwrap()),
            Some("evil")
        );
    }
}
