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

impl RequestExtras {
    /// Decorate a request.
    pub fn apply(&self, mut rb: RequestBuilder) -> RequestBuilder {
        for (k, v) in &self.headers {
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
}
