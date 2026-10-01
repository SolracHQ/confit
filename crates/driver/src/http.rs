//! HTTP downloads over caller streams with driver errors.
//!
//! Production downloads ride `ureq` capped at the
//! caller limit. Tests script bodies into a
//! thread-local registry, so no test ever touches
//! the network by accident.

use std::fmt;
use std::io;

/// HTTP failure shapes holding no url.
///
/// Stores add the url at the boundary, so one
/// failure shape serves every caller.
#[derive(Debug)]
pub enum HttpError {
    /// Refused status holding the status code.
    Status {
        /// Holds the refusing status code.
        code: u16,
    },
    /// Timed-out request holding nothing.
    Timeout,
    /// Transport failure holding the cause.
    Io(io::Error),
    /// Unscripted test url holding nothing.
    Unscripted,
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status { code } => write!(formatter, "status {code}"),
            Self::Timeout => write!(formatter, "timeout"),
            Self::Io(cause) => write!(formatter, "{cause}"),
            Self::Unscripted => write!(formatter, "no scripted body"),
        }
    }
}

impl std::error::Error for HttpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(cause) => Some(cause),
            _ => None,
        }
    }
}

impl From<io::Error> for HttpError {
    fn from(cause: io::Error) -> Self {
        Self::Io(cause)
    }
}

/// Downloads one URL body as a readable stream.
///
/// The limit caps the network reader, so oversized
/// bodies stop early. Test builds serve the
/// thread-local script registry instead.
///
/// # Errors
///
/// - [Status] for refused statuses.
/// - [Timeout] for timeouts.
/// - [Io] for transport read failures.
/// - [Unscripted] for unscripted test urls.
pub fn download(url: &str, limit: u64) -> Result<Box<dyn io::Read>, HttpError> {
    #[cfg(any(test, feature = "test-support"))]
    {
        registry::download(url, limit)
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        network(url, limit)
    }
}

/// Downloads one URL body over the network.
///
/// # Errors
///
/// - [Status] for refused statuses.
/// - [Timeout] for timeouts.
/// - [Io] for transport read failures.
#[cfg(not(any(test, feature = "test-support")))]
fn network(url: &str, limit: u64) -> Result<Box<dyn io::Read>, HttpError> {
    let response = ureq::get(url).call().map_err(from_ureq)?;
    let reader = response
        .into_body()
        .into_with_config()
        .limit(limit)
        .reader();
    Ok(Box::new(reader))
}

/// Maps one ureq failure into its http error.
///
/// Status with timeout and io keep their shape,
/// the long tail rides io with its message, so the
/// store io mapping echoes it verbatim.
#[cfg(not(any(test, feature = "test-support")))]
fn from_ureq(error: ureq::Error) -> HttpError {
    match error {
        ureq::Error::StatusCode(code) => HttpError::Status { code },
        ureq::Error::Timeout(_) => HttpError::Timeout,
        ureq::Error::Io(cause) => HttpError::Io(cause),
        other => HttpError::Io(io::Error::other(other.to_string())),
    }
}

#[cfg(any(test, feature = "test-support"))]
pub use registry::{calls, clear, fail, serve};

#[cfg(any(test, feature = "test-support"))]
mod registry {
    //! Scripted downloads for tests.

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io;

    use super::HttpError;

    struct Scripts {
        bodies: HashMap<String, Vec<u8>>,
        failures: HashMap<String, String>,
        calls: HashMap<String, usize>,
    }

    thread_local! {
        static SCRIPTS: RefCell<Scripts> = RefCell::new(Scripts {
            bodies: HashMap::new(),
            failures: HashMap::new(),
            calls: HashMap::new(),
        });
    }

    /// Scripts one URL body for later downloads.
    pub fn serve(url: &str, body: &[u8]) {
        SCRIPTS.with(|cell| {
            cell.borrow_mut()
                .bodies
                .insert(url.to_string(), body.to_vec());
        });
    }

    /// Scripts one URL failure for later downloads.
    pub fn fail(url: &str, message: &str) {
        SCRIPTS.with(|cell| {
            cell.borrow_mut()
                .failures
                .insert(url.to_string(), message.to_string());
        });
    }

    /// Reads download counts for one URL.
    pub fn calls(url: &str) -> usize {
        SCRIPTS.with(|cell| cell.borrow().calls.get(url).copied().unwrap_or(0))
    }

    /// Clears every scripted body, failure, and count.
    pub fn clear() {
        SCRIPTS.with(|cell| {
            let mut scripts = cell.borrow_mut();
            scripts.bodies.clear();
            scripts.failures.clear();
            scripts.calls.clear();
        });
    }

    /// Serves one scripted download recording the call.
    ///
    /// Scripted failures ride io with the scripted
    /// message. Bodies serve verbatim. Unscripted urls
    /// fail with no body.
    pub(super) fn download(url: &str, _limit: u64) -> Result<Box<dyn io::Read>, HttpError> {
        SCRIPTS.with(|cell| {
            let mut scripts = cell.borrow_mut();
            let count = scripts.calls.get(url).copied().unwrap_or(0);
            scripts.calls.insert(url.to_string(), count + 1);
            if let Some(message) = scripts.failures.get(url).cloned() {
                return Err(HttpError::Io(io::Error::other(message)));
            }
            match scripts.bodies.get(url).cloned() {
                Some(body) => Ok(Box::new(std::io::Cursor::new(body)) as Box<dyn io::Read>),
                None => Err(HttpError::Unscripted),
            }
        })
    }
}
