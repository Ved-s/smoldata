use std::{collections::HashMap, error::Error, fmt::Display, io, sync::Arc, ops::Deref};

use crate::{
    raw::RawValueReadingError, tag::{FlatTypeTag, FloatWidth, IntWidth, OptionTag, StrNewIndex, StructType, TypeTag}, varint, MaybeArcStr, FORMAT_VERSION, MAGIC_HEADER
};

const SERIALIZER_DEBUG_PRINT: bool = false;

macro_rules! serializer_debugprintln {
    ($self:ident, $($t:tt)*) => {
        if SERIALIZER_DEBUG_PRINT {
            for _ in 0..$self.level {
                print!("  ");
            }
            println!($($t)*);
        }
    };
}

#[derive(Debug, thiserror::Error)]
pub enum SerializeError {
    #[error(transparent)]
    IOError(#[from] io::Error),

    #[error("Attempted to serialize more elements than promised")]
    MoreElementsThanPromised,

    #[error("Attempted to serialize less elements than promised")]
    LessElementsThanPromised,

    #[error("Attempted to serialize more data before ending nested serializer")]
    SerializerNotProperlyEnded,

    #[error("Attempted to serialize map key when expected value")]
    ValueExpectedGotKey,

    #[error("Attempted to serialize map value when expected key")]
    KeyExpectedGotValue,

    #[error("Error while reading a RawValue")]
    RawValueReading(#[from] RawValueReadingError),

    #[error(transparent)]
    Custom(Box<dyn Error>),
}

impl serde::ser::Error for SerializeError {
    fn custom<T>(msg: T) -> Self
    where
        T: Display,
    {
        Self::Custom(msg.to_string().into())
    }
}

pub struct Serializer<W: io::Write> {
    pub(crate) writer: W,
    pub(crate) string_map: HashMap<Arc<str>, u32>,
    level: usize,

    next_map_index: u32,
    max_cache_str_len: usize,
}

impl<W: io::Write> Serializer<W> {
    /// Construct a new Serializer.<br>
    /// Writer preferred to be buffered, serialization does many small writes
    pub fn new(mut writer: W, max_cache_str_len: usize) -> Result<Self, io::Error> {
        writer.write_all(MAGIC_HEADER)?;
        writer.write_all(&[FORMAT_VERSION])?;

        let this = Self::new_bare(writer, max_cache_str_len);
        serializer_debugprintln!(
            this,
            " -- Serializer debug log --\nversion: {FORMAT_VERSION}"
        );

        Ok(this)
    }

    pub(crate) fn new_bare(writer: W, max_cache_str_len: usize) -> Self {
        Self {
            writer,
            string_map: Default::default(),
            level: 0,

            next_map_index: 0,
            max_cache_str_len,
        }
    }

    pub(crate) fn write_tag(&mut self, tag: impl Into<FlatTypeTag>) -> Result<(), io::Error> {
        let tag = tag.into();
        serializer_debugprintln!(self, "tag: {tag:?}");
        self.writer.write_all(&[tag.into()])
    }

    pub(crate) fn write_cached_str<'a>(
        &mut self,
        s: impl Into<MaybeArcStr<'a>>,
        tagmaker: &dyn Fn(StrNewIndex) -> TypeTag,
    ) -> Result<(), io::Error> {
        let s = s.into();
        if let Some(index) = self.string_map.get(s.deref()).copied() {
            self.write_tag(tagmaker(StrNewIndex::Index))?;
            serializer_debugprintln!(self, "index: {index} (\"{}\")", s.deref());
            varint::write_unsigned_varint(&mut self.writer, index)?;
        } else {
            let index = self.next_map_index;

            self.write_tag(tagmaker(StrNewIndex::New))?;
            varint::write_unsigned_varint(&mut self.writer, index)?;
            varint::write_unsigned_varint(&mut self.writer, s.len())?;
            self.writer.write_all(s.as_bytes())?;

            serializer_debugprintln!(self, "string: {index} (\"{}\")", s.deref());

            self.next_map_index += 1;
            self.string_map.insert(s.into(), index);
        }
        Ok(())
    }
}

impl<'a, W: io::Write> serde::Serializer for &'a mut Serializer<W> {
    type Ok = ();

    type Error = SerializeError;

    type SerializeSeq = SerializeSeq<'a, W>;

    type SerializeTuple = SerializeTuple<'a, W>;

    type SerializeTupleStruct = SerializeTupleStruct<'a, W>;

    type SerializeTupleVariant = SerializeTupleVariant<'a, W>;

    type SerializeMap = SerializeMap<'a, W>;

    type SerializeStruct = SerializeStruct<'a, W>;

    type SerializeStructVariant = SerializeStructVariant<'a, W>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::Bool(v))?;

        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::Integer {
            width: IntWidth::W8,
            signed: false,
            varint: false,
        })?;
        self.writer.write_all(&[v as u8])?;

        serializer_debugprintln!(self, "i8: {v}");

        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        let varint = is_varint_better(v.unsigned_abs().leading_zeros(), 2, true);
        self.write_tag(TypeTag::Integer {
            width: IntWidth::W16,
            signed: true,
            varint,
        })?;
        if varint {
            varint::write_signed_varint(&mut self.writer, v)?;
        } else {
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "i16: {v}");
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        let varint = is_varint_better(v.unsigned_abs().leading_zeros(), 4, true);
        self.write_tag(TypeTag::Integer {
            width: IntWidth::W32,
            signed: true,
            varint,
        })?;
        if varint {
            varint::write_signed_varint(&mut self.writer, v)?;
        } else {
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "i32: {v}");
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        let varint = is_varint_better(v.unsigned_abs().leading_zeros(), 8, true);
        self.write_tag(TypeTag::Integer {
            width: IntWidth::W64,
            signed: true,
            varint,
        })?;
        if varint {
            varint::write_signed_varint(&mut self.writer, v)?;
        } else {
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "i64: {v}");
        Ok(())
    }

    fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
        let varint = is_varint_better(v.unsigned_abs().leading_zeros(), 16, true);
        self.write_tag(TypeTag::Integer {
            width: IntWidth::W128,
            signed: true,
            varint,
        })?;
        if varint {
            varint::write_signed_varint(&mut self.writer, v)?;
        } else {
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "i128: {v}");
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::Integer {
            width: IntWidth::W8,
            signed: false,
            varint: false,
        })?;
        self.writer.write_all(&[v])?;

        serializer_debugprintln!(self, "u8: {v}");

        Ok(())
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        let varint = is_varint_better(v.leading_zeros(), 2, false);
        self.write_tag(TypeTag::Integer {
            width: IntWidth::W16,
            signed: false,
            varint,
        })?;
        if varint {
            varint::write_unsigned_varint(&mut self.writer, v)?;
        } else {
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "u16: {v}");
        Ok(())
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        let varint = is_varint_better(v.leading_zeros(), 4, false);
        self.write_tag(TypeTag::Integer {
            width: IntWidth::W32,
            signed: false,
            varint,
        })?;
        if varint {
            varint::write_unsigned_varint(&mut self.writer, v)?;
        } else {
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "u32: {v}");
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        let varint = is_varint_better(v.leading_zeros(), 8, false);
        self.write_tag(TypeTag::Integer {
            width: IntWidth::W64,
            signed: false,
            varint,
        })?;
        if varint {
            varint::write_unsigned_varint(&mut self.writer, v)?;
        } else {
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "u64: {v}");
        Ok(())
    }

    fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
        let varint = is_varint_better(v.leading_zeros(), 16, false);
        self.write_tag(TypeTag::Integer {
            width: IntWidth::W128,
            signed: false,
            varint,
        })?;
        if varint {
            varint::write_unsigned_varint(&mut self.writer, v)?;
        } else {
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "u128: {v}");
        Ok(())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::Float(FloatWidth::F32))?;
        self.writer.write_all(&v.to_le_bytes())?;

        serializer_debugprintln!(self, "f32: {v}");

        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::Float(FloatWidth::F64))?;
        self.writer.write_all(&v.to_le_bytes())?;

        serializer_debugprintln!(self, "f64: {v}");

        Ok(())
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        let v = v as u32;

        let varint = is_varint_better(v.leading_zeros(), 4, true);
        self.write_tag(TypeTag::Char { varint })?;

        if varint {
            varint::write_unsigned_varint(&mut self.writer, v)?;
        } else {
            self.writer.write_all(&v.to_le_bytes())?;
        }

        serializer_debugprintln!(self, "char: {v:?}");
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        if v.is_empty() {
            self.write_tag(TypeTag::EmptyStr)?;
        } else if v.len() > self.max_cache_str_len {
            self.write_tag(TypeTag::StrDirect)?;
            varint::write_unsigned_varint(&mut self.writer, v.len())?;
            self.writer.write_all(v.as_bytes())?;
            serializer_debugprintln!(self, "string: \"{v}\"");
        } else {
            self.write_cached_str(v, &|s| TypeTag::Str(s))?;
        }

        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::Bytes)?;
        varint::write_unsigned_varint(&mut self.writer, v.len())?;
        self.writer.write_all(v)?;

        serializer_debugprintln!(self, "bytes: {v:?}");

        Ok(())
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::Option(OptionTag::None))?;

        Ok(())
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        self.write_tag(TypeTag::Option(OptionTag::Some))?;
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::Unit)?;

        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::Struct(StructType::Unit))?;

        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.write_cached_str(variant, &|str| TypeTag::EnumVariant {
            ty: StructType::Unit,
            str,
        })?;

        Ok(())
    }

    fn serialize_newtype_struct<T>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {

        if name == crate::raw::RAW_VALUE_MAGIC_STRING {
            let ser = crate::raw::RawValueSerializer {
                ser: self,
            };
            return value.serialize(ser);
        }

        self.write_tag(TypeTag::Struct(StructType::Newtype))?;
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        self.write_cached_str(variant, &|str| TypeTag::EnumVariant {
            ty: StructType::Newtype,
            str,
        })?;
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        self.write_tag(TypeTag::Seq {
            has_length: len.is_some(),
        })?;
        if let Some(len) = len {
            serializer_debugprintln!(self, "len: {len}");
            varint::write_unsigned_varint(&mut self.writer, len)?;
        }
        self.level += 1;
        Ok(SerializeSeq {
            level: self.level,
            ser: self,
            remaining: len,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.write_tag(TypeTag::Tuple)?;
        varint::write_unsigned_varint(&mut self.writer, len)?;
        serializer_debugprintln!(self, "len: {len}");
        self.level += 1;
        Ok(SerializeTuple {
            level: self.level,
            ser: self,
            remaining: len,
        })
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.write_tag(TypeTag::Struct(StructType::Tuple))?;
        varint::write_unsigned_varint(&mut self.writer, len)?;
        serializer_debugprintln!(self, "len: {len}");
        self.level += 1;
        Ok(SerializeTupleStruct {
            level: self.level,
            ser: self,
            remaining: len,
        })
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.write_cached_str(variant, &|str| TypeTag::EnumVariant {
            ty: StructType::Tuple,
            str,
        })?;
        varint::write_unsigned_varint(&mut self.writer, len)?;
        serializer_debugprintln!(self, "len: {len}");
        self.level += 1;
        Ok(SerializeTupleVariant {
            level: self.level,
            ser: self,
            remaining: len,
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        self.write_tag(TypeTag::Map {
            has_length: len.is_some(),
        })?;
        if let Some(len) = len {
            serializer_debugprintln!(self, "len: {len}");
            varint::write_unsigned_varint(&mut self.writer, len)?;
        }

        self.level += 1;
        Ok(SerializeMap {
            level: self.level,
            ser: self,
            remaining: len,
            value_next: false,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        self.write_tag(TypeTag::Struct(StructType::Struct))?;
        varint::write_unsigned_varint(&mut self.writer, len)?;
        serializer_debugprintln!(self, "len: {len}");

        self.level += 1;
        Ok(SerializeStruct {
            level: self.level,
            ser: self,
            remaining: len,
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.write_cached_str(variant, &|str| TypeTag::EnumVariant {
            ty: StructType::Struct,
            str,
        })?;
        varint::write_unsigned_varint(&mut self.writer, len)?;
        serializer_debugprintln!(self, "len: {len}");

        self.level += 1;
        Ok(SerializeStructVariant {
            level: self.level,
            ser: self,
            remaining: len,
        })
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

pub struct SerializeSeq<'a, W: io::Write> {
    ser: &'a mut Serializer<W>,
    remaining: Option<usize>,
    level: usize,
}

impl<W: io::Write> serde::ser::SerializeSeq for SerializeSeq<'_, W> {
    type Ok = ();

    type Error = SerializeError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        if self.level != self.ser.level {
            return Err(SerializeError::SerializerNotProperlyEnded);
        }

        if let Some(rem) = &mut self.remaining {
            if *rem == 0 {
                return Err(SerializeError::MoreElementsThanPromised);
            }
            *rem -= 1;
        }

        value.serialize(&mut *self.ser)?;

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.remaining.is_some_and(|rem| rem != 0) {
            return Err(SerializeError::LessElementsThanPromised);
        }
        if self.remaining.is_none() {
            self.ser.write_tag(TypeTag::End)?;
        }

        self.ser.level -= 1;

        Ok(())
    }
}

pub struct SerializeTuple<'a, W: io::Write> {
    ser: &'a mut Serializer<W>,
    remaining: usize,
    level: usize,
}

impl<W: io::Write> serde::ser::SerializeTuple for SerializeTuple<'_, W> {
    type Ok = ();

    type Error = SerializeError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        if self.level != self.ser.level {
            return Err(SerializeError::SerializerNotProperlyEnded);
        }

        if self.remaining == 0 {
            return Err(SerializeError::MoreElementsThanPromised);
        }

        self.remaining -= 1;

        value.serialize(&mut *self.ser)?;

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.remaining != 0 {
            return Err(SerializeError::LessElementsThanPromised);
        }

        self.ser.level -= 1;

        Ok(())
    }
}

pub struct SerializeTupleStruct<'a, W: io::Write> {
    ser: &'a mut Serializer<W>,
    remaining: usize,
    level: usize,
}

impl<W: io::Write> serde::ser::SerializeTupleStruct for SerializeTupleStruct<'_, W> {
    type Ok = ();

    type Error = SerializeError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        if self.level != self.ser.level {
            return Err(SerializeError::SerializerNotProperlyEnded);
        }

        if self.remaining == 0 {
            return Err(SerializeError::MoreElementsThanPromised);
        }

        self.remaining -= 1;

        value.serialize(&mut *self.ser)?;

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.remaining != 0 {
            return Err(SerializeError::LessElementsThanPromised);
        }

        self.ser.level -= 1;

        Ok(())
    }
}

pub struct SerializeTupleVariant<'a, W: io::Write> {
    ser: &'a mut Serializer<W>,
    remaining: usize,
    level: usize,
}

impl<W: io::Write> serde::ser::SerializeTupleVariant for SerializeTupleVariant<'_, W> {
    type Ok = ();

    type Error = SerializeError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        if self.level != self.ser.level {
            return Err(SerializeError::SerializerNotProperlyEnded);
        }

        if self.remaining == 0 {
            return Err(SerializeError::MoreElementsThanPromised);
        }

        self.remaining -= 1;

        value.serialize(&mut *self.ser)?;

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.remaining != 0 {
            return Err(SerializeError::LessElementsThanPromised);
        }

        self.ser.level -= 1;

        Ok(())
    }
}

pub struct SerializeMap<'a, W: io::Write> {
    ser: &'a mut Serializer<W>,
    remaining: Option<usize>,
    level: usize,

    value_next: bool,
}

impl<W: io::Write> serde::ser::SerializeMap for SerializeMap<'_, W> {
    type Ok = ();

    type Error = SerializeError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        if self.level != self.ser.level {
            return Err(SerializeError::SerializerNotProperlyEnded);
        }

        if self.value_next {
            return Err(SerializeError::ValueExpectedGotKey);
        }

        if let Some(rem) = &mut self.remaining {
            if *rem == 0 {
                return Err(SerializeError::MoreElementsThanPromised);
            }
            *rem -= 1;
        }

        self.value_next = true;

        key.serialize(&mut *self.ser)?;

        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        if self.level != self.ser.level {
            return Err(SerializeError::SerializerNotProperlyEnded);
        }

        if !self.value_next {
            return Err(SerializeError::KeyExpectedGotValue);
        }

        self.value_next = false;

        value.serialize(&mut *self.ser)?;

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.remaining.is_some_and(|rem| rem != 0) {
            return Err(SerializeError::LessElementsThanPromised);
        }
        if self.remaining.is_none() {
            self.ser.write_tag(TypeTag::End)?;
        }

        self.ser.level -= 1;

        Ok(())
    }
}

pub struct SerializeStruct<'a, W: io::Write> {
    ser: &'a mut Serializer<W>,
    remaining: usize,
    level: usize,
}

impl<W: io::Write> serde::ser::SerializeStruct for SerializeStruct<'_, W> {
    type Ok = ();

    type Error = SerializeError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        if self.level != self.ser.level {
            return Err(SerializeError::SerializerNotProperlyEnded);
        }

        if self.remaining == 0 {
            return Err(SerializeError::MoreElementsThanPromised);
        }

        self.remaining -= 1;

        self.ser.write_cached_str(key, &TypeTag::Str)?;
        value.serialize(&mut *self.ser)?;

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.remaining != 0 {
            return Err(SerializeError::LessElementsThanPromised);
        }

        self.ser.level -= 1;

        Ok(())
    }
}

pub struct SerializeStructVariant<'a, W: io::Write> {
    ser: &'a mut Serializer<W>,
    remaining: usize,
    level: usize,
}

impl<W: io::Write> serde::ser::SerializeStructVariant for SerializeStructVariant<'_, W> {
    type Ok = ();

    type Error = SerializeError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        if self.level != self.ser.level {
            return Err(SerializeError::SerializerNotProperlyEnded);
        }

        if self.remaining == 0 {
            return Err(SerializeError::MoreElementsThanPromised);
        }

        self.remaining -= 1;

        self.ser.write_cached_str(key, &TypeTag::Str)?;
        value.serialize(&mut *self.ser)?;

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.remaining != 0 {
            return Err(SerializeError::LessElementsThanPromised);
        }

        self.ser.level -= 1;

        Ok(())
    }
}