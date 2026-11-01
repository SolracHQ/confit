//! Reads and writes zip and gzipped tar containers.

use std::io::{self, Read as _};

use confit_core::error::{ArchiveError, ContextualCause, ContextualResult};

mod targz;
mod zip_ext;

/// The magic bytes at the start of a zip local file header.
const ZIP_LOCAL_MAGIC: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];

/// The magic bytes of an empty zip archive.
const ZIP_EMPTY_MAGIC: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];

/// The magic bytes that open a gzip stream.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// The byte offset of the tar format tag.
const TAR_MAGIC_OFFSET: usize = 257;

/// The tag that marks the tar format.
const TAR_MAGIC: &[u8] = b"ustar";

/// The number of head bytes a probe checks.
const HEAD_LEN: usize = 512;

/// The permission bits used when an entry holds no mode.
const FALLBACK_MODE: u32 = 0o644;

/// The mask that keeps the exposed permission bits.
const MODE_MASK: u32 = 0o777;

/// The container format of an archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Algorithm {
    /// A zip container that deflates each member.
    Zip,
    /// A gzipped tar container that compresses the whole stream.
    #[default]
    TarGz,
}

/// The compression strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Level {
    /// Raw storage with no compression.
    None,
    /// Fast compression.
    Low,
    /// Balanced compression.
    Mid,
    /// Dense compression.
    #[default]
    High,
    /// The densest compression.
    Max,
}

/// One archive member.
pub struct Entry<'a> {
    /// The name of the member with forward slashes.
    pub name: String,
    /// The permission bits of the member.
    pub mode: u32,
    /// The stream that holds the member body.
    pub reader: Box<dyn io::Read + 'a>,
}

/// Converts a zip failure into a core cause.
fn zip_error(error: zip::result::ZipError) -> ContextualCause {
    use zip::result::ZipError as Zip;
    let message = error.to_string();
    match error {
        Zip::Io(cause) => ContextualCause::from(cause),
        Zip::InvalidPassword | Zip::UnsupportedArchive(Zip::PASSWORD_REQUIRED) => {
            ContextualCause::from(ArchiveError::PasswordLocked)
        }
        Zip::UnsupportedArchive(_) | Zip::CompressionMethodNotSupported(_) => {
            ContextualCause::from(ArchiveError::AlgorithmUnknown).with_detail(message)
        }
        _ => ContextualCause::from(ArchiveError::BackendCorrupt).with_detail(message),
    }
}

/// A writer that streams an archive into a seekable sink.
pub struct Builder<W: io::Write + io::Seek> {
    inner: Inner<W>,
}

/// The writer storage for the zip and gzipped tar formats.
enum Inner<W: io::Write + io::Seek> {
    /// The zip writer and the level it applies.
    Zip {
        /// The zip writer bound to the sink.
        writer: Box<zip::ZipWriter<W>>,
        /// The compression strength the writer applies.
        level: Level,
    },
    /// The builder that writes a gzipped tar.
    Tar {
        /// The tar builder that writes through the gzip encoder.
        builder: Box<tar::Builder<flate2::write::GzEncoder<W>>>,
    },
}

impl Level {
    /// Returns the level as a numeric strength.
    pub fn value(&self) -> u32 {
        match self {
            Self::None => 0,
            Self::Low => 1,
            Self::Mid => 4,
            Self::High => 6,
            Self::Max => 9,
        }
    }
}

impl<W: io::Write + io::Seek> Builder<W> {
    /// Creates an archive writer over the sink.
    pub fn create(sink: W, algorithm: Algorithm, level: Level) -> Self {
        match algorithm {
            Algorithm::Zip => Self {
                inner: Inner::Zip {
                    writer: Box::new(zip::ZipWriter::new(sink)),
                    level,
                },
            },
            Algorithm::TarGz => Self {
                inner: Inner::Tar {
                    builder: Box::new(tar::Builder::new(flate2::write::GzEncoder::new(
                        sink,
                        flate2::Compression::new(level.value()),
                    ))),
                },
            },
        }
    }

    /// Writes a member and its body into the archive.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::BackendCorrupt`] when the backend rejects the framing.
    /// - [`confit_core::error::FilesystemError`] when access is denied or the write fails.
    pub fn member<R: io::Read>(
        &mut self,
        name: &str,
        mode: u32,
        len: u64,
        reader: &mut R,
    ) -> ContextualResult<()> {
        match &mut self.inner {
            Inner::Zip { writer, level } => {
                let _ = len;
                let method = match level {
                    Level::None => zip::CompressionMethod::Stored,
                    Level::Low | Level::Mid | Level::High | Level::Max => {
                        zip::CompressionMethod::Deflated
                    }
                };
                let compression = match level {
                    Level::None => None,
                    Level::Low => Some(1),
                    Level::Mid => Some(4),
                    Level::High => Some(6),
                    Level::Max => Some(9),
                };
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(method)
                    .compression_level(compression)
                    .unix_permissions(mode);
                writer.start_file(name, options).map_err(zip_error)?;
                std::io::copy(reader, writer)?;
                Ok(())
            }
            Inner::Tar { builder } => {
                let mut header = tar::Header::new_gnu();
                header.set_size(len);
                header.set_mode(mode);
                header.set_cksum();
                builder.append_data(&mut header, name, reader)?;
                Ok(())
            }
        }
    }

    /// Writes the archive tail and returns the sink.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::BackendCorrupt`] when the backend rejects the tail.
    /// - [`confit_core::error::FilesystemError`] when access is denied or the tail write fails.
    pub fn finish(self) -> ContextualResult<W> {
        match self.inner {
            Inner::Zip { writer, .. } => Ok((*writer).finish().map_err(zip_error)?),
            Inner::Tar { builder } => {
                let encoder = (*builder).into_inner()?;
                Ok(encoder.finish()?)
            }
        }
    }
}

/// Returns whether the head bytes hold a supported archive.
pub fn supported(head: &[u8]) -> bool {
    is_zip(head) || is_gzip(head) || is_tar_tag(head)
}

/// Calls the visitor for each archive member.
///
/// # Errors
///
/// - [`ArchiveError::NotArchive`] when the bytes hold no archive.
/// - [`ArchiveError::BackendCorrupt`] when the archive is corrupt or a stream ends early.
/// - [`ArchiveError::PasswordLocked`] when the archive is locked.
/// - [`ArchiveError::AlgorithmUnknown`] when the archive uses an unknown feature.
/// - [`confit_core::error::FilesystemError`] when access is denied or the input is missing.
pub fn uncompress<R: io::Read + io::Seek>(
    mut reader: R,
    visit: impl FnMut(Entry<'_>) -> ContextualResult<()>,
) -> ContextualResult<()> {
    let head = peek_head(&mut reader)?;
    if is_zip(&head) {
        return zip_ext::walk(reader, visit);
    }
    if is_gzip(&head) || is_tar_tag(&head) {
        return targz::walk(reader, visit);
    }
    Err(ContextualCause::from(ArchiveError::NotArchive))
}

/// Collects the member names in archive order.
///
/// # Errors
///
/// - [`ArchiveError::NotArchive`] when the bytes hold no archive.
/// - [`ArchiveError::BackendCorrupt`] when the archive is corrupt or a stream ends early.
/// - [`ArchiveError::PasswordLocked`] when the archive is locked.
/// - [`ArchiveError::AlgorithmUnknown`] when the archive uses an unknown feature.
/// - [`confit_core::error::FilesystemError`] when access is denied or the input is missing.
pub fn names<R: io::Read + io::Seek>(mut reader: R) -> ContextualResult<Vec<String>> {
    let head = peek_head(&mut reader)?;
    if is_zip(&head) {
        return zip_ext::names(reader);
    }
    if is_gzip(&head) || is_tar_tag(&head) {
        return targz::names(reader);
    }
    Err(ContextualCause::from(ArchiveError::NotArchive))
}

/// Reads the head bytes and restores the cursor position.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] when access is denied or the input is missing.
fn peek_head<R: io::Read + io::Seek>(reader: &mut R) -> ContextualResult<Vec<u8>> {
    let pos = reader.stream_position()?;
    let mut head = Vec::with_capacity(HEAD_LEN);
    reader
        .by_ref()
        .take(HEAD_LEN as u64)
        .read_to_end(&mut head)?;
    reader.seek(io::SeekFrom::Start(pos))?;
    Ok(head)
}

/// Returns whether the head bytes hold a zip container.
fn is_zip(head: &[u8]) -> bool {
    head.starts_with(&ZIP_LOCAL_MAGIC) || head.starts_with(&ZIP_EMPTY_MAGIC)
}

/// Returns whether the head bytes hold a gzip stream.
fn is_gzip(head: &[u8]) -> bool {
    head.starts_with(&GZIP_MAGIC)
}

/// Returns whether the head bytes hold a tar tag.
fn is_tar_tag(head: &[u8]) -> bool {
    head.get(TAR_MAGIC_OFFSET..TAR_MAGIC_OFFSET + TAR_MAGIC.len())
        .is_some_and(|tag| tag == TAR_MAGIC)
}
