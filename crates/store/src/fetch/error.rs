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
    /// Failed cache write holding the url with the fault.
    #[error("cannot fetch '{url}': {fault}")]
    Write {
        /// Holds the url under caching.
        url: String,
        /// Holds the write fault cause.
        fault: AccessFault,
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
            std::io::ErrorKind::TimedOut => Self::Timeout {
                url: url.to_owned(),
            },
            _ => Self::Unknown {
                url: url.to_owned(),
                message,
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
            _ => Self::Unknown {
                url: url.to_owned(),
                message,
            },
        }
    }
}

/// Fetch result alias.
pub type Result<T> = std::result::Result<T, FetchError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn write_io(url: &str, kind: std::io::ErrorKind) -> FetchError {
        FetchError::from_write_io(url, std::io::Error::new(kind, "disk failed"))
    }

    #[test]
    fn write_carries_url_with_fault() {
        let url = "https://example.com/tool.bin";
        let error = write_io(url, std::io::ErrorKind::FileTooLarge);
        match &error {
            FetchError::Write { url: found, fault } => {
                assert_eq!(found, url, "write keeps the url");
                assert!(
                    matches!(*fault, AccessFault::FileTooLarge),
                    "write keeps the fault: {error}"
                );
            }
            other => panic!("wrong write variant: {other}"),
        }
    }
}
