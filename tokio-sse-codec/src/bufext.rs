use super::diagnostic_errors;
use bytes::Buf;
// We only support UTF-8 in this house
const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

pub(crate) trait BufExt {
    fn find_pos(&self, byte: u8) -> Option<usize>;
    fn advance_if(&mut self, byte: u8);
    fn strip_utf8_bom(&mut self);
}

impl BufExt for &[u8] {
    #[inline]
    fn advance_if(&mut self, byte: u8) {
        if !self.is_empty() && self[0] == byte {
            self.advance(1)
        }
    }
    #[inline]
    fn find_pos(&self, byte: u8) -> Option<usize> {
        self.iter().position(|b| *b == byte)
    }
    #[inline]
    fn strip_utf8_bom(&mut self) {
        if self.starts_with(UTF8_BOM) {
            self.advance(UTF8_BOM.len() + 1);
        }
    }
}

impl BufExt for bytes::Bytes {
    fn find_pos(&self, byte: u8) -> Option<usize> {
        self.iter().position(|b| *b == byte)
    }

    fn advance_if(&mut self, byte: u8) {
        if !self.is_empty() && self[0] == byte {
            self.advance(1)
        }
    }

    fn strip_utf8_bom(&mut self) {
        if self.starts_with(UTF8_BOM) {
            self.advance(UTF8_BOM.len() + 1);
        }
    }
}

impl BufExt for bytes::BytesMut {
    fn find_pos(&self, byte: u8) -> Option<usize> {
        self.iter().position(|b| *b == byte)
    }

    fn advance_if(&mut self, byte: u8) {
        if !self.is_empty() && self[0] == byte {
            self.advance(1)
        }
    }

    fn strip_utf8_bom(&mut self) {
        if self.starts_with(UTF8_BOM) {
            self.advance(UTF8_BOM.len() + 1);
        }
    }
}

/// Extension trait that allows decoding bytes as UTF-8
/// Wrapping errors with [crate::diagnostic_errors::UTF8Error]
pub trait Utf8DecodeDiagnostic {
    fn decode_utf8<'a>(&'a self) -> Result<&'a str, diagnostic_errors::UTF8Error>;
    fn decode_utf8_owned(&self) -> Result<String, diagnostic_errors::UTF8Error>;
}

impl Utf8DecodeDiagnostic for bytes::Bytes {
    fn decode_utf8<'a>(&'a self) -> Result<&'a str, diagnostic_errors::UTF8Error> {
        std::str::from_utf8(self).map_err(|e| diagnostic_errors::UTF8Error::new(e, self.slice(0..)))
    }

    fn decode_utf8_owned(&self) -> Result<String, diagnostic_errors::UTF8Error> {
        self.decode_utf8().map(String::from)
    }
}

impl Utf8DecodeDiagnostic for bytes::BytesMut {
    fn decode_utf8<'a>(&'a self) -> Result<&'a str, diagnostic_errors::UTF8Error> {
        std::str::from_utf8(self.as_ref())
            .map_err(|e| diagnostic_errors::UTF8Error::new(e, self.clone().freeze()))
    }

    fn decode_utf8_owned(&self) -> Result<String, diagnostic_errors::UTF8Error> {
        String::from_utf8(self.to_vec()).map_err(|v| {
            diagnostic_errors::UTF8Error::new(v.utf8_error(), bytes::Bytes::from(v.into_bytes()))
        })
    }
}
