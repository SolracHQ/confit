//! Object file encoding for pool entries.

use std::io::Read as _;
use std::io::Seek as _;
use std::io::Write as _;
use std::path::PathBuf;

use bytemuck::{Pod, Zeroable};
use confit_core::error::{
    ContentError, ContextualCause, ContextualResult, ErrorCause, WithContext, contextual,
};
use confit_core::sha::Sha;

/// Object identity prefix.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(super) struct ObjectHeader {
    /// Magic bytes opening every pool file.
    pub magic: [u8; 4],
    /// Format version, currently zero.
    pub version: u8,
    /// Raw content hash.
    pub sha: [u8; 32],
    /// Content byte count as little-endian bytes.
    pub size: [u8; 8],
}

/// Current object format version.
const OBJECT_VERSION: u8 = 0;

/// Pool gzip level.
const POOL_GZIP_LEVEL: u32 = 6;

/// Checked object file.
pub struct PoolFile {
    /// Object file location.
    path: PathBuf,
    /// Content byte count from the header.
    size: u64,
}

/// Object body writer.
pub(crate) struct BodyWriter {
    /// Gzip encoder over the object sink.
    encoder: flate2::write::GzEncoder<std::fs::File>,
}

impl ObjectHeader {
    /// Return the content hash.
    fn sha(&self) -> Sha {
        Sha::from_bytes(self.sha)
    }

    /// Return the content byte count.
    fn size(&self) -> u64 {
        u64::from_le_bytes(self.size)
    }
}

impl PoolFile {
    /// Open one object file with its header checked.
    ///
    /// # Errors
    ///
    /// - [`ContentError::CorruptObject`] for short and undecodable objects.
    /// - [`ErrorCause::ContentHashMismatch`] for header hash mismatches.
    /// - [`confit_core::error::FilesystemError`] for missing and unreadable pool files.
    pub fn open(path: PathBuf, expected: &Sha) -> ContextualResult<Self> {
        let mut file = std::fs::File::open(&path)
            .map_err(ContextualCause::from)
            .with_cache_path(&path)?;
        let mut raw = [0u8; std::mem::size_of::<ObjectHeader>()];
        if let Err(error) = file.read_exact(&mut raw) {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                return Err(contextual(ContentError::CorruptObject {
                    expected: expected.clone(),
                })
                .with_cache_path(path));
            }
            return Err(ContextualCause::from(error).with_cache_path(path));
        }
        match decode_header(&raw) {
            Some(header) if header.sha() == *expected => Ok(Self {
                path,
                size: header.size(),
            }),
            Some(header) => Err(contextual(ErrorCause::ContentHashMismatch {
                expected: expected.clone(),
                found: header.sha(),
            })
            .with_cache_path(path)),
            None => Err(contextual(ContentError::CorruptObject {
                expected: expected.clone(),
            })
            .with_cache_path(path)),
        }
    }

    /// Start one object body with its header written.
    ///
    /// # Errors
    ///
    /// - Sink write failures report as io errors.
    pub(crate) fn create(
        mut sink: std::fs::File,
        sha: &Sha,
        size: u64,
    ) -> std::io::Result<BodyWriter> {
        sink.write_all(bytemuck::bytes_of(&encode_header(sha, size)))?;
        Ok(BodyWriter {
            encoder: flate2::write::GzEncoder::new(sink, flate2::Compression::new(POOL_GZIP_LEVEL)),
        })
    }

    /// Return the content byte count.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Load the body stream.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] for missing and unreadable pool files.
    pub fn body(&self) -> ContextualResult<flate2::read::GzDecoder<std::fs::File>> {
        let mut file = std::fs::File::open(&self.path)
            .map_err(ContextualCause::from)
            .with_cache_path(&self.path)?;
        file.seek(std::io::SeekFrom::Start(
            std::mem::size_of::<ObjectHeader>() as u64,
        ))
        .map_err(ContextualCause::from)
        .with_cache_path(&self.path)?;
        Ok(flate2::read::GzDecoder::new(file))
    }
}

impl BodyWriter {
    /// Finish the body and close the sink.
    ///
    /// # Errors
    ///
    /// - Sink write failures report as io errors.
    pub(crate) fn finish(self) -> std::io::Result<()> {
        self.encoder.finish()?;
        Ok(())
    }
}

impl std::io::Write for BodyWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.encoder.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.encoder.flush()
    }
}

/// Render one object header for its content hash and byte count.
fn encode_header(sha: &Sha, size: u64) -> ObjectHeader {
    ObjectHeader {
        magic: *b"blob",
        version: OBJECT_VERSION,
        sha: sha.as_bytes(),
        size: size.to_le_bytes(),
    }
}

/// Check one header image into its content hash and byte count.
fn decode_header(raw: &[u8]) -> Option<ObjectHeader> {
    let header: &ObjectHeader = bytemuck::try_from_bytes(raw).ok()?;
    if header.magic != *b"blob" || header.version != OBJECT_VERSION {
        return None;
    }
    Some(*header)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip_keeps_hash_and_size() {
        let sha = Sha::hash(b"header bytes");
        let header = encode_header(&sha, 12);
        assert_eq!(std::mem::size_of_val(&header), 45, "header spans 45 bytes");
        let raw = bytemuck::bytes_of(&header).to_vec();
        match decode_header(&raw) {
            Some(found) => {
                assert_eq!(found.sha(), sha, "header keeps the hash");
                assert_eq!(found.size(), 12, "header keeps the size");
            }
            None => panic!("header decodes"),
        }
        let mut bad_magic = header;
        bad_magic.magic[0] ^= 0xff;
        assert!(
            decode_header(bytemuck::bytes_of(&bad_magic)).is_none(),
            "bad magic rejected"
        );
    }
}
