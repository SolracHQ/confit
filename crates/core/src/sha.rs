//! Verified content hashes.

use lua_binding::Shape;
use lua_binding::error::{DecodeError, DecodeTag};
use lua_binding::value::Hooked;
use mlua::Lua;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

/// One verified hash failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShaError {
    /// A text that is not 64 hex characters.
    Parse {
        /// The offending input value.
        input: String,
        /// The required shape.
        require: String,
    },
}

impl std::fmt::Display for ShaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse { input, require } => {
                write!(formatter, "invalid '{input}', expected {require}")
            }
        }
    }
}

impl std::error::Error for ShaError {}

/// One verified hash result.
pub type ShaResult<T> = std::result::Result<T, ShaError>;

/// The SHA-256 digest size in bytes.
pub const SHA_BYTE_LEN: usize = 32;

/// The SHA-256 digest size in hex characters.
const SHA_HEX_LEN: usize = 64;

/// The chunk size for a streaming read.
const READ_CHUNK_BYTES: usize = 8 * 1024;

/// A SHA-256 digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Shape)]
#[shape(proxy = String)]
pub struct Sha {
    inner: [u8; SHA_BYTE_LEN],
}

impl Sha {
    /// Parses one content hash from hex text.
    ///
    /// # Errors
    ///
    /// - [`ShaError::Parse`] when the text holds no 64 hex characters.
    pub fn new(sha: impl Into<String>) -> ShaResult<Self> {
        let sha = sha.into();
        validate_sha_hex(&sha)?;
        let bytes = sha.as_bytes();
        let mut inner = [0u8; SHA_BYTE_LEN];
        for (index, byte) in inner.iter_mut().enumerate() {
            let high = hex_value(bytes[index * 2], &sha)?;
            let low = hex_value(bytes[index * 2 + 1], &sha)?;
            *byte = (high << 4) | low;
        }
        Ok(Self { inner })
    }

    /// Computes the digest of the given bytes.
    pub fn hash(bytes: &[u8]) -> Self {
        let digest = sha2::Sha256::digest(bytes);
        Self {
            inner: digest.into(),
        }
    }

    /// Reads a byte stream and returns its digest and byte count.
    ///
    /// # Errors
    ///
    /// - [`std::io::Error`] when the stream read fails.
    pub fn read_with_size(stream: &mut impl std::io::Read) -> std::io::Result<(Self, u64)> {
        let mut hasher = sha2::Sha256::new();
        let mut chunk = [0u8; READ_CHUNK_BYTES];
        let mut len: u64 = 0;
        loop {
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            len += read as u64;
            hasher.update(&chunk[..read]);
        }
        Ok((Self::from(hasher), len))
    }

    /// Reads a byte stream and returns its digest.
    ///
    /// # Errors
    ///
    /// - [`std::io::Error`] when the stream read fails.
    pub fn read(stream: &mut impl std::io::Read) -> std::io::Result<Self> {
        Ok(Self::read_with_size(stream)?.0)
    }

    /// Returns the digest in raw byte form.
    pub fn as_bytes(&self) -> [u8; SHA_BYTE_LEN] {
        self.inner
    }

    /// Turns raw bytes into a digest.
    pub fn from_bytes(bytes: [u8; SHA_BYTE_LEN]) -> Self {
        Self { inner: bytes }
    }

    /// Writes the digest as lowercase hex.
    pub fn hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(SHA_HEX_LEN);
        for byte in self.inner {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }

    /// Writes the hash and size as a label.
    pub fn label(&self, len: u64) -> String {
        format!("sha256:{self} ({len} bytes)")
    }

    /// Formats the short hash prefix for a terminal line.
    pub fn short(&self) -> String {
        self.hex()[..12].to_owned()
    }
}

impl From<sha2::Sha256> for Sha {
    fn from(hasher: sha2::Sha256) -> Self {
        Self {
            inner: hasher.finalize().into(),
        }
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

impl Hooked<String> for Sha {
    fn decode(_lua: &Lua, text: String) -> Result<Self, DecodeError> {
        Sha::new(text.clone()).map_err(|_| DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "a 64-char hex digest".to_owned(),
                found: format!("'{text}'"),
            },
        })
    }

    fn encode(&self, _lua: &Lua) -> mlua::Result<String> {
        Ok(self.hex())
    }
}

/// A writer that hashes every byte it writes.
pub struct ShaWriter<W> {
    inner: W,
    hasher: sha2::Sha256,
}

impl<W: std::io::Write> ShaWriter<W> {
    /// Turns one writer into a hashing writer.
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: sha2::Sha256::new(),
        }
    }

    /// Closes the running hash after the last byte.
    pub fn digest(self) -> Sha {
        Sha::from(self.hasher)
    }

    /// Closes the running hash and hands back the writer with its digest.
    pub fn finish(self) -> (W, Sha) {
        let digest = Sha::from(self.hasher);
        (self.inner, digest)
    }
}

impl<W: std::io::Write> std::io::Write for ShaWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let wrote = self.inner.write(buf)?;
        self.hasher.update(&buf[..wrote]);
        Ok(wrote)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Turns one hex character into its value.
fn hex_value(byte: u8, sha: &str) -> ShaResult<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(ShaError::Parse {
            input: sha.to_owned(),
            require: "64 hex characters".to_owned(),
        }),
    }
}

/// Refuses a content hash that is not 64 hex characters.
fn validate_sha_hex(sha: &str) -> ShaResult<()> {
    if sha.len() == SHA_HEX_LEN && sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(());
    }
    Err(ShaError::Parse {
        input: sha.to_owned(),
        require: "64 hex characters".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha_hook_decodes_hex_text() {
        let lua = Lua::new();
        let text = "0".repeat(SHA_HEX_LEN);
        let found = <Sha as Hooked<String>>::decode(&lua, text.clone()).unwrap();
        assert_eq!(
            found,
            Sha::new(text).unwrap(),
            "the hook keeps the digest bytes"
        );
    }

    #[test]
    fn sha_hook_encodes_hex_text() {
        let lua = Lua::new();
        let sha = Sha::hash(b"1.2.3");
        let text = <Sha as Hooked<String>>::encode(&sha, &lua).unwrap();
        assert_eq!(text, sha.hex(), "the hook returns the hex text");
    }

    #[test]
    fn sha_hook_rejects_short_text() {
        let lua = Lua::new();
        let error = <Sha as Hooked<String>>::decode(&lua, "0".repeat(SHA_HEX_LEN - 1)).unwrap_err();
        assert!(
            error.to_string().contains("a 64-char hex digest"),
            "the hook error shows the expected shape: {error}"
        );
    }
}
