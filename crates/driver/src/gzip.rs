//! Gzip single streams over caller handles with io errors.
//!
//! Blob pool bytes ride these seams, so digests
//! with footers stay home in the store.

use std::io;

/// Trailer holding the raw byte count.
pub const TRAILER_LEN: u64 = 4;

/// Gzip decoder reading decoded bytes on demand.
///
/// The reader decodes bytes as the caller pulls,
/// so spill verbs stream without staging.
pub struct Decoder<R: io::Read>(flate2::read::GzDecoder<R>);

impl<R: io::Read> Decoder<R> {
    /// Opens one decoder over a caller reader.
    pub fn open(reader: R) -> Self {
        Self(flate2::read::GzDecoder::new(reader))
    }
}

impl<R: io::Read> io::Read for Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::Read::read(&mut self.0, buf)
    }
}

/// Gzip encoder compressing at a caller level.
///
/// Bytes compress as the caller pushes, so pool
/// spills stream straight into staging.
pub struct Encoder<W: io::Write>(flate2::write::GzEncoder<W>);

impl<W: io::Write> Encoder<W> {
    /// Opens one encoder over a caller sink.
    pub fn open(sink: W, level: u32) -> Self {
        Self(flate2::write::GzEncoder::new(
            sink,
            flate2::Compression::new(level),
        ))
    }

    /// Finishes the stream returning the caller sink.
    ///
    /// # Errors
    ///
    /// Sink writes fail as io errors.
    pub fn finish(self) -> io::Result<W> {
        self.0.finish()
    }
}

impl<W: io::Write> io::Write for Encoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        io::Write::write(&mut self.0, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.0)
    }
}
