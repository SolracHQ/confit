//! Gzip streams.

use std::io;

/// Raw byte count in the trailer.
pub const TRAILER_LEN: u64 = 4;

/// Gzip decoder for decoded bytes on demand.
pub struct Decoder<R: io::Read>(flate2::read::GzDecoder<R>);

impl<R: io::Read> Decoder<R> {
    /// Open one decoder.
    pub fn open(reader: R) -> Self {
        Self(flate2::read::GzDecoder::new(reader))
    }
}

impl<R: io::Read> io::Read for Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::Read::read(&mut self.0, buf)
    }
}

/// Gzip encoder at one compression level.
pub struct Encoder<W: io::Write>(flate2::write::GzEncoder<W>);

impl<W: io::Write> Encoder<W> {
    /// Open one encoder.
    pub fn open(sink: W, level: u32) -> Self {
        Self(flate2::write::GzEncoder::new(
            sink,
            flate2::Compression::new(level),
        ))
    }

    /// Finish the stream and return the sink.
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
