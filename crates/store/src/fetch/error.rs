//! Fetch failures.

use confit_model::sha::Sha;
use thiserror::Error;

/// Fetch failure shapes.
#[derive(Debug, Error)]
pub enum FetchError {
    /// Failed download holding the url with the transport reason.
    #[error("cannot fetch '{url}': {reason}")]
    Download {
        /// Holds the url under fetching.
        url: String,
        /// Holds the transport reason.
        reason: String,
    },
    /// Failed status holding the url with the status code.
    #[error("cannot fetch '{url}': status {code}")]
    Status {
        /// Holds the url under fetching.
        url: String,
        /// Holds the refusing status code.
        code: u16,
    },
    /// Timed-out fetch holding the url alone.
    #[error("cannot fetch '{url}': timeout")]
    Timeout {
        /// Holds the url under fetching.
        url: String,
    },
    /// Missing cache holding the url.
    #[error("cannot fetch '{url}': missing cache")]
    Missing {
        /// Holds the url under fetching.
        url: String,
    },
    /// Denied cache holding the url.
    #[error("cannot fetch '{url}': permission denied")]
    Denied {
        /// Holds the url under fetching.
        url: String,
    },
    /// Unknown failure holding the url with message.
    #[error("cannot fetch '{url}': {message}")]
    Unknown {
        /// Holds the url under fetching.
        url: String,
        /// Holds the failure message under fetching.
        message: String,
    },
    /// Digest mismatch holding the url with want and got.
    #[error("sha256 mismatch for '{url}': want {want}, got {got}")]
    DigestMismatch {
        /// Holds the url under checking.
        url: String,
        /// Holds the wanted digest.
        want: Sha,
        /// Holds the seen digest.
        got: Sha,
    },
    /// Unscripted url holding the url alone.
    #[error("cannot fetch '{url}': no scripted body")]
    Unscripted {
        /// Holds the url under fetching.
        url: String,
    },
}

impl FetchError {
    /// Maps one cache io failure at the url into domain language.
    pub fn from_io(url: &str, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match kind {
            std::io::ErrorKind::NotFound => Self::Missing {
                url: url.to_owned(),
            },
            std::io::ErrorKind::PermissionDenied => Self::Denied {
                url: url.to_owned(),
            },
            _ => Self::Unknown {
                url: url.to_owned(),
                message,
            },
        }
    }
}

/// Fetch result alias.
pub type Result<T> = std::result::Result<T, FetchError>;
