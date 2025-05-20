use std::{
    fmt,
    io::{self, ErrorKind},
};

use crate::{
    copy, reader::{ReadError, ReadResult, ReaderRef}, tag::{OptionTag, StructTag, Tag}, writer::WriterRef, SmolRead, SmolWrite, FORMAT_VERSION
};

/// Represents serialized object bytes
pub struct RawValue(Box<[u8]>);

impl RawValue {
    /// Warning: Data does not contain header or version info, not for storing
    /// Use `Reader::new_headerless` with `FORMAT_VERSION` version to deserialize data
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
    
    /// Warning: Data does not contain header or version info, not for storing
    /// Use `Reader::new_headerless` with `FORMAT_VERSION` version to deserialize data
    pub fn into_bytes(self) -> Box<[u8]> {
        self.0
    }

    /// Will error on (de)serializarion if invalid data was provided
    /// Assumes headerless data of the current version of the format
    pub fn from_bytes(data: Box<[u8]>) -> Self {
        Self(data)
    }

    pub fn write_object<T: SmolWrite>(obj: &T) -> Result<Self, io::Error> {
        let mut vec = vec![];
        let mut writer = crate::writer::Writer::new_headerless(&mut vec);
        obj.write(writer.write())?;
        writer.finish()?;
        Ok(Self(vec.into_boxed_slice())) 
    }

    pub fn read_object<T: SmolRead>(&self) -> ReadResult<T> {
        let mut cur = io::Cursor::new(&self.0);
        let mut reader = crate::reader::Reader::new_headerless(&mut cur, FORMAT_VERSION);
        let obj = T::read(reader.read())?;
        reader.finish();
        Ok(obj)
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

#[derive(Debug, thiserror::Error)]
#[error("invalid data while writing a RawValue")]
pub struct InvalidRawValueData(ReadError);

impl SmolRead for RawValue {
    fn read(mut main_reader: crate::reader::ValueReader) -> crate::reader::ReadResult<Self> {
        let reader = main_reader.reader.get();
        let mut vec = vec![];
        let mut main_writer = crate::writer::Writer::new_headerless(&mut vec);
        let writer = main_writer.get_ref();

        copy_object(reader, writer)?;

        main_reader.reader.finish();

        Ok(RawValue(vec.into_boxed_slice()))
    }
}

impl SmolWrite for RawValue {
    fn write(&self, mut main_writer: crate::writer::ValueWriter) -> io::Result<()> {
        let mut cursor = std::io::Cursor::new(&self.0);
        let mut main_reader = crate::reader::Reader::new_headerless(&mut cursor, FORMAT_VERSION);
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
                if reader.read_seq_end()? {
                    writer.write_seq_end().map_err(ReadError::from)?;
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
                if reader.read_seq_end()? {
                    writer.write_seq_end().map_err(ReadError::from)?;
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

        writer.write_tag(tag.clone()).map_err(ReadError::from)?;

        match tag {
            Tag::Unit
            | Tag::Bool(_)
            | Tag::Integer { .. }
            | Tag::Char { .. }
            | Tag::F32(_)
            | Tag::F64(_)
            | Tag::Str(_)
            | Tag::EmptyStr
            | Tag::Option(OptionTag::None)
            | Tag::Struct(StructTag::Unit)
            | Tag::Variant { name: _, ty: StructTag::Unit } => {}

            Tag::StrDirect { len } | Tag::Bytes { len } => {
                copy::<_, _, 256>(reader.inner(), writer.inner(), Some(len)).map_err(ReadError::from)?;
            }

            Tag::Option(OptionTag::Some) | Tag::Struct(StructTag::Newtype) | Tag::Variant { name: _, ty: StructTag::Newtype } => {
                stack.push(RawValueCopyStack::Object);
            }
            Tag::Tuple { len } | Tag::Struct(StructTag::Tuple { len }) | Tag::Variant { name: _, ty: StructTag::Tuple { len }} => {
                stack.push(RawValueCopyStack::Seq {
                    remaining: Some(len),
                });
            }
            Tag::Struct(StructTag::Struct { len }) | Tag::Variant { name: _, ty: StructTag::Struct { len }} => {
                stack.push(RawValueCopyStack::Struct { remaining: len });
            }

            Tag::Array { len } => {
                stack.push(RawValueCopyStack::Seq {
                    remaining: len,
                });
            }

            Tag::Map { len } => {
                stack.push(RawValueCopyStack::Map {
                    value_next: false,
                    remaining: len,
                });
            }
        }
    }
}