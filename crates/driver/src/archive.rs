//! Archive containers.
//!
//! Builders hold members under names and modes.
//! Readers visit members under names and modes.
//! Probes report the shape of one head window.

use std::io::{self, Read as _};

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
///
/// Default holds TarGz for compact output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Algorithm {
    /// Zip container with per-member deflation.
    Zip,
    /// Gzipped tar container with whole-stream compression.
    #[default]
    TarGz,
}

/// Compression strength for archive builders.
///
/// Default holds High for balanced output.
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

/// One archive member with its body stream.
///
/// The reader borrows the archive walk.
pub struct Entry<'a> {
    /// Container name with forward slashes.
    pub name: String,
    /// Unix permission bits.
    pub mode: u32,
    /// Reader for the member body bytes.
    pub reader: Box<dyn io::Read + 'a>,
}

/// Archive failure shapes.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Foreign bytes.
    #[error("foreign bytes hold no archive")]
    Foreign,
    /// Bare gzip single.
    #[error("bare gzip holds no archive members")]
    Bare,
    /// Locked archive.
    #[error("locked archive needs a password")]
    Locked,
    /// Unknown archive shape.
    #[error("unknown archive shape refuses members: {detail}")]
    Unknown {
        /// The refusal detail from the backend.
        detail: String,
    },
    /// Corrupt archive.
    #[error("corrupt archive refuses members: {0}")]
    Corrupt(#[source] io::Error),
    /// Cut archive.
    #[error("cut archive ends mid stream: {0}")]
    Truncated(#[source] io::Error),
    /// Refused access.
    #[error("denied access refuses members: {0}")]
    Denied(#[source] io::Error),
    /// Absent input.
    #[error("missing input refuses members: {0}")]
    Missing(#[source] io::Error),
    /// Rejected name.
    #[error("rejected name refuses members: {0}")]
    Invalid(#[source] io::Error),
    /// Transport failure.
    #[error(transparent)]
    Io(io::Error),
    /// Failing visitor.
    #[error("visitor refuses members: {0}")]
    Visit(#[source] io::Error),
}

/// Archive result alias.
pub type Result<T> = std::result::Result<T, Error>;

impl From<io::Error> for Error {
    fn from(cause: io::Error) -> Self {
        match cause.kind() {
            io::ErrorKind::InvalidData => Self::Corrupt(cause),
            io::ErrorKind::UnexpectedEof => Self::Truncated(cause),
            io::ErrorKind::PermissionDenied => Self::Denied(cause),
            io::ErrorKind::NotFound => Self::Missing(cause),
            io::ErrorKind::InvalidInput => Self::Invalid(cause),
            _ => Self::Io(cause),
        }
    }
}

impl From<Error> for io::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Io(cause)
            | Error::Visit(cause)
            | Error::Corrupt(cause)
            | Error::Truncated(cause)
            | Error::Denied(cause)
            | Error::Missing(cause)
            | Error::Invalid(cause) => cause,
            Error::Foreign => {
                Self::new(io::ErrorKind::InvalidData, "foreign bytes hold no archive")
            }
            Error::Bare => Self::new(
                io::ErrorKind::InvalidData,
                "bare gzip holds no archive members",
            ),
            Error::Locked => Self::new(
                io::ErrorKind::InvalidInput,
                "locked archive needs a password",
            ),
            Error::Unknown { detail } => Self::new(io::ErrorKind::Unsupported, detail),
        }
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(error: zip::result::ZipError) -> Self {
        let message = error.to_string();
        match error {
            zip::result::ZipError::Io(cause) => Self::Io(cause),
            zip::result::ZipError::InvalidPassword => Self::Locked,
            zip::result::ZipError::UnsupportedArchive(detail)
                if detail == zip::result::ZipError::PASSWORD_REQUIRED =>
            {
                Self::Locked
            }
            zip::result::ZipError::UnsupportedArchive(_) => Self::Unknown { detail: message },
            zip::result::ZipError::CompressionMethodNotSupported(method) => Self::Unknown {
                detail: format!("compression method {method}"),
            },
            zip::result::ZipError::FileNotFound | zip::result::ZipError::InvalidArchive(_) => {
                Self::Corrupt(io::Error::new(io::ErrorKind::InvalidData, message))
            }
            _ => Self::Unknown { detail: message },
        }
    }
}

/// Streaming archive writer over one seekable sink.
///
/// The writer holds one backend and its level.
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
    /// - Names carry forward slashes.
    /// - Modes carry unix permission bits.
    /// - Len sizes tar headers.
    /// - Zip members stream reader bytes directly.
    ///
    /// # Errors
    ///
    /// - [`Error::Corrupt`] for rejected zip framing.
    /// - [`Error::Invalid`] for rejected names.
    /// - [`Error::Denied`] for refused access.
    /// - [`Error::Io`] for failed readers and sinks.
    pub fn member<R: io::Read>(
        &mut self,
        name: &str,
        mode: u32,
        len: u64,
        reader: &mut R,
    ) -> Result<()> {
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
                writer.start_file(name, options)?;
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
    /// - [`Error::Corrupt`] for broken zip tails.
    /// - [`Error::Denied`] for refused access.
    /// - [`Error::Io`] for failed tails.
    pub fn finish(self) -> Result<W> {
        match self.inner {
            Inner::Zip { writer, .. } => Ok((*writer).finish()?),
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
/// - Every visited member is a file with a non-empty name.
///
/// # Errors
///
/// - [`Error::Foreign`] for foreign bytes.
/// - [`Error::Bare`] for bare gzip singles.
/// - [`Error::Locked`] for locked archives.
/// - [`Error::Unknown`] for refused features and methods.
/// - [`Error::Corrupt`] for corrupt archives.
/// - [`Error::Truncated`] for cut streams.
/// - [`Error::Denied`] for refused access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams and entries.
/// - [`Error::Visit`] for failing visitors.
pub fn uncompress<R: io::Read + io::Seek>(
    mut reader: R,
    visit: impl FnMut(Entry<'_>) -> io::Result<()>,
) -> Result<()> {
    let head = peek_head(&mut reader)?;
    if is_zip(&head) {
        return zip_ext::walk(reader, visit);
    }
    if is_gzip(&head) || is_tar_tag(&head) {
        return targz::walk(reader, visit);
    }
    Err(Error::Foreign)
}

/// Return member names in archive order.
///
/// - Every returned name is a file with a non-empty name.
///
/// # Errors
///
/// - [`Error::Foreign`] for foreign bytes.
/// - [`Error::Bare`] for bare gzip singles.
/// - [`Error::Locked`] for locked archives.
/// - [`Error::Unknown`] for refused features and methods.
/// - [`Error::Corrupt`] for corrupt archives.
/// - [`Error::Truncated`] for cut streams.
/// - [`Error::Denied`] for refused access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams and entries.
pub fn names<R: io::Read + io::Seek>(mut reader: R) -> Result<Vec<String>> {
    let head = peek_head(&mut reader)?;
    if is_zip(&head) {
        return zip_ext::names(reader);
    }
    if is_gzip(&head) || is_tar_tag(&head) {
        return targz::names(reader);
    }
    Err(Error::Foreign)
}

/// Read one head window and restore the cursor.
///
/// # Errors
///
/// - [`Error::Denied`] for refused access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams.
fn peek_head<R: io::Read + io::Seek>(reader: &mut R) -> Result<Vec<u8>> {
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
