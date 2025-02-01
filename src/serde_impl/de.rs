use std::{collections::BTreeMap, fmt, io, ops::Deref, slice, sync::Arc};

use crate::{
    tag::{FlatTypeTag, FloatWidth, IntWidth, OptionTag, StrNewIndex, StructType, TypeTag},
    varint, FORMAT_VERSION, MAGIC_HEADER,
};

// TODO: care about what deserializer wants, not just deserializing any

#[derive(Debug, thiserror::Error)]
pub enum DeserializeError {
    #[error(transparent)]
    IOError(#[from] io::Error),

    #[error(transparent)]
    InitError(#[from] DeserializerInitError),

    #[error("Read invalid tag {0}")]
    InvalidTag(u8),

    #[error("Expected {0}, read {1:?}")]
    Expected(&'static str, FlatTypeTag),

    #[error("VarInt reading error")]
    ReadVarint(
        #[from]
        #[source]
        varint::VarIntReadError,
    ),

    #[error("Read invalid charachter")]
    InvalidChar,

    #[error("Read invalid string id {0}")]
    InvalidStringId(u32),

    #[error("Read invalid UTF-8 data")]
    InvalidUTF8String,

    #[error("Expected value, read end-of-sequence")]
    ReadEnd,

    #[error("Attempted to deserialize more data before exsausting nested deserializer")]
    DeserializerNotEnded,

    #[error("This deserializer can only deserialize strings")]
    StringsOnly,

    #[error("Tried to deserialize wrong enum type {tried:?} (got {got:?})")]
    WrongEnumVariantType { tried: StructType, got: StructType },

    #[error("Attempted to deserialize map key but got value")]
    TriedKeyGotValue,

    #[error("Attempted to deserialize map value but got key")]
    TriedValedGotKey,

    #[error("{0}")]
    Custom(String),
}

impl serde::de::Error for DeserializeError {
    fn custom<T>(msg: T) -> Self
    where
        T: fmt::Display,
    {
        Self::Custom(msg.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeserializerInitError {
    #[error(transparent)]
    IOError(#[from] io::Error),

    #[error("Read invalid magic value")]
    InvalidHeader,

    #[error("Unsupported format version {0}")]
    UnsupportedVersion(u8),
}

#[derive(Debug, thiserror::Error)]
pub enum ReadTagError {
    #[error(transparent)]
    IOError(#[from] io::Error),

    #[error("Read invalid tag {0}")]
    InvalidTag(u8),
}

impl From<ReadTagError> for DeserializeError {
    fn from(val: ReadTagError) -> Self {
        match val {
            ReadTagError::IOError(error) => Self::IOError(error),
            ReadTagError::InvalidTag(i) => Self::InvalidTag(i),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReadStrError {
    #[error(transparent)]
    IOError(#[from] io::Error),

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

impl From<ReadStrError> for DeserializeError {
    fn from(val: ReadStrError) -> Self {
        match val {
            ReadStrError::IOError(error) => Self::IOError(error),
            ReadStrError::InvalidStringId(i) => Self::InvalidStringId(i),
            ReadStrError::InvalidUTF8String => Self::InvalidUTF8String,
            ReadStrError::ReadVarint(v) => Self::ReadVarint(v)
        }
    }
}

pub struct Deserializer<R: io::Read> {
    pub(crate) reader: R,
    pub(crate) string_map: BTreeMap<u32, Arc<str>>,
    tag_peek: Option<TypeTag>,
    level: usize,

    #[allow(unused)]
    data_version: u8,
}

impl<R: io::Read> Deserializer<R> {
    /// Construct a new Deserializer.<br>
    /// Reader preferred to be buffered, deserialization does many small reads
    pub fn new(mut reader: R) -> Result<Self, DeserializerInitError> {
        if !read_check_eq(&mut reader, MAGIC_HEADER)? {
            return Err(DeserializerInitError::InvalidHeader);
        }

        let mut ver = 0u8;
        reader.read_exact(slice::from_mut(&mut ver))?;

        if ver > FORMAT_VERSION {
            return Err(DeserializerInitError::UnsupportedVersion(ver));
        }

        Ok(Self::new_bare(reader, ver))
    }

    pub(crate) fn new_bare(reader: R, data_version: u8) -> Self {
        Self {
            reader,
            string_map: Default::default(),
            tag_peek: None,
            level: 0,
            data_version,
        }
    }

    pub(crate) fn read_tag(&mut self) -> Result<TypeTag, ReadTagError> {
        if let Some(tag) = self.tag_peek.take() {
            return Ok(tag);
        }

        let mut byte = 0u8;
        self.reader.read_exact(slice::from_mut(&mut byte))?;
        FlatTypeTag::try_from(byte)
            .map(Into::into)
            .map_err(ReadTagError::InvalidTag)
    }

    pub(crate) fn peek_tag(&mut self) -> Result<TypeTag, ReadTagError> {
        if let Some(tag) = self.tag_peek {
            return Ok(tag);
        }

        let mut byte = 0u8;
        self.reader.read_exact(slice::from_mut(&mut byte))?;
        let tag = FlatTypeTag::try_from(byte)
            .map(Into::into)
            .map_err(ReadTagError::InvalidTag)?;
        self.tag_peek = Some(tag);
        Ok(tag)
    }

    pub(crate) fn peek_tag_consume(&mut self) -> Option<TypeTag> {
        self.tag_peek.take()
    }

    pub(crate) fn read_str_by_index(&mut self) -> Result<Arc<str>, ReadStrError> {
        let index = varint::read_unsigned_varint(&mut self.reader)?;
        let str = self
            .string_map
            .get(&index)
            .ok_or(ReadStrError::InvalidStringId(index))?;
        Ok(str.clone())
    }

    pub(crate) fn read_str_new(&mut self) -> Result<Arc<str>, ReadStrError> {
        let index = varint::read_unsigned_varint(&mut self.reader)?;
        let len = varint::read_unsigned_varint(&mut self.reader)?;
        let mut data = vec![0u8; len];
        self.reader.read_exact(&mut data)?;
        let string = String::from_utf8(data).map_err(|_| ReadStrError::InvalidUTF8String)?;

        let boxed = self.string_map.entry(index).or_default();
        *boxed = string.into();

        Ok(boxed.clone())
    }

    pub(crate) fn read_str(&mut self, ty: StrNewIndex) -> Result<Arc<str>, ReadStrError> {
        match ty {
            StrNewIndex::New => self.read_str_new(),
            StrNewIndex::Index => self.read_str_by_index(),
        }
    }

    fn visit_enum<'de, V: serde::de::Visitor<'de>>(
        &mut self,
        visitor: V,
        ty: StructType,
        str: StrNewIndex,
    ) -> Result<V::Value, DeserializeError> {
        self.level += 1;
        let access = EnumAccess {
            level: self.level,
            de: self,
            ty,
            str_ty: str,
        };

        visitor.visit_enum(access)
    }

    fn visit_map<'de, V: serde::de::Visitor<'de>>(
        &mut self,
        visitor: V,
        len: Option<usize>,
        string_keys: bool,
    ) -> Result<V::Value, DeserializeError> {
        self.level += 1;
        let map = MapAccess {
            level: self.level,
            de: self,
            string_keys,
            next_value: false,
            remaining: len,
            done: false,
        };

        visitor.visit_map(map)
    }
}

impl<'de, R: io::Read> serde::Deserializer<'de> for &mut Deserializer<R> {
    type Error = DeserializeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        let tag = self.read_tag()?;

        match tag {
            TypeTag::Unit => visitor.visit_unit(),
            TypeTag::Bool(b) => visitor.visit_bool(b),
            TypeTag::Integer {
                width: IntWidth::W8,
                signed,
                varint: false,
            } => {
                let mut buf = [0u8; 1];
                self.reader.read_exact(&mut buf)?;
                if signed {
                    visitor.visit_i8(buf[0] as i8)
                } else {
                    visitor.visit_u8(buf[0])
                }
            }
            TypeTag::Integer {
                width: IntWidth::W16,
                signed,
                varint: false,
            } => {
                let mut buf = [0u8; 2];
                self.reader.read_exact(&mut buf)?;
                if signed {
                    visitor.visit_i16(i16::from_le_bytes(buf))
                } else {
                    visitor.visit_u16(u16::from_le_bytes(buf))
                }
            }
            TypeTag::Integer {
                width: IntWidth::W32,
                signed,
                varint: false,
            } => {
                let mut buf = [0u8; 4];
                self.reader.read_exact(&mut buf)?;
                if signed {
                    visitor.visit_i32(i32::from_le_bytes(buf))
                } else {
                    visitor.visit_u32(u32::from_le_bytes(buf))
                }
            }
            TypeTag::Integer {
                width: IntWidth::W64,
                signed,
                varint: false,
            } => {
                let mut buf = [0u8; 8];
                self.reader.read_exact(&mut buf)?;
                if signed {
                    visitor.visit_i64(i64::from_le_bytes(buf))
                } else {
                    visitor.visit_u64(u64::from_le_bytes(buf))
                }
            }
            TypeTag::Integer {
                width: IntWidth::W128,
                signed,
                varint: false,
            } => {
                let mut buf = [0u8; 16];
                self.reader.read_exact(&mut buf)?;
                if signed {
                    visitor.visit_i128(i128::from_le_bytes(buf))
                } else {
                    visitor.visit_u128(u128::from_le_bytes(buf))
                }
            }
            TypeTag::Integer {
                width: IntWidth::W8,
                signed: false,
                varint: true,
            } => visitor.visit_u8(varint::read_unsigned_varint(&mut self.reader)?),
            TypeTag::Integer {
                width: IntWidth::W16,
                signed: false,
                varint: true,
            } => visitor.visit_u16(varint::read_unsigned_varint(&mut self.reader)?),
            TypeTag::Integer {
                width: IntWidth::W32,
                signed: false,
                varint: true,
            } => visitor.visit_u32(varint::read_unsigned_varint(&mut self.reader)?),
            TypeTag::Integer {
                width: IntWidth::W64,
                signed: false,
                varint: true,
            } => visitor.visit_u64(varint::read_unsigned_varint(&mut self.reader)?),
            TypeTag::Integer {
                width: IntWidth::W128,
                signed: false,
                varint: true,
            } => visitor.visit_u128(varint::read_unsigned_varint(&mut self.reader)?),
            TypeTag::Integer {
                width: IntWidth::W8,
                signed: true,
                varint: true,
            } => visitor.visit_i8(varint::read_signed_varint(&mut self.reader)?),
            TypeTag::Integer {
                width: IntWidth::W16,
                signed: true,
                varint: true,
            } => visitor.visit_i16(varint::read_signed_varint(&mut self.reader)?),
            TypeTag::Integer {
                width: IntWidth::W32,
                signed: true,
                varint: true,
            } => visitor.visit_i32(varint::read_signed_varint(&mut self.reader)?),
            TypeTag::Integer {
                width: IntWidth::W64,
                signed: true,
                varint: true,
            } => visitor.visit_i64(varint::read_signed_varint(&mut self.reader)?),
            TypeTag::Integer {
                width: IntWidth::W128,
                signed: true,
                varint: true,
            } => visitor.visit_i128(varint::read_signed_varint(&mut self.reader)?),
            TypeTag::Char { varint: false } => {
                let mut buf = [0u8; 4];
                self.reader.read_exact(&mut buf)?;
                let char =
                    char::from_u32(u32::from_le_bytes(buf)).ok_or(DeserializeError::InvalidChar)?;
                visitor.visit_char(char)
            }
            TypeTag::Char { varint: true } => {
                let val = varint::read_unsigned_varint(&mut self.reader)?;
                let char = char::from_u32(val).ok_or(DeserializeError::InvalidChar)?;
                visitor.visit_char(char)
            }
            TypeTag::Float(FloatWidth::F32) => {
                let mut buf = [0u8; 4];
                self.reader.read_exact(&mut buf)?;
                visitor.visit_f32(f32::from_le_bytes(buf))
            },
            TypeTag::Float(FloatWidth::F64) => {
                let mut buf = [0u8; 8];
                self.reader.read_exact(&mut buf)?;
                visitor.visit_f64(f64::from_le_bytes(buf))
            },
            TypeTag::Str(sni) => {
                visitor.visit_str(&self.read_str(sni)?)
            },
            TypeTag::StrDirect => {
                let len = varint::read_unsigned_varint(&mut self.reader)?;
                let mut data = vec![0u8; len];
                self.reader.read_exact(&mut data)?;
                let string =
                    String::from_utf8(data).map_err(|_| DeserializeError::InvalidUTF8String)?;
                visitor.visit_string(string)
            },
            TypeTag::EmptyStr => visitor.visit_str(""),
            TypeTag::Bytes => {
                let len = varint::read_unsigned_varint(&mut self.reader)?;
                let mut data = vec![0u8; len];
                self.reader.read_exact(&mut data)?;
                visitor.visit_byte_buf(data)
            },
            TypeTag::Option(OptionTag::None) => visitor.visit_none(),
            TypeTag::Option(OptionTag::Some) => visitor.visit_some(self),
            TypeTag::Struct(StructType::Unit) => visitor.visit_unit(),
            TypeTag::Struct(StructType::Newtype) => visitor.visit_newtype_struct(self),

            TypeTag::Struct(StructType::Struct) => {
                let len = varint::read_unsigned_varint(&mut self.reader)?;
                self.visit_map(visitor, Some(len), true)
            },

            TypeTag::EnumVariant { ty, str } => self.visit_enum(visitor, ty, str),
            TypeTag::Seq { has_length: false } => {
                self.level += 1;
                let seq = SeqAccess {
                    remaining: None,
                    level: self.level,
                    de: self,
                    done: false,
                };
                visitor.visit_seq(seq)
            },

            TypeTag::Seq { has_length: true } | TypeTag::Tuple | TypeTag::Struct(StructType::Tuple) => {
                let len = varint::read_unsigned_varint(&mut self.reader)?;
                self.level += 1;
                let seq = SeqAccess {
                    remaining: Some(len),
                    level: self.level,
                    de: self,
                    done: false,
                };
                visitor.visit_seq(seq)
            }

            TypeTag::Map { has_length } => {
                let len = has_length.then(|| varint::read_unsigned_varint(&mut self.reader)).transpose()?;
                self.visit_map(visitor, len, false)
            },
            TypeTag::End => Err(DeserializeError::ReadEnd),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        if name == crate::raw::RAW_VALUE_MAGIC_STRING {
            let buf = crate::raw::RawValue::deserialize_raw(self)?;
            return visitor.visit_bytes(&buf);
        }
        self.deserialize_any(visitor)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }
}

struct SeqAccess<'a, R: io::Read> {
    remaining: Option<usize>,
    de: &'a mut Deserializer<R>,
    done: bool,
    level: usize,
}

impl<'de, R: io::Read> serde::de::SeqAccess<'de> for SeqAccess<'_, R> {
    type Error = DeserializeError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: serde::de::DeserializeSeed<'de>,
    {
        if self.done {
            return Ok(None);
        }

        if self.level != self.de.level {
            return Err(DeserializeError::DeserializerNotEnded);
        }

        match self.remaining {
            Some(rem) => {
                if rem == 0 {
                    self.done = true;
                    self.de.level -= 1;
                    return Ok(None);
                }
            }
            None => {
                let next_tag = self.de.peek_tag()?;
                if matches!(next_tag, TypeTag::End) {
                    self.done = true;
                    self.de.level -= 1;
                    self.de.peek_tag_consume();
                    return Ok(None);
                }
            }
        }

        let ret = seed.deserialize(&mut *self.de)?;

        match &mut self.remaining {
            Some(rem) => {
                *rem -= 1;
                if *rem == 0 {
                    self.done = true;
                    self.de.level -= 1;
                }
            }
            None => {
                let next_tag = self.de.peek_tag()?;
                if matches!(next_tag, TypeTag::End) {
                    self.done = true;
                    self.de.level -= 1;
                    self.de.peek_tag_consume();
                }
            }
        }

        Ok(Some(ret))
    }

    fn size_hint(&self) -> Option<usize> {
        self.remaining
    }
}

struct EnumAccess<'a, R: io::Read> {
    de: &'a mut Deserializer<R>,
    level: usize,

    ty: StructType,
    str_ty: StrNewIndex,
}

impl<'de, 'a, R: io::Read> serde::de::EnumAccess<'de> for EnumAccess<'a, R> {
    type Error = DeserializeError;

    type Variant = VariantAccess<'a, R>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: serde::de::DeserializeSeed<'de>,
    {
        let ident = seed.deserialize(StringDeserializer {
            de: self.de,
            str_ty: Some(self.str_ty),
        })?;

        let access = VariantAccess {
            de: self.de,
            level: self.level,
            ty: self.ty,
        };

        Ok((ident, access))
    }
}

struct VariantAccess<'a, R: io::Read> {
    de: &'a mut Deserializer<R>,
    level: usize,

    ty: StructType,
}

impl<R: io::Read> VariantAccess<'_, R> {
    fn assert_type(&self, ty: StructType) -> Result<(), DeserializeError> {
        if self.ty != ty {
            Err(DeserializeError::WrongEnumVariantType {
                tried: ty,
                got: self.ty,
            })
        } else {
            Ok(())
        }
    }
}

impl<'de, R: io::Read> serde::de::VariantAccess<'de> for VariantAccess<'_, R> {
    type Error = DeserializeError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        self.assert_type(StructType::Unit)?;
        self.de.level -= 1;
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: serde::de::DeserializeSeed<'de>,
    {
        self.assert_type(StructType::Newtype)?;
        let val = seed.deserialize(&mut *self.de);
        self.de.level -= 1;
        val
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.assert_type(StructType::Tuple)?;
        let len = varint::read_unsigned_varint(&mut self.de.reader)?;
        let seq = SeqAccess {
            remaining: Some(len),
            level: self.level,
            de: self.de,
            done: false,
        };
        visitor.visit_seq(seq)
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        let len = varint::read_unsigned_varint(&mut self.de.reader)?;
        let map = MapAccess {
            de: self.de,
            level: self.level,
            string_keys: true,
            next_value: false,
            remaining: Some(len),
            done: false,
        };

        visitor.visit_map(map)
    }
}

struct StringDeserializer<'a, R: io::Read> {
    de: &'a mut Deserializer<R>,

    /// Deserialize a specific string on Some, or read a string tag and operate on that on None
    str_ty: Option<StrNewIndex>,
}

impl<R: io::Read> StringDeserializer<'_, R> {
    fn read_str(self) -> Result<Arc<str>, DeserializeError> {
        match self.str_ty {
            Some(s) => self.de.read_str(s).map_err(Into::into),
            None => {
                let tag = self.de.read_tag()?;
                match tag {
                    TypeTag::Str(s) => self.de.read_str(s).map_err(Into::into),
                    _ => Err(DeserializeError::Expected("str", tag.into())),
                }
            }
        }
    }
}

impl<'de, R: io::Read> serde::de::Deserializer<'de> for StringDeserializer<'_, R> {
    type Error = DeserializeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bool<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_i8<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_i16<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_i32<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_i64<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_u8<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_u16<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_u32<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_u64<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_f32<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_f64<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_char<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        visitor.visit_str(&self.read_str()?)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        visitor.visit_string(self.read_str()?.deref().into())
    }

    fn deserialize_bytes<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_byte_buf<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_option<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_unit<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_seq<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_tuple<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_map<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        Err(DeserializeError::StringsOnly)
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }
}

struct MapAccess<'a, R: io::Read> {
    de: &'a mut Deserializer<R>,
    level: usize,

    string_keys: bool,
    next_value: bool,
    remaining: Option<usize>,
    done: bool,
}

impl<'de, R: io::Read> serde::de::MapAccess<'de> for MapAccess<'_, R> {
    type Error = DeserializeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: serde::de::DeserializeSeed<'de>,
    {
        if self.done {
            return Ok(None);
        }

        if self.next_value {
            return Err(DeserializeError::TriedKeyGotValue);
        }

        if self.level != self.de.level {
            return Err(DeserializeError::DeserializerNotEnded);
        }

        match self.remaining {
            Some(rem) => {
                if rem == 0 {
                    self.done = true;
                    self.de.level -= 1;
                    return Ok(None);
                }
            }
            None => {
                let next_tag = self.de.peek_tag()?;
                if matches!(next_tag, TypeTag::End) {
                    self.done = true;
                    self.de.level -= 1;
                    self.de.peek_tag_consume();
                    return Ok(None);
                }
            }
        }

        let ret = if self.string_keys {
            let de = StringDeserializer {
                de: self.de,
                str_ty: None,
            };
            seed.deserialize(de)?
        } else {
            seed.deserialize(&mut *self.de)?
        };

        self.next_value = true;

        match &mut self.remaining {
            Some(rem) => {
                *rem -= 1;
                if *rem == 0 {
                    self.done = true;
                }
            }
            None => {
                let next_tag = self.de.peek_tag()?;
                if matches!(next_tag, TypeTag::End) {
                    self.done = true;
                    self.de.peek_tag_consume();
                }
            }
        }

        Ok(Some(ret))
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::DeserializeSeed<'de>,
    {
        if !self.next_value {
            return Err(DeserializeError::TriedValedGotKey);
        }
        let res = seed.deserialize(&mut *self.de)?;
        self.next_value = false;

        if self.done {
            self.de.level -= 1;
        }

        Ok(res)
    }
}

fn read_check_eq<R: io::Read>(mut reader: R, mut data: &[u8]) -> Result<bool, io::Error> {
    let mut buf = [0u8; 256];

    // read full length of data
    let mut res = true;

    let buf_len = buf.len();

    while !data.is_empty() {
        let buf = &mut buf[..data.len().min(buf_len)];

        reader.read_exact(buf)?;

        if buf != &data[..buf.len()] {
            res = false;
        }

        data = &data[buf.len()..];
    }

    Ok(res)
}
