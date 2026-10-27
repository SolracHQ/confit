//! HTTP downloads.

use std::io;

use confit_core::error::ErrorContext;
#[cfg(not(any(test, feature = "test-support")))]
use confit_core::error::FetchError;

/// Download one URL body as a readable stream with the total length.
///
/// # Errors
///
/// - [`confit_core::error::FetchError::StatusRejected`] for failing statuses.
/// - [`confit_core::error::FetchError::TimedOut`] for timeouts.
/// - [`confit_core::error::FetchError::DownloadFailed`] for transport failures.
pub fn download(url: &str, limit: u64) -> Result<(Box<dyn io::Read>, Option<u64>), ErrorContext> {
    #[cfg(any(test, feature = "test-support"))]
    {
        registry::download(url, limit)
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        network(url, limit)
    }
}

/// Download one URL body over the network.
///
/// # Errors
///
/// - [`FetchError::StatusRejected`] for failing statuses.
/// - [`FetchError::TimedOut`] for timeouts.
/// - [`FetchError::DownloadFailed`] for transport failures.
#[cfg(not(any(test, feature = "test-support")))]
fn network(url: &str, limit: u64) -> Result<(Box<dyn io::Read>, Option<u64>), ErrorContext> {
    let response = match ureq::get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(code)) => {
            return Err(ErrorContext::from(FetchError::StatusRejected { code }));
        }
        Err(ureq::Error::Timeout(_)) => {
            return Err(ErrorContext::from(FetchError::TimedOut));
        }
        Err(other) => {
            return Err(ErrorContext::from(FetchError::DownloadFailed {
                reason: other.to_string(),
            }));
        }
    };
    let total = response.body().content_length();
    let reader = response
        .into_body()
        .into_with_config()
        .limit(limit)
        .reader();
    Ok((Box::new(reader), total))
}

#[cfg(any(test, feature = "test-support"))]
pub use registry::{calls, clear, fail, serve};

#[cfg(any(test, feature = "test-support"))]
mod registry {
    //! Scripted downloads for tests.

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io;

    use confit_core::error::{ErrorContext, FetchError};

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

    /// Script one URL body for later downloads.
    pub fn serve(url: &str, body: &[u8]) {
        SCRIPTS.with(|cell| {
            cell.borrow_mut()
                .bodies
                .insert(url.to_string(), body.to_vec());
        });
    }

    /// Script one URL failure for later downloads.
    pub fn fail(url: &str, message: &str) {
        SCRIPTS.with(|cell| {
            cell.borrow_mut()
                .failures
                .insert(url.to_string(), message.to_string());
        });
    }

    /// Return download counts for one URL.
    pub fn calls(url: &str) -> usize {
        SCRIPTS.with(|cell| cell.borrow().calls.get(url).copied().unwrap_or(0))
    }

    /// Clear every scripted body, failure, and count.
    pub fn clear() {
        SCRIPTS.with(|cell| {
            let mut scripts = cell.borrow_mut();
            scripts.bodies.clear();
            scripts.failures.clear();
            scripts.calls.clear();
        });
    }

    /// Serve one scripted download with the scripted body length.
    pub(super) fn download(
        url: &str,
        _limit: u64,
    ) -> Result<(Box<dyn io::Read>, Option<u64>), ErrorContext> {
        SCRIPTS.with(|cell| {
            let mut scripts = cell.borrow_mut();
            let count = scripts.calls.get(url).copied().unwrap_or(0);
            scripts.calls.insert(url.to_string(), count + 1);
            if let Some(message) = scripts.failures.get(url).cloned() {
                return Err(ErrorContext::from(FetchError::DownloadFailed {
                    reason: message,
                }));
            }
            match scripts.bodies.get(url).cloned() {
                Some(body) => {
                    let total = u64::try_from(body.len()).unwrap_or(u64::MAX);
                    Ok((
                        Box::new(std::io::Cursor::new(body)) as Box<dyn io::Read>,
                        Some(total),
                    ))
                }
                None => panic!("no scripted body for url '{url}'"),
            }
        })
    }
}
