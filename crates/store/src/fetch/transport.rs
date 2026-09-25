//! Transport
//!
//! Download source serving fetch bodies over streams.
//!
//! Production downloads over HTTP. Tests script bodies
//! into a thread-local registry through test-only
//! functions; unscripted URLs fail naming the URL, so
//! no test ever touches the network by accident.

use std::io::Read;

use confit_model::error::Result;

/// Body cap shared by download and cache writes.
pub(crate) const BODY_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

/// Downloads one URL body as a readable stream.
///
/// Production reaches the network through `ureq`. Test
/// builds serve the thread-local script registry
/// instead, so the binary shape never changes.
///
/// # Errors
///
/// Transport failures fail as plan errors naming the URL.
pub fn download(url: &str) -> Result<Box<dyn Read>> {
    #[cfg(test)]
    {
        registry::download(url)
    }
    #[cfg(not(test))]
    {
        use confit_model::error::Error;

        let response = ureq::get(url)
            .call()
            .map_err(|error| Error::Plan(format!("cannot fetch '{url}': {error}")))?;
        let reader = response
            .into_body()
            .into_with_config()
            .limit(BODY_LIMIT_BYTES)
            .reader();
        Ok(Box::new(reader))
    }
}

#[cfg(test)]
pub use registry::{calls, clear, fail, serve};

#[cfg(test)]
pub(crate) mod registry {
    //! Registry
    //!
    //! Thread-local scripted downloads for tests.

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io::Read;

    use confit_model::error::{Error, Result};

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
    pub(super) fn download(url: &str) -> Result<Box<dyn Read>> {
        SCRIPTS.with(|cell| {
            let mut scripts = cell.borrow_mut();
            let count = scripts.calls.get(url).copied().unwrap_or(0);
            scripts.calls.insert(url.to_string(), count + 1);
            if let Some(message) = scripts.failures.get(url).cloned() {
                return Err(Error::Plan(format!("cannot fetch '{url}': {message}")));
            }
            match scripts.bodies.get(url).cloned() {
                Some(body) => Ok(Box::new(std::io::Cursor::new(body)) as Box<dyn Read>),
                None => Err(Error::Plan(format!(
                    "cannot fetch '{url}': no scripted body"
                ))),
            }
        })
    }
}
