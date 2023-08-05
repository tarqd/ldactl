use bytes::{Buf, BufMut, Bytes, BytesMut};
use miette::Diagnostic;
use std::{cmp, io, str, usize};
use thiserror::Error;
use tokio_util::codec::{Decoder, Encoder};
use tracing::{instrument, trace};

/// A simple [`Decoder`] and [`Encoder`] implementation that splits up data into lines.
///
/// Modified from tokio_util::codec::LinesCodec to provide better diagnstics
///
/// [`Decoder`]: tokio_util::codec::Decoder
/// [`Encoder`]: tokio_util::codec::Encoder
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct LinesCodec {
    // Stored index of the next index to examine for a `\n` character.
    // This is used to optimize searching.
    // For example, if `decode` was called with `abc`, it would hold `3`,
    // because that is the next index to examine.
    // The next time `decode` is called with `abcde\n`, the method will
    // only look at `de\n` before returning.
    next_index: usize,

    /// The maximum length for a given line. If `usize::MAX`, lines will be
    /// read until a `\n` character is reached.
    max_length: usize,

    /// Are we currently discarding the remainder of a line which was over
    /// the length limit?
    is_discarding: bool,
}

impl LinesCodec {
    /// Returns a `LinesCodec` for splitting up data into lines.
    ///
    /// # Note
    ///
    /// The returned `LinesCodec` will not have an upper bound on the length
    /// of a buffered line. See the documentation for [`new_with_max_length`]
    /// for information on why this could be a potential security risk.
    ///
    /// [`new_with_max_length`]: crate::codec::LinesCodec::new_with_max_length()
    pub fn new() -> LinesCodec {
        LinesCodec {
            next_index: 0,
            max_length: usize::MAX,
            is_discarding: false,
        }
    }

    /// Returns a `LinesCodec` with a maximum line length limit.
    ///
    /// If this is set, calls to `LinesCodec::decode` will return a
    /// [`LinesCodecError`] when a line exceeds the length limit. Subsequent calls
    /// will discard up to `limit` bytes from that line until a newline
    /// character is reached, returning `None` until the line over the limit
    /// has been fully discarded. After that point, calls to `decode` will
    /// function as normal.
    ///
    /// # Note
    ///
    /// Setting a length limit is highly recommended for any `LinesCodec` which
    /// will be exposed to untrusted input. Otherwise, the size of the buffer
    /// that holds the line currently being read is unbounded. An attacker could
    /// exploit this unbounded buffer by sending an unbounded amount of input
    /// without any `\n` characters, causing unbounded memory consumption.
    ///
    /// [`LinesCodecError`]: crate::codec::LinesCodecError
    pub fn new_with_max_length(max_length: usize) -> Self {
        LinesCodec {
            max_length,
            ..LinesCodec::new()
        }
    }

    /// Returns the maximum line length when decoding.
    ///
    /// ```
    /// use std::usize;
    /// use tokio_util::codec::LinesCodec;
    ///
    /// let codec = LinesCodec::new();
    /// assert_eq!(codec.max_length(), usize::MAX);
    /// ```
    /// ```
    /// use tokio_util::codec::LinesCodec;
    ///
    /// let codec = LinesCodec::new_with_max_length(256);
    /// assert_eq!(codec.max_length(), 256);
    /// ```
    pub fn max_length(&self) -> usize {
        self.max_length
    }
}

fn without_carriage_return(s: &mut BytesMut) {
    if let Some(&b'\r') = s.last() {
        s.truncate(s.len() - 1)
    }
}

impl Decoder for LinesCodec {
    type Item = Bytes;
    type Error = LinesCodecError;
    #[instrument(skip(self, buf), err)]
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Bytes>, LinesCodecError> {
        loop {
            // Determine how far into the buffer we'll search for a newline. If
            // there's no max_length set, we'll read to the end of the buffer.
            let read_to = cmp::min(self.max_length.saturating_add(1), buf.len());

            let newline_offset = buf[self.next_index..read_to]
                .iter()
                .position(|b| *b == b'\n');
            trace!(
                ?newline_offset,
                read_to,
                buflen = buf.len(),
                "searching for new line"
            );

            match (self.is_discarding, newline_offset) {
                (true, Some(offset)) => {
                    // If we found a newline, discard up to that offset and
                    // then stop discarding. On the next iteration, we'll try
                    // to read a line normally.
                    buf.advance(offset + self.next_index + 1);
                    self.is_discarding = false;
                    self.next_index = 0;
                }
                (true, None) => {
                    // Otherwise, we didn't find a newline, so we'll discard
                    // everything we read. On the next iteration, we'll continue
                    // discarding up to max_len bytes unless we find a newline.
                    buf.advance(read_to);
                    self.next_index = 0;
                    if buf.is_empty() {
                        return Ok(None);
                    }
                }
                (false, Some(offset)) => {
                    // Found a line!
                    let newline_index = offset + self.next_index;
                    self.next_index = 0;
                    let mut line = buf.split_to(newline_index + 1);
                    line.truncate(line.len() - 1);
                    without_carriage_return(&mut line);
                    return Ok(Some(line.freeze()));
                }
                (false, None) if buf.len() > self.max_length => {
                    // Reached the maximum length without finding a
                    // newline, return an error and start discarding on the
                    // next call.
                    self.is_discarding = true;
                    return Err(LinesCodecError::MaxLineLengthExceeded(self.max_length));
                }
                (false, None) => {
                    // We didn't find a line or reach the length limit, so the next
                    // call will resume searching at the current offset.
                    self.next_index = read_to;
                    return Ok(None);
                }
            }
        }
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Bytes>, LinesCodecError> {
        Ok(match self.decode(buf)? {
            Some(frame) => Some(frame),
            None => {
                // No terminating newline - return remaining data, if any
                if buf.is_empty() || buf == &b"\r"[..] {
                    None
                } else {
                    let mut line = buf.split_to(buf.len());
                    without_carriage_return(&mut line);
                    self.next_index = 0;
                    Some(line.freeze())
                }
            }
        })
    }
}

impl<T> Encoder<T> for LinesCodec
where
    T: AsRef<str>,
{
    type Error = LinesCodecError;

    fn encode(&mut self, line: T, buf: &mut BytesMut) -> Result<(), LinesCodecError> {
        let line = line.as_ref();
        buf.reserve(line.len() + 1);
        buf.put(line.as_bytes());
        buf.put_u8(b'\n');
        Ok(())
    }
}

impl Default for LinesCodec {
    fn default() -> Self {
        Self::new()
    }
}

/// An error occurred while encoding or decoding a line.
#[derive(Debug, Error, Diagnostic)]
pub enum LinesCodecError {
    #[error("max line length exceeded (limit={0} bytes)")]
    #[diagnostic(
        help("validate the input or increase the maximum line length"),
        url(docsrs)
    )]
    MaxLineLengthExceeded(usize),
    /// An IO error occurred.
    #[error("io error")]
    Io(#[from] io::Error),
}
