//! Fetch failures.

use confit_model::sha::Sha;
use thiserror::Error;

use crate::faults::AccessFault;

/// Fetch failure shapes.
#[derive(Debug, Clone, Error)]
pub enum FetchError {
    /// Failed download holding the url with the transport reason.
    #[error("cannot fetch '{url}': {reason}")]
    Download {
        /// The url under fetching.
        url: String,
        /// The transport reason.
        reason: String,
    },
    /// Failed status holding the url with the status code.
    #[error("cannot fetch '{url}': status {code}")]
    Status {
        /// The url under fetching.
        url: String,
        /// The refusing status code.
        code: u16,
    },
    /// Timed-out fetch holding the url alone.
    #[error("cannot fetch '{url}': timeout")]
    Timeout {
        /// The url under fetching.
        url: String,
    },
    /// Failed cache read holding the url with the fault.
    #[error("cannot fetch '{url}': {fault}")]
    Read {
        /// The url under fetching.
        url: String,
        /// The read fault cause.
        fault: AccessFault,
    },
    /// Digest mismatch holding the url with want and got.
    #[error("sha256 mismatch for '{url}': want {want}, got {got}")]
    DigestMismatch {
        /// The url under checking.
        url: String,
        /// The wanted digest.
        want: Sha,
        /// The seen digest.
        got: Sha,
    },
    /// Unscripted url holding the url alone.
    #[error("cannot fetch '{url}': no scripted body")]
    Unscripted {
        /// The url under fetching.
        url: String,
    },
    /// Failed cache write holding the url with the fault.
    #[error("cannot fetch '{url}': {fault}")]
    Write {
        /// The url under caching.
        url: String,
        /// The write fault cause.
        fault: AccessFault,
    },
    /// Unknown cache write failure holding the url with message.
    #[error("cannot fetch '{url}': {message}")]
    WriteUnknown {
        /// The url under caching.
        url: String,
        /// The failure message under caching.
        message: String,
    },
}

impl FetchError {
    /// Maps one cache io failure at the url into domain language.
    pub fn from_io(url: &str, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        if kind == std::io::ErrorKind::TimedOut {
            return Self::Timeout {
                url: url.to_owned(),
            };
        }
        match AccessFault::interpret(kind) {
            Some(fault) => Self::Read {
                url: url.to_owned(),
                fault,
            },
            None => Self::Read {
                url: url.to_owned(),
                fault: AccessFault::Unknown { message },
            },
        }
    }

    /// Maps one cache write io failure at the url into domain language.
    pub fn from_write_io(url: &str, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match AccessFault::interpret(kind) {
            Some(fault) => Self::Write {
                url: url.to_owned(),
                fault,
            },
            None => Self::WriteUnknown {
                url: url.to_owned(),
                message,
            },
        }
    }
}

/// Fetch result alias.
pub type Result<T> = std::result::Result<T, FetchError>;
