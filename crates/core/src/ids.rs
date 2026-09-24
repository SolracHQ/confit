//! Ids
//!
//! Newtypes guarding core boundaries.

use sha2::Digest;

use crate::error::Result;

/// Disk state behind one document path.
///
/// The caller reads the path and reports the outcome.
/// Core stays free of filesystem access.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// Empty path. The document awaits creation.
    Absent,
    /// Disk bytes and permission bits for the path.
    Present {
        /// Holds raw disk bytes for the path.
        bytes: Vec<u8>,
        /// Holds unix permission bits. None while the backend
        /// holds no mode, like symlinks or umask default files.
        mode: Option<u32>,
    },
    /// Failing read. Carries the raw failure detail.
    Unreadable {
        /// Holds the raw failure detail from the read.
        reason: String,
    },
}

/// Computes lowercase hex SHA-256 over bytes.
///
/// Content hashes identify blobs the way paths identify
/// destinations.
///
/// # Arguments
///
/// * `bytes` - the input bytes.
///
/// # Returns
///
/// Lowercase hex digest.
///
/// # Examples
///
/// ```rust
/// use confit_core::ids::sha256_hex;
///
/// let digest = sha256_hex(b"abc");
/// assert!(matches!(digest.starts_with("ba7816"), true));
/// ```
pub fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Computes lowercase hex SHA-256 over a byte stream.
///
/// Reads in chunks, so large inputs hash without loading.
///
/// # Errors
///
/// Stream read failures surface as io errors.
pub fn sha256_read(stream: &mut impl std::io::Read) -> Result<String> {
    const CHUNK: usize = 8192;

    let mut hasher = sha2::Sha256::new();
    let mut chunk = [0u8; CHUNK];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_hash_matches_bytes_hash() {
        let bytes = b"abc".repeat(3000);
        let mut stream = std::io::Cursor::new(&bytes);
        let digest = sha256_read(&mut stream).unwrap();
        assert_eq!(digest, sha256_hex(&bytes));
    }
}
