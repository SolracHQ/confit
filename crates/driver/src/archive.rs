//! Archive containers.

use std::io::{self, Read as _};

use confit_core::error::{ArchiveError, ContextualCause, ContextualResult};

mod targz;
mod zip_ext;

/// Zip local file magic.
const ZIP_LOCAL_MAGIC: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];

/// Zip empty archive magic.
const ZIP_EMPTY_MAGIC: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];

/// Gzip magic for deflated streams.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Offset of the tar format tag.
const TAR_MAGIC_OFFSET: usize = 257;

/// Tar magic tag for the format.
const TAR_MAGIC: &[u8] = b"ustar";

/// Head window for probes.
const HEAD_LEN: usize = 512;

/// Fallback mode for missing permission bits.
const FALLBACK_MODE: u32 = 0o644;

/// Permission mask for exposed modes.
const MODE_MASK: u32 = 0o777;

/// Archive container shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Algorithm {
    /// Zip container with per-member deflation.
    Zip,
    /// Gzipped tar container with whole-stream compression.
    #[default]
    TarGz,
}

/// Compression strength for archive builders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Level {
    /// Raw storage.
    None,
    /// Fast output.
    Low,
    /// Balanced output.
    Mid,
    /// Dense output.
    #[default]
    High,
    /// Densest output.
    Max,
}

/// One archive member.
pub struct Entry<'a> {
    /// Container name with forward slashes.
    pub name: String,
    /// Unix permission bits.
    pub mode: u32,
    /// Reader for the member body bytes.
    pub reader: Box<dyn io::Read + 'a>,
}

/// Map one zip backend failure to its bare core cause.
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

/// Streaming archive writer over one seekable sink.
pub struct Builder<W: io::Write + io::Seek> {
    inner: Inner<W>,
}

/// Builder storage for zip writers and gzipped tar builders.
enum Inner<W: io::Write + io::Seek> {
    /// Zip writer with its level.
    Zip {
        /// Zip writer over the sink.
        writer: Box<zip::ZipWriter<W>>,
        /// Compression strength.
        level: Level,
    },
    /// Gzipped tar builder.
    Tar {
        /// Tar builder over the gzip encoder.
        builder: Box<tar::Builder<flate2::write::GzEncoder<W>>>,
    },
}

impl Level {
    /// Return the numeric strength for one level.
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
    /// Build one writer over one sink with one algorithm and one level.
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

    /// Append one member with its body bytes.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::BackendCorrupt`] for rejected framing.
    /// - [`confit_core::error::FilesystemError`] for denied access and failed sinks.
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

    /// Flush archive tails and return the sink.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::BackendCorrupt`] for broken tails.
    /// - [`confit_core::error::FilesystemError`] for denied access and failed tails.
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

/// Report whether archive bytes fill one head window.
pub fn supported(head: &[u8]) -> bool {
    is_zip(head) || is_gzip(head) || is_tar_tag(head)
}

/// Walk archive members with one visitor.
///
/// # Errors
///
/// - [`ArchiveError::NotArchive`] for foreign bytes.
/// - [`ArchiveError::BackendCorrupt`] for corrupt archives and cut streams.
/// - [`ArchiveError::PasswordLocked`] for locked archives.
/// - [`ArchiveError::AlgorithmUnknown`] for unknown features and methods.
/// - [`confit_core::error::FilesystemError`] for denied access and missing input.
pub fn uncompress<R: io::Read + io::Seek>(
    mut reader: R,
    visit: impl FnMut(Entry<'_>) -> confit_core::error::ContextualResult<()>,
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

/// Return member names in archive order.
///
/// # Errors
///
/// - [`ArchiveError::NotArchive`] for foreign bytes.
/// - [`ArchiveError::BackendCorrupt`] for corrupt archives and cut streams.
/// - [`ArchiveError::PasswordLocked`] for locked archives.
/// - [`ArchiveError::AlgorithmUnknown`] for unknown features and methods.
/// - [`confit_core::error::FilesystemError`] for denied access and missing input.
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

/// Read one head window and restore the cursor.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] for denied access and missing input.
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

/// Report whether zip bytes fill one head window.
fn is_zip(head: &[u8]) -> bool {
    head.starts_with(&ZIP_LOCAL_MAGIC) || head.starts_with(&ZIP_EMPTY_MAGIC)
}

/// Report whether gzip bytes fill one head window.
fn is_gzip(head: &[u8]) -> bool {
    head.starts_with(&GZIP_MAGIC)
}

/// Report whether tar bytes fill one head window.
fn is_tar_tag(head: &[u8]) -> bool {
    head.get(TAR_MAGIC_OFFSET..TAR_MAGIC_OFFSET + TAR_MAGIC.len())
        .is_some_and(|tag| tag == TAR_MAGIC)
}
