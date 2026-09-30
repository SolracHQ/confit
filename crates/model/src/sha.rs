//! Sha
//!
//! Sealed content hashes.

use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::error::{Error, Result};

/// Byte count for a SHA-256 digest.
const SHA_BYTE_LEN: usize = 32;

/// Hex character count for a SHA-256 digest.
const SHA_HEX_LEN: usize = 64;

/// Read chunk size for streaming hashes.
const READ_CHUNK_BYTES: usize = 8 * 1024;

/// Sealed SHA-256 digest.
///
/// Raw 32-byte digest with hex at the edge.
///
/// Birth checks the shape once; readers trust the type without rechecking.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sha {
    inner: [u8; SHA_BYTE_LEN],
}

impl Sha {
    /// Seals one content hash.
    ///
    /// # Errors
    ///
    /// - [`Error::Parse`] for malformed hashes.
    ///
    pub fn new(sha: impl Into<String>) -> Result<Self> {
        let sha = sha.into();
        check_sha_hex(&sha)?;
        let bytes = sha.as_bytes();
        let mut inner = [0u8; SHA_BYTE_LEN];
        for (index, slot) in inner.iter_mut().enumerate() {
            let high = hex_value(bytes[index * 2], &sha)?;
            let low = hex_value(bytes[index * 2 + 1], &sha)?;
            *slot = (high << 4) | low;
        }
        Ok(Self { inner })
    }

    /// Hashes bytes into a sealed digest.
    ///
    pub fn hash(bytes: &[u8]) -> Self {
        let digest = sha2::Sha256::digest(bytes);
        Self {
            inner: digest.into(),
        }
    }

    /// Seals a streaming hash.
    ///
    pub fn finish(hasher: sha2::Sha256) -> Self {
        Self {
            inner: hasher.finalize().into(),
        }
    }

    /// Hashes a byte stream into a sealed digest.
    ///
    /// # Errors
    ///
    /// Stream read failures surface as io errors.
    ///
    pub fn read(stream: &mut impl std::io::Read) -> std::io::Result<Self> {
        let mut hasher = sha2::Sha256::new();
        let mut chunk = [0u8; READ_CHUNK_BYTES];
        loop {
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            hasher.update(&chunk[..read]);
        }
        Ok(Self::finish(hasher))
    }

    /// Reads the hex digest.
    ///
    pub fn hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(SHA_HEX_LEN);
        for byte in self.inner {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }

    /// Reads the hash and size label.
    pub fn label(&self, len: u64) -> String {
        format!("sha256:{self} ({len} bytes)")
    }
}

impl Serialize for Sha {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Sha {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for Sha {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.hex())
    }
}

/// Reads one hex character value.
fn hex_value(byte: u8, sha: &str) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(Error::Parse {
            input: sha.to_owned(),
            want: "64 hex characters".to_owned(),
        }),
    }
}

/// Rejects malformed content hashes.
fn check_sha_hex(sha: &str) -> Result<()> {
    if sha.len() == SHA_HEX_LEN && sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(());
    }
    Err(Error::Parse {
        input: sha.to_owned(),
        want: "64 hex characters".to_owned(),
    })
}
