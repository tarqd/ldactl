#![allow(warnings)]
use std::{borrow::BorrowMut, ops::Range, thread::current};

use crate::bufext::{BufExt, BufMutExt};
use bytes::{Buf, Bytes, BytesMut};
use tokio_util::codec::Decoder;
use tracing::field;
#[derive(Debug, Default)]
pub struct SseFieldDecoder {
    state: State,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Field(FieldKind, Bytes);

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum FieldKind {
    Data,
    Event,
    Retry,
    Comment,
    Id,
    UnknownField(Bytes),
}

#[derive(Debug, PartialEq, Eq)]
pub enum FieldFrame {
    Field(Field),
    EmptyLine,
}

impl From<Field> for FieldFrame {
    fn from(field: Field) -> Self {
        Self::Field(field)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum State {
    Bom,
    NextFrame,
    Field {
        next_colon_index: usize,
    },
    Value {
        field_kind: FieldKind,
        next_line_index: usize,
    },
}
impl State {
    fn new() -> Self {
        Self::Bom
    }
    #[inline(always)]
    fn field() -> Self {
        Self::field_at(0)
    }
    #[inline(always)]
    fn field_at(at: usize) -> Self {
        Self::Field {
            next_colon_index: at,
        }
    }
    #[inline(always)]
    fn value(kind: FieldKind) -> Self {
        Self::value_at(kind, 0)
    }
    #[inline(always)]
    fn value_at(kind: FieldKind, at: usize) -> Self {
        Self::Value {
            field_kind: kind,
            next_line_index: at,
        }
    }
    #[inline(always)]
    fn set_next_field(&mut self) {
        *self = Self::field();
    }
    #[inline(always)]
    fn set_next_field_at(&mut self, at: usize) {
        *self = Self::field_at(at);
    }
    #[inline(always)]
    fn set_next_value(&mut self, kind: FieldKind) {
        *self = Self::value(kind);
    }
    #[inline(always)]
    fn set_value_at(&mut self, at: usize) {
        match self {
            State::Value {
                field_kind,
                next_line_index,
            } => *next_line_index = at,
            _ => unreachable!(),
        }
    }
    #[inline(always)]
    fn set_next_frame(&mut self) {
        *self = Self::NextFrame;
    }
    #[inline(always)]
    fn take_field(&mut self, next_state: State, value: BytesMut) -> Field {
        let current_state = std::mem::replace(self, next_state);
        match current_state {
            Self::Value { field_kind, .. } => Field(field_kind, value.freeze()),
            _ => unreachable!(),
        }
    }
}
impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for SseFieldDecoder {
    type Item = FieldFrame;

    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
        loop {
            match self.state.borrow_mut() {
                _ if src.is_empty() => break Ok(None),
                State::Bom => {
                    let read_to = UTF8_BOM.len().min(src.len());
                    match src.get(0..read_to) {
                        Some(bom) if bom == UTF8_BOM => {
                            src.advance(read_to);
                            self.state.borrow_mut().set_next_frame();
                            continue;
                        }
                        // maybe a partial BOM
                        Some(&[0xEF]) | Some(&[0xEF, 0xBB]) => {
                            // we need more data
                            break Ok(None);
                        }
                        Some(_) => {
                            // not a BOM
                            self.state.set_next_frame();
                            continue;
                        }
                        None => {
                            break Ok(None);
                        }
                    }
                }
                State::NextFrame => match src[0] {
                    b':' => {
                        src.advance(1);
                        self.state.set_next_value(FieldKind::Comment);
                        continue;
                    }
                    b'\r' if src.get(1) == Some(&b'\n') => {
                        src.advance(2);
                        self.state.set_next_frame();
                        break Ok(Some(FieldFrame::EmptyLine));
                    }
                    b'\n' => {
                        src.advance(1);
                        break Ok(Some(FieldFrame::EmptyLine));
                    }
                    _ => {
                        self.state.set_next_field();
                        continue;
                    }
                },
                State::Field { next_colon_index } => {
                    let start_from = *next_colon_index;
                    let read_to = src.len();
                    let line_or_colon_index = src[start_from..read_to]
                        .iter()
                        .position(|b| *b == b':' || *b == b'\n')
                        .map(|offset| {
                            let index = start_from + offset;
                            (index, src[index])
                        });

                    match line_or_colon_index {
                        Some((colon_index, b':')) => {
                            let field_kind = src.split_to(colon_index);
                            src.bump();
                            let field_kind = match field_kind.as_ref() {
                                b"data" => FieldKind::Data,
                                b"event" => FieldKind::Event,
                                b"retry" => FieldKind::Retry,
                                b"id" => FieldKind::Id,
                                _ => FieldKind::UnknownField(field_kind.freeze()),
                            };
                            self.state.set_next_value(field_kind);
                            continue;
                        }
                        Some((line_index, b'\n')) => {
                            let line = src.split_to(line_index + 1);
                            self.state.set_next_frame();

                            // no colon before new line, treat the whole thing as a field
                            break Ok(Some(
                                Field(FieldKind::UnknownField(line.freeze()), Bytes::default())
                                    .into(),
                            ));
                        }
                        Some(_) => unreachable!(),
                        None => {
                            // we need to keep looking
                            *next_colon_index = read_to;
                            break Ok(None);
                        }
                    }
                }
                State::Value {
                    field_kind,
                    next_line_index,
                } => {
                    let read_to = src.len();
                    let start_from = *next_line_index;
                    let new_line_index = src[start_from..read_to]
                        .iter()
                        .position(|b| *b == b'\n')
                        .map(|offset| start_from + offset);
                    match new_line_index {
                        Some(new_line_index) => {
                            // ready to parse value

                            // includes the \n
                            let mut value = src.split_to(new_line_index + 1);
                            // extract the field name for unknown fields

                            // skip the first whitespace
                            value.bump_if(b' ');
                            // we leave the new line alone so upstream decoders
                            // can use it to implement effecient SSE proxies
                            // that take advantage of the trailing new line

                            // kind of a weird dance we need to do here
                            let field = self.state.take_field(State::NextFrame, value);
                            break Ok(Some(field.into()));
                        }
                        None => {
                            *next_line_index = read_to;
                            break Ok(None);
                        }
                    }
                }
            }
        }
    }
    fn decode_eof(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            Ok(None)
        } else {
            let value = self.decode(src)?;
            if value.is_some() {
                Ok(value)
            } else {
                Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use bytes::BufMut;

    use super::*;
    #[test]
    fn empty_line() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(result, Some(FieldFrame::EmptyLine));
    }
    #[test]
    fn empty_line_cr() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("\r\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(result, Some(FieldFrame::EmptyLine));
    }
    #[test]
    fn named_event() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("event: test\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Event,
                Bytes::from_static(b"test\n")
            )))
        );
    }
    #[test]
    fn data_field() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("data: test\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Data,
                Bytes::from_static(b"test\n")
            )))
        );
    }
    #[test]
    fn data_field_split() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("data: hello ");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(result, None);
        buf.put("world\n".as_bytes());
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Data,
                Bytes::from_static(b"hello world\n")
            )))
        );
    }
    #[test]
    fn comment_field() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from(": test\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Comment,
                Bytes::from_static(b"test\n")
            )))
        );
    }
    #[test]
    fn retry_field() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("retry: 1000\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Retry,
                Bytes::from_static(b"1000\n")
            )))
        );
    }
    #[test]
    fn id_field() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("id: 1000\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Id,
                Bytes::from_static(b"1000\n")
            )))
        );
    }
    #[test]
    fn unamed_event() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("event:\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Event,
                Bytes::from_static(b"\n")
            )))
        );
    }
    #[test]
    fn unamed_event_with_space() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("event: \n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Event,
                Bytes::from_static(b"\n")
            )))
        );
    }
    #[test]
    fn values_trim_first_space() {
        let fields = vec!["event", "data", "retry", "id", "unknown"];
        for field in fields {
            let mut decoder = SseFieldDecoder::default();
            let mut buf = BytesMut::from(format!("{}: value\n", field).as_bytes());
            let result = decoder.decode(&mut buf).unwrap();
            assert!(
                matches!(result, Some(FieldFrame::Field(Field(_, value))) if value.as_ref() == b"value\n")
            );
        }
    }
    #[test]
    fn field_no_colon() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("event\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::UnknownField(Bytes::from_static(b"event\n")),
                Bytes::default()
            )))
        );
    }
    #[test]
    fn field_cr() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("event\r:\r\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::UnknownField(Bytes::from_static(b"event\r")),
                Bytes::from_static(b"\r\n")
            )))
        );
    }
    #[test]
    fn strips_bom() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("\u{feff}event: test\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Event,
                Bytes::from_static(b"test\n")
            )))
        );
    }
    #[test]
    fn strips_partial_bom() {
        let mut decoder = SseFieldDecoder::default();
        let partial_bom: &[u8] = &[0xEF];
        let mut buf = BytesMut::from(partial_bom);
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(result, None);
        buf.put_u8(0xBB);
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(result, None);
        buf.put_u8(0xBF);
        buf.put(b"event: test\n".as_ref());
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Event,
                Bytes::from_static(b"test\n")
            )))
        );
    }
    #[test]
    fn does_not_strip_partial_invalid_bom() {
        let mut decoder = SseFieldDecoder::default();
        let partial_bom: &[u8] = &[0xEF, 0xBB];
        let mut buf = BytesMut::from(partial_bom);
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(result, None);
        buf.put("event: test\n".as_bytes());
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::UnknownField(Bytes::from_static(b"\xEF\xBBevent")),
                Bytes::from_static(b"test\n")
            )))
        );
    }
    #[test]
    fn does_not_strip_inner_bom() {
        let mut decoder = SseFieldDecoder::default();
        let mut buf = BytesMut::from("event: \u{feff}test\n");
        let result = decoder.decode(&mut buf).unwrap();
        assert_eq!(
            result,
            Some(FieldFrame::Field(Field(
                FieldKind::Event,
                Bytes::from_static("\u{feff}test\n".as_bytes())
            )))
        );
    }
}
