use std::{collections::HashMap, error::Error, fmt::Display, io};

use crate::{varint, TypeTag, FORMAT_VERSION, MAGIC_HEADER};

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
    writer: W,
    string_map: HashMap<Box<str>, u32>,
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

        let this = Self {
            writer,
            string_map: Default::default(),
            level: 0,

            next_map_index: 0,
            max_cache_str_len,
        };
        serializer_debugprintln!(this, " -- Serializer debug log --\nversion: {FORMAT_VERSION}");

        Ok(this)
    }

    fn write_tag(&mut self, tag: TypeTag) -> Result<(), io::Error> {
        serializer_debugprintln!(self, "tag: {tag:?}");
        self.writer.write_all(&[tag.into()])
    }

    fn write_cached_str(
        &mut self,
        s: &str,
        indextag: TypeTag,
        newtag: TypeTag,
    ) -> Result<(), io::Error> {
        if let Some(index) = self.string_map.get(s).copied() {
            self.write_tag(indextag)?;
            serializer_debugprintln!(self, "index: {index} (\"{s}\")");
            varint::write_unsigned_varint(&mut self.writer, index)?;
        } else {
            let index = self.next_map_index;

            self.write_tag(newtag)?;
            varint::write_unsigned_varint(&mut self.writer, index)?;
            varint::write_unsigned_varint(&mut self.writer, s.len())?;
            self.writer.write_all(s.as_bytes())?;

            serializer_debugprintln!(self, "string: {index} (\"{s}\")");

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
        if v {
            self.write_tag(TypeTag::BoolTrue)?;
        } else {
            self.write_tag(TypeTag::BoolFalse)?;
        }

        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::I8)?;
        self.writer.write_all(&[v as u8])?;

        serializer_debugprintln!(self, "i8: {v}");

        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        if is_varint_better(v.unsigned_abs().leading_zeros(), 2, true) {
            self.write_tag(TypeTag::I16Var)?;
            varint::write_signed_varint(&mut self.writer, v)?;
        } else {
            self.write_tag(TypeTag::I16)?;
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "i16: {v}");
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        if is_varint_better(v.unsigned_abs().leading_zeros(), 4, true) {
            self.write_tag(TypeTag::I32Var)?;
            varint::write_signed_varint(&mut self.writer, v)?;
        } else {
            self.write_tag(TypeTag::I32)?;
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "i32: {v}");
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        if is_varint_better(v.unsigned_abs().leading_zeros(), 8, true) {
            self.write_tag(TypeTag::I64Var)?;
            varint::write_signed_varint(&mut self.writer, v)?;
        } else {
            self.write_tag(TypeTag::I64)?;
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "i64: {v}");
        Ok(())
    }

    fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
        if is_varint_better(v.unsigned_abs().leading_zeros(), 16, true) {
            self.write_tag(TypeTag::I128Var)?;
            varint::write_signed_varint(&mut self.writer, v)?;
        } else {
            self.write_tag(TypeTag::I128)?;
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "i128: {v}");
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::U8)?;
        self.writer.write_all(&[v])?;

        serializer_debugprintln!(self, "u8: {v}");

        Ok(())
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        if is_varint_better(v.leading_zeros(), 2, true) {
            self.write_tag(TypeTag::U16Var)?;
            varint::write_unsigned_varint(&mut self.writer, v)?;
        } else {
            self.write_tag(TypeTag::U16)?;
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "u16: {v}");
        Ok(())
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        if is_varint_better(v.leading_zeros(), 4, true) {
            self.write_tag(TypeTag::U32Var)?;
            varint::write_unsigned_varint(&mut self.writer, v)?;
        } else {
            self.write_tag(TypeTag::U32)?;
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "u32: {v}");
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        if is_varint_better(v.leading_zeros(), 8, true) {
            self.write_tag(TypeTag::U64Var)?;
            varint::write_unsigned_varint(&mut self.writer, v)?;
        } else {
            self.write_tag(TypeTag::U64)?;
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "u64: {v}");
        Ok(())
    }

    fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
        if is_varint_better(v.leading_zeros(), 16, true) {
            self.write_tag(TypeTag::U128Var)?;
            varint::write_unsigned_varint(&mut self.writer, v)?;
        } else {
            self.write_tag(TypeTag::U128)?;
            self.writer.write_all(&v.to_le_bytes())?;
        }
        serializer_debugprintln!(self, "u128: {v}");
        Ok(())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::F32)?;
        self.writer.write_all(&v.to_le_bytes())?;

        serializer_debugprintln!(self, "f32: {v}");

        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::F64)?;
        self.writer.write_all(&v.to_le_bytes())?;

        serializer_debugprintln!(self, "f64: {v}");

        Ok(())
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        let v = v as u32;

        if is_varint_better(v.leading_zeros(), 4, true) {
            self.write_tag(TypeTag::CharVar)?;
            varint::write_unsigned_varint(&mut self.writer, v)?;
        } else {
            self.write_tag(TypeTag::Char32)?;
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
            self.write_cached_str(v, TypeTag::StrIndex, TypeTag::StrNew)?;
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
        self.write_tag(TypeTag::None)?;

        Ok(())
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        self.write_tag(TypeTag::Some)?;
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::Unit)?;

        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.write_tag(TypeTag::UnitStruct)?;

        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.write_cached_str(
            variant,
            TypeTag::UnitVariantStrIndex,
            TypeTag::UnitVariantStrNew,
        )?;

        Ok(())
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        self.write_tag(TypeTag::NewtypeStruct)?;
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
        self.write_cached_str(
            variant,
            TypeTag::NewtypeVariantStrIndex,
            TypeTag::NewtypeVariantStrNew,
        )?;
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        match len {
            None => self.write_tag(TypeTag::Seq)?,
            Some(len) => {
                self.write_tag(TypeTag::LenSeq)?;
                serializer_debugprintln!(self, "len: {len}");
                varint::write_unsigned_varint(&mut self.writer, len)?;
            }
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
        self.write_tag(TypeTag::TupleStruct)?;
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
        self.write_cached_str(
            variant,
            TypeTag::TupleVariantStrIndex,
            TypeTag::TupleVariantStrNew,
        )?;
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
        match len {
            None => self.write_tag(TypeTag::Map)?,
            Some(len) => {
                self.write_tag(TypeTag::LenMap)?;
                varint::write_unsigned_varint(&mut self.writer, len)?;
                serializer_debugprintln!(self, "len: {len}");
            }
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
        self.write_tag(TypeTag::Struct)?;
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
        self.write_cached_str(
            variant,
            TypeTag::StructVariantStrIndex,
            TypeTag::StructVariantStrNew,
        )?;
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

impl<'a, W: io::Write> serde::ser::SerializeSeq for SerializeSeq<'a, W> {
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

impl<'a, W: io::Write> serde::ser::SerializeTuple for SerializeTuple<'a, W> {
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

impl<'a, W: io::Write> serde::ser::SerializeTupleStruct for SerializeTupleStruct<'a, W> {
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

impl<'a, W: io::Write> serde::ser::SerializeTupleVariant for SerializeTupleVariant<'a, W> {
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

impl<'a, W: io::Write> serde::ser::SerializeMap for SerializeMap<'a, W> {
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

impl<'a, W: io::Write> serde::ser::SerializeStruct for SerializeStruct<'a, W> {
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

        self.ser.write_cached_str(key, TypeTag::StrIndex, TypeTag::StrNew)?;
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

impl<'a, W: io::Write> serde::ser::SerializeStructVariant for SerializeStructVariant<'a, W> {
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

        self.ser.write_cached_str(key, TypeTag::StrIndex, TypeTag::StrNew)?;
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

fn is_varint_better(abs_leading_zeros: u32, bytewidth: u32, signed: bool) -> bool {
    let value_width = bytewidth * 8 - abs_leading_zeros;

    let rem_value_width = if signed {
        value_width.saturating_sub(6)
    } else {
        value_width.saturating_sub(7)
    };

    let extra_varint_bytes = rem_value_width.div_ceil(7);

    bytewidth > (extra_varint_bytes + 1)
}

mod test {

    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_is_varint_better() {
        let varint_short_values = [0x0u16, 0x7f, 0x0f];
        let varint_long_values = [0x80u16, 0xff];

        for v in varint_short_values {
            assert!(is_varint_better(v.leading_zeros(), 2, false));
        }

        for v in varint_long_values {
            assert!(!is_varint_better(v.leading_zeros(), 2, false));
        }

        let varint_shorter_values = [0x0u16, 0x3f, 0x0f];
        for v in varint_shorter_values {
            assert!(is_varint_better(v.leading_zeros(), 2, true));
        }

        assert!(!is_varint_better(0x7fu16.leading_zeros(), 2, true));

        let varint_short_values = [0x0u32, 0xffff, 0x0f];
        let varint_long_values = [0b1_0000000_0000000_0000000u32, 0xffffffff];

        for v in varint_short_values {
            assert!(is_varint_better(v.leading_zeros(), 4, false));
        }

        for v in varint_long_values {
            assert!(!is_varint_better(v.leading_zeros(), 4, false));
        }
    }
}