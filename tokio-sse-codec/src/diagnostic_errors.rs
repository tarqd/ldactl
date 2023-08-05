#[deny(warnings)]
use bytes::Bytes;
use miette::{Diagnostic, SourceCode, SourceSpan};
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
#[error("invalid utf-8")]
pub struct UTF8Error {
    #[source]
    source: std::str::Utf8Error,
    #[source_code]
    valid_buf: BytesSource,
    buf: Bytes,
    #[label("followed by {} more bytes", self.buf.len() - self.valid_buf.0.len() - self.source.error_len().unwrap_or(0))]
    remaining: Option<SourceSpan>,
    #[label("invalid data starts here: {}", self.invalid_format())]
    at: SourceSpan,
}

#[derive(Debug)]
struct BytesSource(Bytes);
impl SourceCode for BytesSource {
    fn read_span<'a>(
        &'a self,
        span: &SourceSpan,
        context_lines_before: usize,
        context_lines_after: usize,
    ) -> Result<Box<dyn miette::SpanContents<'a> + 'a>, miette::MietteError> {
        self.0
            .as_ref()
            .read_span(span, context_lines_before, context_lines_after)
    }
}

impl UTF8Error {
    /// Create a new UTF8Error from a Utf8Error and a buffer
    /// See [crate::bufext::]
    pub(crate) fn new(source: std::str::Utf8Error, buf: Bytes) -> Self {
        let displayed_len = source.valid_up_to() + source.error_len().unwrap_or(0);
        println!("displayed_len: {}, buf len: {}", displayed_len, buf.len());

        let valid = buf.slice(0..source.valid_up_to());
        let remaining = if displayed_len < buf.len() {
            let last_line =
                (valid.iter().rposition(|b| *b == b'\n').unwrap_or(0) + 1).min(valid.len());
            Some(SourceSpan::from(last_line..valid.len()))
        } else {
            None
        };
        let span = valid.len() - 1..valid.len();
        let s = Self {
            source,
            valid_buf: BytesSource(valid),
            buf,
            at: span.into(),
            remaining: remaining,
        };

        s
    }
    /// Returns valid portion of the buffer before the parsing error occurred
    pub fn valid_buf(&self) -> Bytes {
        self.valid_buf.0.slice(0..)
    }
    /// Returns the entire buffer that was being parsed when the error occurred
    pub fn bytes(&self) -> Bytes {
        self.buf.slice(0..)
    }

    fn invalid_format(&self) -> String {
        let start = self.source.valid_up_to();
        let end = {
            let end = start + self.source.error_len().unwrap_or(1);
            if end > self.buf.len() {
                self.buf.len()
            } else {
                end
            }
        };

        let invalid = self.buf.slice(self.source.valid_up_to()..end);
        if invalid.is_empty() {
            String::new()
        } else {
            format!("{:#04X?}", invalid)
        }
    }
}
