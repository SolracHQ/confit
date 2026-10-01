//! Transport
//!
//! Boundary mapping between driver downloads and fetch errors.
//!
//! The driver answers bodies over streams, transport
//! adds the url with fetch language. The body cap rides
//! along as fetch policy passed per call.

use std::io::Read;

use confit_driver::http::HttpError;

use super::error::{FetchError, Result};

/// Body cap passed per call into the driver download.
pub(crate) const BODY_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

/// Downloads one URL body as a readable stream.
///
/// # Errors
///
/// - [`FetchError::Status`] for refused statuses.
/// - [`FetchError::Timeout`] for timeouts.
/// - [`FetchError::Read`] for other transport read failures.
/// - [`FetchError::Unscripted`] for unscripted test urls.
pub fn download(url: &str, limit: u64) -> Result<Box<dyn Read>> {
    confit_driver::http::download(url, limit).map_err(|error| from_http(url, error))
}

/// Maps one driver failure into its fetch error.
///
/// Status with timeout and unscripted keep their
/// shape holding the url, io folds through the io
/// mapping with the url.
fn from_http(url: &str, error: HttpError) -> FetchError {
    match error {
        HttpError::Status { code } => FetchError::Status {
            url: url.to_owned(),
            code,
        },
        HttpError::Timeout => FetchError::Timeout {
            url: url.to_owned(),
        },
        HttpError::Io(cause) => FetchError::from_io(url, cause),
        HttpError::Unscripted => FetchError::Unscripted {
            url: url.to_owned(),
        },
    }
}
