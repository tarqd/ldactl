use super::errors::DecodeUtf8Error;
use bytes::Buf;
// We only support UTF-8 in this house
const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

pub(crate) trait BufExt: Buf {
    fn bump(&mut self);
    fn bump_if(&mut self, byte: u8);
    fn find_byte(&self, byte: u8) -> Option<usize>;
    // allow unused
    #[allow(unused)]
    fn strip_utf8_bom(&mut self);
}
pub(crate) trait BufMutExt: Buf {
    fn rbump_if(&mut self, byte: u8);
    fn rbump(&mut self);
}

impl<T> BufExt for T
where
    T: Buf + AsRef<[u8]>,
{
    /// Equivalent of [`Buf::advance(1)`](`Buf::advance`)
    ///
    /// ```
    #[inline(always)]
    fn bump(&mut self) {
        self.advance(1);
    }

    /// Call [`BufExt::bump()`] if the first byte of the buffer is equal to `byte`
    ///
    #[inline]
    fn bump_if(&mut self, byte: u8) {
        if self.as_ref().first() == Some(&byte) {
            self.advance(1);
        }
    }
    /// Returns the position of the first byte matching `byte`
    ///
    /// ```
    #[inline]
    fn find_byte(&self, byte: u8) -> Option<usize> {
        // TODO: make this optionally use burntsushi/memchr
        self.as_ref().iter().position(|b| *b == byte)
    }

    /// Advances the buffer cursor past the UTF-8 BOM if it exists
    #[inline]
    fn strip_utf8_bom(&mut self) {
        if self.as_ref().starts_with(UTF8_BOM) {
            self.advance(UTF8_BOM.len());
        }
    }
}

impl BufMutExt for bytes::BytesMut {
    /// Truncates the buffer if the last byte is equal to `byte`
    ///
    #[inline]
    fn rbump_if(&mut self, byte: u8) {
        if self.last() == Some(&byte) {
            // ! SAFETY
            // we just checked that there's at least 1 byte
            unsafe {
                self.set_len(self.len() - 1);
            }
        }
    }

    #[inline]
    fn rbump(&mut self) {
        if !self.is_empty() {
            unsafe { self.set_len(self.len() - 1) }
        }
    }
}

impl BufMutExt for bytes::Bytes {
    /// Truncates the buffer if the last byte is equal to `byte`
    ///
    #[inline]
    fn rbump_if(&mut self, byte: u8) {
        if self.last() == Some(&byte) {
            // ! SAFETY
            // we just checked that there's at least 1 byte
            self.truncate(self.len() - 1);
        }
    }

    #[inline]
    fn rbump(&mut self) {
        if !self.is_empty() {
            self.truncate(self.len() - 1);
        }
    }
}

/// Extension trait that allows decoding bytes as UTF-8
/// Wrapping errors with [crate::diagnostic_errors::UTF8Error]
pub trait Utf8DecodeDiagnostic {
    /// Decodes the bytes as UTF-8, only allocating if there's an error for reporting
    fn decode_utf(&self) -> Result<&str, DecodeUtf8Error>;
}
impl<T> Utf8DecodeDiagnostic for T
where
    T: AsRef<[u8]> + bytes::Buf,
{
    fn decode_utf(&self) -> Result<&str, DecodeUtf8Error> {
        // ! SAFETY
        // This is safe because the buffer passed to from_std is the same one
        // We got the error from, otherwise the labels might point to invalid spans
        std::str::from_utf8(self.as_ref())
            .map_err(|e| unsafe { DecodeUtf8Error::from_std(e, self.as_ref().to_vec()) })
    }
}

#[cfg(test)]
mod tests {
    use crate::bufext::BufExt;
    use bytes::{Buf, Bytes};
    #[test]
    fn advance_if() {
        let mut bytes = Bytes::from_static(b"Hello, world!");
        bytes.bump_if(b'H');
        assert_eq!(bytes.as_ref(), b"ello, world!");
        let mut bytes = Bytes::from_static(b"f");
        bytes.advance(1);
        assert!(bytes.is_empty());
        bytes.bump_if(b'f');
        assert!(bytes.is_empty());
    }
    #[test]
    fn find_pos() {
        let bytes = Bytes::from_static(b"Hello, world!");
        let pos = bytes.find_byte(b',');
        assert_eq!(pos, Some(5));
        assert_eq!(bytes[pos.unwrap()], b',');

        let bytes = Bytes::from_static(b"Hello, world!");
        let pos = bytes.find_byte(b'X');
        assert_eq!(pos, None);
    }
    #[test]
    fn strip_utf8_bom() {
        //0xEF, 0xBB, 0xBF
        let mut bytes = Bytes::from_static(b"\xEF\xBB\xBFHello, world!");
        bytes.strip_utf8_bom();
        println!("BOM: {:?}", super::UTF8_BOM);
        assert_eq!(bytes.as_ref(), b"Hello, world!");
    }
    #[test]
    fn strip_utf8_bom_slice() {
        //0xEF, 0xBB, 0xBF
        let mut bytes = b"\xEF\xBB\xBFHello, world!".as_ref();
        bytes.strip_utf8_bom();
        assert_eq!(bytes, b"Hello, world!");
    }
}
