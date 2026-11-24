//! Downloads URL bodies over HTTP.

use std::io;

use confit_core::error::ContextualCause;
#[cfg(not(any(test, feature = "test-support")))]
use confit_core::error::FetchError;

/// Opens a URL body as a stream with its total length.
///
/// - Each failure records the request URL in its context.
///
/// # Errors
///
/// - [`confit_core::error::FetchError::StatusRejected`] when the server rejects the status.
/// - [`confit_core::error::FetchError::TimedOut`] when the request times out.
/// - [`confit_core::error::FetchError::DownloadFailed`] when the transport fails.
pub fn download(
    url: &str,
    limit: u64,
) -> Result<(Box<dyn io::Read>, Option<u64>), ContextualCause> {
    #[cfg(any(test, feature = "test-support"))]
    {
        registry::download(url, limit)
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        network(url, limit)
    }
}

/// Fetches a URL body over the network.
///
/// # Errors
///
/// - [`FetchError::StatusRejected`] when the server rejects the status.
/// - [`FetchError::TimedOut`] when the request times out.
/// - [`FetchError::DownloadFailed`] when the transport fails.
#[cfg(not(any(test, feature = "test-support")))]
fn network(url: &str, limit: u64) -> Result<(Box<dyn io::Read>, Option<u64>), ContextualCause> {
    let response = match ureq::get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(code)) => {
            return Err(ContextualCause::from(FetchError::StatusRejected { code }).with_url(url));
        }
        Err(ureq::Error::Timeout(_)) => {
            return Err(ContextualCause::from(FetchError::TimedOut).with_url(url));
        }
        Err(error) => {
            let message = error.to_string();
            return Err(ContextualCause::from(FetchError::DownloadFailed)
                .with_url(url)
                .with_detail(message));
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
    //! Serves scripted downloads for tests.

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io;

    use confit_core::error::{ContextualCause, FetchError};

    struct Scripts {
        bodies: HashMap<String, Vec<u8>>,
        failures: HashMap<String, FetchError>,
        calls: HashMap<String, usize>,
    }

    thread_local! {
        static SCRIPTS: RefCell<Scripts> = RefCell::new(Scripts {
            bodies: HashMap::new(),
            failures: HashMap::new(),
            calls: HashMap::new(),
        });
    }

    /// Registers a URL body for a later download.
    pub fn serve(url: &str, body: &[u8]) {
        SCRIPTS.with(|cell| {
            cell.borrow_mut()
                .bodies
                .insert(url.to_string(), body.to_vec());
        });
    }

    /// Registers a URL failure for a later download.
    pub fn fail(url: &str, cause: FetchError) {
        SCRIPTS.with(|cell| {
            cell.borrow_mut().failures.insert(url.to_string(), cause);
        });
    }

    /// Returns how many times a URL was downloaded.
    pub fn calls(url: &str) -> usize {
        SCRIPTS.with(|cell| cell.borrow().calls.get(url).copied().unwrap_or(0))
    }

    /// Removes every scripted body, failure, and count.
    pub fn clear() {
        SCRIPTS.with(|cell| {
            let mut scripts = cell.borrow_mut();
            scripts.bodies.clear();
            scripts.failures.clear();
            scripts.calls.clear();
        });
    }

    /// Returns a scripted body as a stream with its length.
    pub(super) fn download(
        url: &str,
        _limit: u64,
    ) -> Result<(Box<dyn io::Read>, Option<u64>), ContextualCause> {
        SCRIPTS.with(|cell| {
            let mut scripts = cell.borrow_mut();
            let count = scripts.calls.get(url).copied().unwrap_or(0);
            scripts.calls.insert(url.to_string(), count + 1);
            if let Some(cause) = scripts.failures.get(url).cloned() {
                return Err(ContextualCause::from(cause).with_url(url));
            }
            match scripts.bodies.get(url).cloned() {
                Some(body) => {
                    let total = u64::try_from(body.len()).unwrap_or(u64::MAX);
                    Ok((
                        Box::new(std::io::Cursor::new(body)) as Box<dyn io::Read>,
                        Some(total),
                    ))
                }
                None => panic!("the scripts hold no body for url '{url}'"),
            }
        })
    }
}
