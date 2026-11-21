//! Validated fetch urls.

use serde::{Deserialize, Serialize};
use url::Url as ParsedUrl;

/// A fetch url with http or https scheme and a present host.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Url {
    /// The url text.
    inner: String,
}

/// A rejected fetch url.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid fetch url '{found}', expected http or https with a host")]
pub struct UrlError {
    /// The rejected url text.
    found: String,
}

impl Url {
    /// Builds a fetch url from text.
    ///
    /// # Errors
    ///
    /// - [`UrlError`] when the text holds a non-http scheme.
    /// - [`UrlError`] when the text holds a missing host.
    pub fn new(text: String) -> Result<Self, UrlError> {
        if valid_url(&text) {
            Ok(Self { inner: text })
        } else {
            Err(UrlError { found: text })
        }
    }

    /// Returns the url text.
    pub fn as_str(&self) -> &str {
        &self.inner
    }
}

/// Returns whether the text parses with http or https and a host.
fn valid_url(text: &str) -> bool {
    match ParsedUrl::parse(text) {
        Ok(parsed) => {
            (parsed.scheme() == "http" || parsed.scheme() == "https") && parsed.has_host()
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_builds_for_http_and_https_with_a_host() {
        for text in [
            "https://example.com/tool.bin",
            "http://example.com/tool.bin",
            "https://localhost:8080/tool.bin",
        ] {
            match Url::new(text.to_owned()) {
                Ok(url) => assert_eq!(url.as_str(), text),
                Err(error) => panic!("the url builds: {error}"),
            }
        }
    }

    #[test]
    fn url_rejects_a_missing_host_or_scheme() {
        for text in [
            "",
            "not a url",
            "gopher://x",
            "ftp://example.com/tool.bin",
            "file:///etc/passwd",
            "https://",
            "example.com/tool.bin",
        ] {
            assert!(
                Url::new(text.to_owned()).is_err(),
                "the url rejects the text: {text}"
            );
        }
    }

    #[test]
    fn url_error_names_the_rejected_text() {
        match Url::new("gopher://x".to_owned()) {
            Ok(_) => panic!("the url rejects a gopher scheme"),
            Err(error) => {
                assert_eq!(
                    error.to_string(),
                    "invalid fetch url 'gopher://x', \
                     expected http or https with a host"
                );
                assert_eq!(error.found, "gopher://x");
            }
        }
    }

    #[test]
    fn url_keeps_a_string_shape_through_serde() {
        let url = Url::new("https://example.com/tool.bin".to_owned()).unwrap();
        let text = serde_json::to_string(&url).unwrap();
        assert_eq!(text, "\"https://example.com/tool.bin\"");
        match serde_json::from_str::<Url>(&text) {
            Ok(back) => assert_eq!(back, url),
            Err(error) => panic!("the url decodes: {error}"),
        }
    }
}
