use std::{
    fmt, io::{self, Read, Write}, marker::PhantomData, ops::Deref
};

use serde::{de::{DeserializeOwned, Visitor}, Deserialize, Serialize};

use crate::{
    de::{DeserializeError, Deserializer, ReadStrError, ReadTagError}, ser::SerializeError, tag::{FloatWidth, IntWidth, OptionTag, StrNewIndex, StructType, TagParameter, TypeTag}, varint, Serializer, FORMAT_VERSION
};

pub(crate) const RAW_VALUE_MAGIC_STRING: &str = "smoldata::RAW::ef812e7a46e822cd";

/// Represents serialized object bytes
pub struct RawValue(Box<[u8]>);

enum RawValueSerStack {
    SingleObject,
    Seq {
        remaining: Option<usize>,
    },
    Map {
        value_next: bool,
        remaining: Option<usize>,
        string_keys: bool,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RawValueReadingError {
    #[error("Read invalid tag {0}")]
    InvalidTag(u8),

    #[error("Read invalid string id {0}")]
    InvalidStringId(u32),

    #[error("Read invalid UTF-8 data")]
    InvalidUTF8String,

    #[error("VarInt reading error")]
    ReadVarint(
        #[from]
        #[source]
        varint::VarIntReadError,
    ),
}

impl RawValue {
    pub(crate) fn deserialize_raw<R: io::Read>(
        de: &mut Deserializer<R>,
    ) -> Result<Vec<u8>, DeserializeError> {
        let mut buf: Vec<u8> = vec![];
        let mut se = Serializer::new_bare(&mut buf, 256);
        let mut stack: Vec<RawValueSerStack> = vec![];
        let mut first = true;

        while first || !stack.is_empty() {
            first = false;

            if let Some(top) = stack.last_mut() {
                match top {
                    RawValueSerStack::SingleObject => {
                        stack.pop();
                    }
                    RawValueSerStack::Seq { remaining } => match remaining {
                        Some(0) => {
                            stack.pop();
                            continue;
                        }
                        Some(remaining) => *remaining -= 1,
                        None => {
                            if matches!(de.peek_tag()?, TypeTag::End) {
                                se.write_tag(TypeTag::End)?;
                                de.peek_tag_consume();
                                stack.pop();
                                continue;
                            }
                        }
                    },
                    RawValueSerStack::Map {
                        value_next,
                        remaining,
                        string_keys,
                    } => {
                        if !*value_next {
                            match remaining {
                                Some(0) => {
                                    stack.pop();
                                    continue;
                                }
                                Some(remaining) => *remaining -= 1,
                                None => {
                                    if matches!(de.peek_tag()?, TypeTag::End) {
                                        se.write_tag(TypeTag::End)?;
                                        de.peek_tag_consume();
                                        stack.pop();
                                        continue;
                                    }
                                }
                            }

                            if *string_keys && !matches!(de.peek_tag()?, TypeTag::Str(_)) {
                                return Err(DeserializeError::StringsOnly);
                            }

                            *value_next = true;
                        } else {
                            *value_next = false;
                        }
                    }
                };
            }

            let tag = de.read_tag()?;

            if let Some(str) = tag.get_str() {
                let str = de.read_str(str)?;
                se.write_cached_str(str, &|news| {
                    let mut tag = tag;
                    if let Some(s) = tag.get_str_mut() {
                        *s = news;
                    }
                    tag
                })?;
            } else {
                se.write_tag(tag)?;
            }

            match tag {
                TypeTag::Unit | TypeTag::Bool(_) => {}
                TypeTag::Integer {
                    width,
                    signed: _,
                    varint,
                } => {
                    if varint {
                        varint::copy_varint(&mut de.reader, &mut se.writer)?;
                    } else {
                        let mut buf = [0u8; IntWidth::MAX_BYTES];
                        let slice = &mut buf[..width.bytes()];
                        de.reader.read_exact(slice)?;
                        se.writer.write_all(slice)?;
                    }
                }
                TypeTag::Char { varint } => {
                    if varint {
                        varint::copy_varint(&mut de.reader, &mut se.writer)?;
                    } else {
                        let mut buf = [0u8; 4];
                        de.reader.read_exact(&mut buf)?;
                        se.writer.write_all(&buf)?;
                    }
                }
                TypeTag::Float(width) => {
                    let mut buf = [0u8; FloatWidth::MAX_BYTES];
                    let slice = &mut buf[..width.bytes()];
                    de.reader.read_exact(slice)?;
                    se.writer.write_all(slice)?;
                }
                TypeTag::Str(_) => {}
                TypeTag::StrDirect | TypeTag::Bytes => {
                    let len = varint::read_unsigned_varint(&mut de.reader)?;
                    varint::write_unsigned_varint(&mut se.writer, len)?;
                    copy_data::<1024, _, _>(&mut de.reader, &mut se.writer, len)?;
                }
                TypeTag::EmptyStr => {}
                TypeTag::Option(OptionTag::None) => {}
                TypeTag::Option(OptionTag::Some) => {
                    stack.push(RawValueSerStack::SingleObject);
                }
                TypeTag::Struct(StructType::Unit) => {}
                TypeTag::Struct(StructType::Newtype) => {
                    stack.push(RawValueSerStack::SingleObject);
                }
                TypeTag::Struct(StructType::Struct)
                | TypeTag::EnumVariant {
                    ty: StructType::Struct,
                    str: _,
                } => {
                    let len = varint::read_unsigned_varint(&mut de.reader)?;
                    varint::write_unsigned_varint(&mut se.writer, len)?;
                    if len > 0 {
                        stack.push(RawValueSerStack::Map {
                            remaining: Some(len),
                            string_keys: true,
                            value_next: false,
                        });
                    }
                }

                TypeTag::Struct(StructType::Tuple)
                | TypeTag::Tuple
                | TypeTag::Seq { has_length: true }
                | TypeTag::EnumVariant {
                    ty: StructType::Tuple,
                    str: _,
                } => {
                    let len = varint::read_unsigned_varint(&mut de.reader)?;
                    varint::write_unsigned_varint(&mut se.writer, len)?;
                    if len > 0 {
                        stack.push(RawValueSerStack::Seq {
                            remaining: Some(len),
                        });
                    }
                }

                TypeTag::EnumVariant {
                    ty: StructType::Unit,
                    str: _,
                } => {}
                TypeTag::EnumVariant {
                    ty: StructType::Newtype,
                    str: _,
                } => {
                    stack.push(RawValueSerStack::SingleObject);
                }
                TypeTag::Seq { has_length: false } => {
                    stack.push(RawValueSerStack::Seq { remaining: None });
                }
                TypeTag::Map { has_length } => {
                    let len = has_length
                        .then(|| varint::read_unsigned_varint(&mut de.reader))
                        .transpose()?;
                    if let Some(len) = len {
                        varint::write_unsigned_varint(&mut se.writer, len)?;
                    }
                    if len.is_none_or(|l| l > 0) {
                        stack.push(RawValueSerStack::Map {
                            remaining: len,
                            string_keys: false,
                            value_next: false,
                        });
                    }
                }
                TypeTag::End => return Err(DeserializeError::ReadEnd),
            }
        }

        Ok(buf)
    }

    pub(crate) fn serialize_raw<W: io::Write>(data: &[u8], ser: &mut Serializer<W>) -> Result<(), SerializeError> {

        let mut de = Deserializer::new_bare(io::Cursor::new(data), FORMAT_VERSION);

        loop {
            let tag = de.read_tag();

            let tag = match tag {
                Ok(tag) => tag,
                Err(ReadTagError::IOError(e)) if matches!(e.kind(), io::ErrorKind::UnexpectedEof) => {
                    break;
                },
                Err(ReadTagError::IOError(e)) => return Err(e.into()),
                Err(ReadTagError::InvalidTag(i)) => return Err(RawValueReadingError::InvalidTag(i).into()),
            };

            let mut tag_args = tag.tag_params();
            let mut write_tag = true;

            if let Some(str_ty) = tag.get_str() {
                let str = match de.read_str(str_ty) {
                    Ok(s) => s,
                    Err(ReadStrError::IOError(e)) => return Err(e.into()),
                    Err(ReadStrError::InvalidStringId(i)) => return Err(RawValueReadingError::InvalidStringId(i).into()),
                    Err(ReadStrError::InvalidUTF8String) => return Err(RawValueReadingError::InvalidUTF8String.into()),
                    Err(ReadStrError::ReadVarint(e)) => return Err(RawValueReadingError::ReadVarint(e).into()),
                };

                ser.write_cached_str(str, &|s| {
                    let mut tag = tag;
                    if let Some(str) = tag.get_str_mut() {
                        *str = s;
                    };
                    tag
                })?;

                write_tag = false;

                let skip = match str_ty {
                    StrNewIndex::New => 2,
                    StrNewIndex::Index => 1,
                };
                tag_args = &tag_args[skip..];
            }

            if write_tag {
                ser.write_tag(tag)?;
            }

            for arg in tag_args {
                match arg {
                    TagParameter::FixedIntBytes(width) => {
                        let mut buf = [0u8; IntWidth::MAX_BYTES];
                        let buf = &mut buf[..width.bytes()];
                        de.reader.read_exact(buf)?;
                        ser.writer.write_all(buf)?;
                    },
                    TagParameter::Varint => {
                        varint::copy_varint(&mut de.reader, &mut ser.writer)?;
                    },
                    TagParameter::VarintLengthPrefixedBytearray => {
                        let len = match varint::read_unsigned_varint(&mut de.reader) {
                            Ok(len) => len,
                            Err(e) => return Err(RawValueReadingError::ReadVarint(e).into()),
                        };
                        copy_data::<1024, _, _>(&mut de.reader, &mut ser.writer, len)?;
                    },
                }
            }
        }

        Ok(())
    }

    /// Warning: Data does not contain header or version info, not for storing
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// Warning: Data does not contain header or version info, not for storing
    pub fn into_bytes(self) -> Box<[u8]> {
        self.0
    }

    /// Will error on serializarion if invalid data was provided
    /// Assumes data is of the current version of the format
    pub fn from_bytes(data: Box<[u8]>) -> Self {
        Self(data)
    }

    pub fn create_deserializer(&self) -> Deserializer<io::Cursor<&'_ [u8]>> {
        let cur = io::Cursor::new(self.0.deref());
        Deserializer::new_bare(cur, FORMAT_VERSION)
    }

    pub fn deserialize_into<T: DeserializeOwned>(&self) -> Result<T, DeserializeError> {
        T::deserialize(&mut self.create_deserializer())
    }

    pub fn serialize_from<T: Serialize>(value: &T) -> Result<Self, SerializeError> {
        let mut buf = vec![];
        let mut ser = Serializer::new_bare(&mut buf, 256);
        value.serialize(&mut ser)?;
        Ok(Self(buf.into_boxed_slice()))
    }
}

impl fmt::Debug for RawValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RawValue").finish_non_exhaustive()
    }
}

impl<'de> Deserialize<'de> for RawValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_newtype_struct(RAW_VALUE_MAGIC_STRING, RawValueVisitor)
    }
}

struct RawValueVisitor;

impl Visitor<'_> for RawValueVisitor {
    type Value = RawValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("RawValue data")
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(RawValue(v.into()))
    }
}

impl Serialize for RawValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_newtype_struct(RAW_VALUE_MAGIC_STRING, &RawValueBytes(self.bytes()))
    }
}

struct RawValueBytes<'a>(&'a [u8]);

impl Serialize for RawValueBytes<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        serializer.serialize_bytes(self.0)
    }
}

pub(crate) struct RawValueSerializer<'a, W: io::Write> {
    pub ser: &'a mut Serializer<W>,
}

impl<W: io::Write> serde::Serializer for RawValueSerializer<'_, W> {
    type Ok = ();
    type Error = SerializeError;

    type SerializeSeq = SerdeSerializerStub<(), SerializeError>;
    type SerializeTuple = SerdeSerializerStub<(), SerializeError>;
    type SerializeTupleStruct = SerdeSerializerStub<(), SerializeError>;
    type SerializeTupleVariant = SerdeSerializerStub<(), SerializeError>;
    type SerializeMap = SerdeSerializerStub<(), SerializeError>;
    type SerializeStruct = SerdeSerializerStub<(), SerializeError>;
    type SerializeStructVariant = SerdeSerializerStub<(), SerializeError>;

    fn serialize_bool(self, _v: bool) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_i8(self, _v: i8) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_i16(self, _v: i16) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_i32(self, _v: i32) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_i64(self, _v: i64) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_u8(self, _v: u8) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_u16(self, _v: u16) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_u32(self, _v: u32) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_u64(self, _v: u64) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_f32(self, _v: f32) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_f64(self, _v: f64) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_char(self, _v: char) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_str(self, _v: &str) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        RawValue::serialize_raw(v, self.ser)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_some<T>(self, _value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        panic!("Invalid use of RawValueSeriazizer")
    }
}

pub(crate) struct SerdeSerializerStub<Ok, Error: serde::ser::Error>(PhantomData<(Ok, Error)>);

impl<Ok, Error: serde::ser::Error> serde::ser::SerializeTupleVariant for SerdeSerializerStub<Ok, Error> {
    type Ok = Ok;

    type Error = Error;

    fn serialize_field<T>(&mut self, _value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize {
        panic!("stub called!")
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        panic!("stub called!")
    }
}

impl<Ok, Error: serde::ser::Error> serde::ser::SerializeTupleStruct for SerdeSerializerStub<Ok, Error> {
    type Ok = Ok;

    type Error = Error;

    fn serialize_field<T>(&mut self, _value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize {
        panic!("stub called!")
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        panic!("stub called!")
    }
}

impl<Ok, Error: serde::ser::Error> serde::ser::SerializeTuple for SerdeSerializerStub<Ok, Error> {
    type Ok = Ok;

    type Error = Error;

    fn serialize_element<T>(&mut self, _value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize {
        panic!("stub called!")
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        panic!("stub called!")
    }
}

impl<Ok, Error: serde::ser::Error> serde::ser::SerializeStructVariant for SerdeSerializerStub<Ok, Error> {
    type Ok = Ok;

    type Error = Error;

    fn serialize_field<T>(&mut self, _key: &'static str, _value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize {
        panic!("stub called!")
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        panic!("stub called!")
    }
}

impl<Ok, Error: serde::ser::Error> serde::ser::SerializeStruct for SerdeSerializerStub<Ok, Error> {
    type Ok = Ok;

    type Error = Error;

    fn serialize_field<T>(&mut self, _key: &'static str, _value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize {
        panic!("stub called!")
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        panic!("stub called!")
    }
}

impl<Ok, Error: serde::ser::Error> serde::ser::SerializeMap for SerdeSerializerStub<Ok, Error> {
    type Ok = Ok;

    type Error = Error;

    fn serialize_key<T>(&mut self, _key: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize {
        panic!("stub called!")
    }

    fn serialize_value<T>(&mut self, _value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize {
        panic!("stub called!")
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        panic!("stub called!")
    }
}

impl<Ok, Error: serde::ser::Error> serde::ser::SerializeSeq for SerdeSerializerStub<Ok, Error> {
    type Ok = Ok;

    type Error = Error;

    fn serialize_element<T>(&mut self, _value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize {
        panic!("stub called!")
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        panic!("stub called!")
    }
}

fn copy_data<const BUF_SIZE: usize, S: io::Read, D: io::Write>(
    src: &mut S,
    dst: &mut D,
    mut amount: usize,
) -> Result<(), io::Error> {
    let mut buf = [0u8; BUF_SIZE];
    while amount > 0 {
        let size = amount.min(BUF_SIZE);
        let slice = &mut buf[..size];

        let read = src.read(slice)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "EOF while copying data",
            ));
        }
        let slice = &slice[..read];
        dst.write_all(slice)?;

        amount -= read;
    }

    Ok(())
}
