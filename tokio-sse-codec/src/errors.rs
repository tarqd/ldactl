use miette::{Diagnostic, LabeledSpan, SourceCode, SourceSpan};
use std::string::FromUtf8Error;
use std::{fmt::Display, str::Utf8Error as StdUtf8Error};
use thiserror::Error;
use miette::Context
/// Type for unrecoverable decoder errors
/// Returned by [`SSEDecoder::decode`] and [`SSEDecoder::decode_eof`].
///
/// All of these errors are considered fatal and you should call decode again. You may use [`SSEDecoder::reset`] clear the internal state
/// and re-use the decoder. This allows you to keep the allocated capacity in the internal buffers
///
/// [`SSEDecoder::decode`]: ./struct.SSEDecoder.html#method.decode
/// [`SSEDecoder::decode_eof`]: ./struct.SSEDecoder.html#method.decode_eof
/// [`SSEDecoder::reset`]: ./struct.SSEDecoder.html#method.reset
#[derive(Error, Diagnostic, Debug)]
pub enum SSEDecodeError {
    /// [`std::io::Error`], generally coming from the underlying stream
    #[error("i/o error while reading stream")]
    #[diagnostic(code(tokio_sse_codec::decoder::io_error), url(docsrs))]
    Io(#[from] std::io::Error),
    /// The stream ended unexpectedly and we had a partial event in the buffers before we had enough data to dispatch it
    #[error("unexpected end of stream")]
    #[diagnostic(
        help("The input ended before completing the last event. Ensure that the source is sending an empty line after each event"),
        code(tokio_sse_codec::decoder::unexpected_eof),
        url(docsrs)
    )]
    UnexpectedEof,
    /// Invalid UTF-8 data was found in the stream
    #[error(transparent)]
    #[diagnostic(transparent)]
    Utf8Error(#[from] UTF8Error),
    /// The maximum buffer size was exceeded before we could dispatch the event being read.
    #[error(transparent)]
    #[diagnostic(transparent)]
    ExceededSizeLimit(ExceededSizeLimit),
}

impl From<SSEDecodeError> for std::io::Error {
    fn from(e: SSEDecodeError) -> Self {
        match e {
            SSEDecodeError::Io(ref io) => std::io::Error::new(io.kind(), e),
            SSEDecodeError::UnexpectedEof => {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, e)
            }

            SSEDecodeError::Utf8Error(_) => std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            SSEDecodeError::ExceededSizeLimit(..) => {
                std::io::Error::new(std::io::ErrorKind::Other, e)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DecodeUtf8ErrorInner {
    source: StdUtf8Error,
    buf: Vec<u8>,
}
impl DecodeUtf8ErrorInner {
    fn valid_str(&self) -> Option<&str> {
        let start = self.source.valid_up_to();
        let end = match self.source.error_len() {
            Some(len) => start + len,
            None => return None,
        };
        self.buf
            .get(start..end)
            .and_then(|s| std::str::from_utf8(s).ok())
    }
    fn remaining_label(&self) -> Option<LabeledSpan> {
        let valid_offset = self.source.valid_up_to();
        let error_len = self.source.error_len()?;
        let displayed_offset = valid_offset + error_len;
        let remaining_len = self.buf.len() - displayed_offset;
        let valid_buf = &self.buf[0..valid_offset];
        if remaining_len == 0 {
            None
        } else {
            // Start the span at the last line or the start of the valid buffer
            let start_pos = valid_buf.iter().rposition(|b| *b == b'\n').unwrap_or(0);
            Some(LabeledSpan::new_with_span(
                if remaining_len == 1 {
                    Some("followed by 1 more byte".into())
                } else {
                    Some(format!("followed by {} more bytes", remaining_len))
                },
                SourceSpan::from(start_pos..valid_buf.len()),
            ))
        }
    }
    fn invalid_label(&self) -> Option<LabeledSpan> {
        let valid_len = self.source.valid_up_to();

        let buf: &[u8] = self.buf.as_ref();
        self.source
            .error_len()
            .map(|len| {
                let span = SourceSpan::from(0..valid_len);
                LabeledSpan::new_with_span(
                    Some(format!(
                        "invalid data starts here: {:#04X?}",
                        &buf[valid_len..len]
                    )),
                    span,
                )
            })
            .or_else(|| {
                // when error_len is None valid_len.. contains the incomplete sequence
                Some(LabeledSpan::at_offset(
                    valid_len,
                    format!(
                        "unexpected end of utf8 sequence: {:#04X?}",
                        &buf[valid_len..]
                    ),
                ))
            })
    }
}

impl SourceCode for DecodeUtf8ErrorInner {
    fn read_span<'a>(
        &'a self,
        span: &SourceSpan,
        context_lines_before: usize,
        context_lines_after: usize,
    ) -> Result<Box<dyn miette::SpanContents<'a> + 'a>, miette::MietteError> {
        let buf = self
            .valid_str()
            .ok_or_else(|| miette::MietteError::OutOfBounds)?;

        <str as SourceCode>::read_span(buf, span, context_lines_before, context_lines_after)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UTF8Error {
    inner: DecodeUtf8ErrorInner,
}
impl std::error::Error for UTF8Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.inner.source)
    }
}
impl Display for UTF8Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.source.fmt(f)
    }
}
impl From<FromUtf8Error> for UTF8Error {
    fn from(e: FromUtf8Error) -> Self {
        Self {
            inner: DecodeUtf8ErrorInner {
                source: e.utf8_error(),
                buf: e.into_bytes(),
            },
        }
    }
}

impl UTF8Error {
    pub(crate) unsafe fn from_std(source: StdUtf8Error, buf: Vec<u8>) -> Self {
        Self {
            inner: DecodeUtf8ErrorInner { source, buf },
        }
    }
}

impl Diagnostic for UTF8Error {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::<&str>::new("tokio_sse_codec::decoder::utf8_error"))
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(miette::Severity::Error)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        None
    }

    fn url<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        let version = option_env!("CARGO_PKG_VERSION").unwrap_or("latest");
        Some(Box::<String>::new(format!(
            "https://docs.rs/tokio-sse-codec/{}/tokio_sse_codec/decoder/struct.UTF8Error.html",
            version
        )))
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        Some(&self.inner as &dyn miette::SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        let labels = [self.inner.remaining_label(), self.inner.invalid_label()];
        Some(Box::new(labels.into_iter().flatten()))
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
        None
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        None
    }
}

#[derive(Error, Diagnostic, Debug)]
#[error("exceeded limit of {limit} bytes for buffer size")]
#[diagnostic(
    help("Ensure that the source is sending an empty line after each event and you are connected to a valid SSE stream."),
    code(tokio_sse_codec::decoder::exceeded_size_limit),
    url(docsrs)
)]
pub struct ExceededSizeLimit {
    limit: usize,
    incoming_len: usize,
    consumed_len: usize,
}

impl ExceededSizeLimit {
    pub fn new(limit: usize, incoming_len: usize, consumed_len: usize) -> Self {
        Self {
            limit,
            incoming_len,
            consumed_len,
        }
    }
    pub fn limit(&self) -> usize {
        self.limit
    }
    pub fn incoming_len(&self) -> usize {
        self.incoming_len
    }
    pub fn consumed_len(&self) -> usize {
        self.consumed_len
    }
}

impl From<ExceededSizeLimit> for SSEDecodeError {
    fn from(e: ExceededSizeLimit) -> Self {
        Self::ExceededSizeLimit(e)
    }
}

impl From<FromUtf8Error> for SSEDecodeError {
    fn from(e: FromUtf8Error) -> Self {
        Self::Utf8Error(e.into())
    }
}
