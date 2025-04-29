use std::{
    fmt,
    io::{self, ErrorKind},
};

use crate::{
    reader::{ReadError, ReadResult, ReaderRef},
    tag::{OptionTag, StructType, TagParameter, TypeTag},
    writer::WriterRef,
    SmolRead, SmolWrite,
};

/// Represents serialized object bytes
pub struct RawValue(Box<[u8]>);

impl RawValue {
    /// Warning: Data does not contain header or version info, not for storing
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// Warning: Data does not contain header or version info, not for storing
    pub fn into_bytes(self) -> Box<[u8]> {
        self.0
    }

    /// Will error on (de)serializarion if invalid data was provided
    /// Assumes data is of the current version of the format
    pub fn from_bytes(data: Box<[u8]>) -> Self {
        Self(data)
    }
}

impl fmt::Debug for RawValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RawValue").finish_non_exhaustive()
    }
}

enum RawValueCopyStack {
    Object,
    Seq {
        remaining: Option<usize>,
    },
    Map {
        value_next: bool,
        remaining: Option<usize>,
    },
    Struct {
        remaining: usize,
    },
}

macro_rules! const_assert {
    ($($tt:tt)*) => {
        const _: () = {
            assert!($($tt)*);
        };
    };
}

#[derive(Debug, thiserror::Error)]
#[error("invalid data while writing a RawValue")]
pub struct InvalidRawValueData(ReadError);

impl SmolRead for RawValue {
    fn read(mut main_reader: crate::reader::ValueReader) -> crate::reader::ReadResult<Self> {
        let reader = main_reader.reader.get();
        let mut vec = vec![];
        let mut main_writer = crate::writer::Writer::new(&mut vec);
        let writer = main_writer.get_ref();

        copy_object(reader, writer)?;

        main_reader.reader.finish();

        Ok(RawValue(vec.into_boxed_slice()))
    }
}

impl SmolWrite for RawValue {
    fn write(&self, mut main_writer: crate::writer::ValueWriter) -> io::Result<()> {
        let mut cursor = std::io::Cursor::new(&self.0);
        let mut main_reader = crate::reader::Reader::new(&mut cursor);
        let reader = main_reader.get_ref();
        let writer = main_writer.writer.get();

        copy_object(reader, writer).map_err(|e| match *e {
            ReadError::Io(e) => e,
            e => io::Error::new(ErrorKind::InvalidData, InvalidRawValueData(e)),
        })?;

        main_writer.writer.finish();

        if cursor.position() != self.0.len() as u64 {
            return Err(io::Error::new(ErrorKind::InvalidData, "RawValue contains data past its object data"));
        }

        Ok(())
    }
}

fn copy_object(mut reader: ReaderRef, mut writer: WriterRef) -> ReadResult<()> {
    let mut stack: Vec<RawValueCopyStack> = vec![RawValueCopyStack::Object];
    loop {
        let Some(top) = stack.last_mut() else {
            break Ok(());
        };

        match top {
            RawValueCopyStack::Object => {
                stack.pop();
            }
            RawValueCopyStack::Seq { remaining: None } => {
                if matches!(reader.peek_tag()?, TypeTag::End) {
                    writer.write_tag(TypeTag::End).map_err(ReadError::from)?;
                    reader.consume_peek();
                    stack.pop();
                    continue;
                }
            }
            RawValueCopyStack::Seq { remaining: Some(0) } => {
                stack.pop();
                continue;
            }
            RawValueCopyStack::Seq {
                remaining: Some(remaining),
            } => {
                *remaining -= 1;
            }

            RawValueCopyStack::Map {
                value_next: value_next @ true,
                ..
            } => {
                *value_next = false;
            }
            RawValueCopyStack::Map {
                value_next: false,
                remaining: None,
            } => {
                if matches!(reader.peek_tag()?, TypeTag::End) {
                    writer.write_tag(TypeTag::End).map_err(ReadError::from)?;
                    reader.consume_peek();
                    stack.pop();
                    continue;
                }
            }
            RawValueCopyStack::Map {
                value_next: false,
                remaining: Some(0),
            } => {
                stack.pop();
                continue;
            }

            RawValueCopyStack::Map {
                value_next: false,
                remaining: Some(remaining),
            } => {
                *remaining -= 1;
            }
            RawValueCopyStack::Struct { remaining: 0 } => {
                stack.pop();
                continue;
            }
            RawValueCopyStack::Struct { remaining } => {
                let name = reader.read_str()?;
                writer.write_str(name.into()).map_err(ReadError::from)?;
                *remaining -= 1;
            }
        }

        let tag = reader.read_tag()?;

        writer.write_tag(tag).map_err(ReadError::from)?;

        match tag {
            TypeTag::Unit
            | TypeTag::Bool(_)
            | TypeTag::Integer { .. }
            | TypeTag::Char { .. }
            | TypeTag::Float(_)
            | TypeTag::Str
            | TypeTag::StrDirect
            | TypeTag::EmptyStr
            | TypeTag::Bytes
            | TypeTag::Option(OptionTag::None)
            | TypeTag::Struct(StructType::Unit)
            | TypeTag::EnumVariant(StructType::Unit) => {
                copy_tag_params(reader.clone(), writer.clone(), tag)?;
            }
            TypeTag::Option(OptionTag::Some) | TypeTag::Struct(StructType::Newtype) => {
                const_assert!(TypeTag::Option(OptionTag::Some).tag_params().is_empty());
                const_assert!(TypeTag::Struct(StructType::Newtype).tag_params().is_empty());

                stack.push(RawValueCopyStack::Object);
            }
            TypeTag::EnumVariant(StructType::Newtype) => {
                const_assert!(matches!(
                    TypeTag::EnumVariant(StructType::Newtype).tag_params(),
                    [TagParameter::StringRef]
                ));

                let name = reader.read_str()?;
                writer.write_str(name.into()).map_err(ReadError::from)?;

                stack.push(RawValueCopyStack::Object);
            }
            TypeTag::Struct(StructType::Tuple) => {
                const_assert!(matches!(
                    TypeTag::Struct(StructType::Tuple).tag_params(),
                    [TagParameter::Varint]
                ));
                let len: usize =
                    crate::varint::read_unsigned_varint(reader.inner()).map_err(ReadError::from)?;

                crate::varint::write_unsigned_varint(writer.inner(), len)
                    .map_err(ReadError::from)?;

                stack.push(RawValueCopyStack::Seq {
                    remaining: Some(len),
                });
            }
            TypeTag::Struct(StructType::Struct) => {
                const_assert!(matches!(
                    TypeTag::Struct(StructType::Struct).tag_params(),
                    [TagParameter::Varint]
                ));
                let len: usize =
                    crate::varint::read_unsigned_varint(reader.inner()).map_err(ReadError::from)?;

                crate::varint::write_unsigned_varint(writer.inner(), len)
                    .map_err(ReadError::from)?;

                stack.push(RawValueCopyStack::Struct { remaining: len });
            }
            TypeTag::EnumVariant(StructType::Tuple) => {
                const_assert!(matches!(
                    TypeTag::EnumVariant(StructType::Tuple).tag_params(),
                    [TagParameter::StringRef, TagParameter::Varint]
                ));

                let name = reader.read_str()?;
                writer.write_str(name.into()).map_err(ReadError::from)?;

                let len: usize =
                    crate::varint::read_unsigned_varint(reader.inner()).map_err(ReadError::from)?;

                crate::varint::write_unsigned_varint(writer.inner(), len)
                    .map_err(ReadError::from)?;

                stack.push(RawValueCopyStack::Seq {
                    remaining: Some(len),
                });
            }
            TypeTag::EnumVariant(StructType::Struct) => {
                const_assert!(matches!(
                    TypeTag::EnumVariant(StructType::Struct).tag_params(),
                    [TagParameter::StringRef, TagParameter::Varint]
                ));

                let name = reader.read_str()?;
                writer.write_str(name.into()).map_err(ReadError::from)?;

                let len: usize =
                    crate::varint::read_unsigned_varint(reader.inner()).map_err(ReadError::from)?;

                crate::varint::write_unsigned_varint(writer.inner(), len)
                    .map_err(ReadError::from)?;

                stack.push(RawValueCopyStack::Struct { remaining: len });
            }
            TypeTag::Array { has_length: true } => {
                const_assert!(matches!(
                    TypeTag::Array { has_length: true }.tag_params(),
                    [TagParameter::Varint]
                ));

                let len: usize =
                    crate::varint::read_unsigned_varint(reader.inner()).map_err(ReadError::from)?;

                crate::varint::write_unsigned_varint(writer.inner(), len)
                    .map_err(ReadError::from)?;

                stack.push(RawValueCopyStack::Seq {
                    remaining: Some(len),
                });
            }
            TypeTag::Array { has_length: false } => {
                const_assert!(TypeTag::Array { has_length: false }.tag_params().is_empty());

                stack.push(RawValueCopyStack::Seq { remaining: None });
            }
            TypeTag::Tuple => {
                const_assert!(matches!(
                    TypeTag::Tuple.tag_params(),
                    [TagParameter::Varint]
                ));

                let len: usize =
                    crate::varint::read_unsigned_varint(reader.inner()).map_err(ReadError::from)?;

                crate::varint::write_unsigned_varint(writer.inner(), len)
                    .map_err(ReadError::from)?;

                stack.push(RawValueCopyStack::Seq {
                    remaining: Some(len),
                });
            }
            TypeTag::Map { has_length: true } => {
                const_assert!(matches!(
                    TypeTag::Map { has_length: true }.tag_params(),
                    [TagParameter::Varint]
                ));

                let len: usize =
                    crate::varint::read_unsigned_varint(reader.inner()).map_err(ReadError::from)?;

                crate::varint::write_unsigned_varint(writer.inner(), len)
                    .map_err(ReadError::from)?;

                stack.push(RawValueCopyStack::Map {
                    value_next: false,
                    remaining: Some(len),
                });
            }
            TypeTag::Map { has_length: false } => {
                const_assert!(TypeTag::Map { has_length: false }.tag_params().is_empty());

                stack.push(RawValueCopyStack::Map {
                    value_next: false,
                    remaining: None,
                });
            }
            TypeTag::End => return Err(ReadError::UnexpectedEnd.into()),
        }
    }
}

fn copy_tag_params(mut reader: ReaderRef, mut writer: WriterRef, tag: TypeTag) -> ReadResult<()> {
    let mut buf = [0u8; 256];
    for param in tag.tag_params() {
        match *param {
            TagParameter::ShortBytes { len } => {
                let buf = &mut buf[..(len as usize)];
                reader.read_exact(buf).map_err(ReadError::from)?;
                writer.write_all(buf).map_err(ReadError::from)?;
            }
            TagParameter::Varint => {
                crate::varint::copy_varint(reader.inner(), writer.inner())
                    .map_err(ReadError::from)?;
            }
            TagParameter::VarintLengthPrefixedBytearray => {
                let len: usize =
                    crate::varint::read_unsigned_varint(reader.inner()).map_err(ReadError::from)?;

                let mut remaining = len;
                while remaining > 0 {
                    let read = remaining.min(buf.len());
                    let buf = &mut buf[..read];
                    let read = reader.read(buf).map_err(ReadError::from)?;
                    if read == 0 {
                        return Err(ReadError::Io(io::Error::new(
                            ErrorKind::UnexpectedEof,
                            "read no data while copying a tag",
                        ))
                        .into());
                    }
                    let buf = &buf[..read];
                    writer.write_all(buf).map_err(ReadError::from)?;
                    remaining -= read;
                }
            }
            TagParameter::StringRef => {
                let str = reader.read_str()?;
                writer.write_str(str.into()).map_err(ReadError::from)?;
            }
        }
    }
    Ok(())
}
