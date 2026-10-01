//! Fetch failures.

use confit_model::sha::Sha;
use thiserror::Error;

use crate::faults::AccessFault;

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
        let message = error.to_string();
        match error.kind() {
            std::io::ErrorKind::TimedOut => Self::Timeout {
                url: url.to_owned(),
            },
            kind @ (std::io::ErrorKind::NotFound
            | std::io::ErrorKind::PermissionDenied
            | std::io::ErrorKind::StorageFull
            | std::io::ErrorKind::ReadOnlyFilesystem
            | std::io::ErrorKind::QuotaExceeded
            | std::io::ErrorKind::FileTooLarge) => Self::Read {
                url: url.to_owned(),
                fault: AccessFault::from_kind(kind),
            },
            _ => Self::Read {
                url: url.to_owned(),
                fault: AccessFault::Unknown { message },
            },
        }
    }

    /// Maps one cache write io failure at the url into domain language.
    pub fn from_write_io(url: &str, error: std::io::Error) -> Self {
        let message = error.to_string();
        match error.kind() {
            kind @ (std::io::ErrorKind::NotFound
            | std::io::ErrorKind::PermissionDenied
            | std::io::ErrorKind::StorageFull
            | std::io::ErrorKind::ReadOnlyFilesystem
            | std::io::ErrorKind::QuotaExceeded
            | std::io::ErrorKind::FileTooLarge) => Self::Write {
                url: url.to_owned(),
                fault: AccessFault::from_kind(kind),
            },
            _ => Self::WriteUnknown {
                url: url.to_owned(),
                message,
            },
        }
    }
}

/// Fetch result alias.
pub type Result<T> = std::result::Result<T, FetchError>;
