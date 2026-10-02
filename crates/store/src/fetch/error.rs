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
    /// Failed cache read holding the url with the fault.
    #[error("cannot fetch '{url}': {fault}")]
    Read {
        /// Holds the url under fetching.
        url: String,
        /// Holds the read fault cause.
        fault: AccessFault,
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
    /// Failed cache write holding the url with the fault.
    #[error("cannot fetch '{url}': {fault}")]
    Write {
        /// Holds the url under caching.
        url: String,
        /// Holds the write fault cause.
        fault: AccessFault,
    },
    /// Unknown cache write failure holding the url with message.
    #[error("cannot fetch '{url}': {message}")]
    WriteUnknown {
        /// Holds the url under caching.
        url: String,
        /// Holds the failure message under caching.
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
