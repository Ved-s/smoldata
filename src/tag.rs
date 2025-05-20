use std::{io, sync::Arc};

use crate::{reader::{PackedI128, PackedU128, Primitive}, str::RefArcStr};

define_tag! {
    #[repr(u8)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TagType {

        #[doc = "(), no data"]
        Unit = 0,

        #[doc = "bool false"]
        BoolFalse = 1,

        #[doc = "bool true"]
        BoolTrue = 2,

        #[doc = "`u8`, one byte of `u8` follows"]
        U8 = 3,

        #[doc = "`i8`, one byte of `i8` follows"]
        I8 = 4,

        #[doc = "`u16`, 2 bytes of Little Endian encoded `u16` follows"]
        U16 = 5,

        #[doc = "`i16`, 2 bytes of Little Endian encoded `i16` follows"]
        I16 = 6,

        #[doc = "`u32`, 4 bytes of Little Endian encoded `u32` follows"]
        U32 = 7,

        #[doc = "`i32`, 4 bytes of Little Endian encoded `i32` follows"]
        I32 = 8,

        #[doc = "`u64`, 8 bytes of Little Endian encoded `u64` follows"]
        U64 = 9,

        #[doc = "`i64`, 8 bytes of Little Endian encoded `i64` follows"]
        I64 = 10,

        #[doc = "`u128`, 16 bytes of Little Endian encoded `u128` follows"]
        U128 = 11,

        #[doc = "`i128`, 16 bytes of Little Endian encoded `i128` follows"]
        I128 = 12,

        #[doc = "`u16`, varint encoded `u16` follows"]
        U16Var = 13,

        #[doc = "`i16`, varint encoded `i16` follows"]
        I16Var = 14,

        #[doc = "`u32`, varint encoded `u32` follows"]
        U32Var = 15,

        #[doc = "`i32`, varint encoded `i32` follows"]
        I32Var = 16,

        #[doc = "`u64`, varint encoded `u64` follows"]
        U64Var = 17,

        #[doc = "`i64`, varint encoded `i64` follows"]
        I64Var = 18,

        #[doc = "`u128`, varint encoded `u128` follows"]
        U128Var = 19,

        #[doc = "`i128`, varint encoded `i128` follows"]
        I128Var = 20,

        #[doc = "`f32`, 4 bytes of Little Endian encoded IEEE 754 binary32"]
        F32 = 21,

        #[doc = "`f64`, 8 bytes of Little Endian encoded IEEE 754 binary64"]
        F64 = 22,

        #[doc = "`char as u32`, 4 bytes of Little Endian encoded `u32` follows"]
        Char32 = 23,

        #[doc = "`char as u32`, varint encoded `u32` follows"]
        CharVar = 24,

        #[doc = "Signed varint encoded `u32` id follows, depending on the sign:"]
        #[doc = ""]
        #[doc = "Positive: String id in the string map"]
        #[doc = ""]
        #[doc = "Negative: Index for a string for the string map,"]
        #[doc = ""]
        #[doc = " strlen as varint encoded `usize`"]
        #[doc = ""]
        #[doc = " and string data encoded as utf8 follow"]
        Str = 25,

        #[doc = "New string without caching,"]
        #[doc = " strlen as varint encoded `usize`"]
        #[doc = " and string data encoded as utf8 follow"]
        StrDirect = 26,

        #[doc = "\"\", no data"]
        EmptyStr = 27,

        #[doc = "`[u8]`, length as varint encoded `usize` and byte data follow"]
        Bytes = 28,

        #[doc = "`Option::None`, no data"]
        None = 29,

        #[doc = "`Option::Some`, object follows"]
        Some = 30,

        #[doc = "unit struct, no data"]
        UnitStruct = 31,

        #[doc = "unit variant, name as `Self::Str` data follows"]
        UnitVariant = 32,

        #[doc = "newtype struct, object follows"]
        NewtypeStruct = 33,

        #[doc = "newtype variant, name as `Self::Str` data and object follow"]
        NewtypeVariant = 34,

        #[doc = "`[T]`, objects follow until End tag"]
        Array = 35,

        #[doc = "`[T]`, length as varint encoded usize and objects follow"]
        LenArray = 36,

        #[doc = "`(T, ...)`, length as varint encoded usize and objects follow"]
        Tuple = 37,

        #[doc = "tuple struct, `Self::Tuple` data follows"]
        TupleStruct = 38,

        #[doc = "tuple variant, name as `Self::Str` data and `Self::Tuple` data follow"]
        TupleVariant = 39,

        #[doc = "`[(T, T)]`, pairs of key-value objects follow until End tag"]
        Map = 40,

        #[doc = "`[(T, T)]`, length as varint encoded usize and pairs of key-value objects follow"]
        LenMap = 41,

        #[doc = "`[(String, T)]`, length as varint encoded `usize` and pairs of key-value strings and objects follow"]
        #[doc = ""]
        #[doc = "Strings are encoded without tags, only `Self::Str` data"]
        Struct = 42,

        #[doc = "struct variant, name as `Self::Str` data and `Self::Struct` data follow"]
        StructVariant = 43,

        #[doc = "Meta tag, repeat previously read tag"]
        RepeatTag = 44,

        #[doc = "Meta tag, repeat previously read tag N+2 more times, N as varint encoded `usize` follows"]
        RepeatTagMany = 45,

        #[doc = "End marker for Seq and Map"]
        End = 255,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionTag {
    None,
    Some,
}

impl OptionTag {
    pub fn from_option<T>(op: &Option<T>) -> Self {
        match op {
            Some(_) => Self::Some,
            None => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructType {
    Unit,
    Newtype,
    Tuple,
    Struct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegerTag {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    I128(PackedI128),
    U128(PackedU128),
}

impl From<IntegerTag> for Primitive {
    fn from(val: IntegerTag) -> Self {
        match val {
            IntegerTag::I8(v) => Self::I8(v),
            IntegerTag::U8(v) => Self::U8(v),
            IntegerTag::I16(v) => Self::I16(v),
            IntegerTag::U16(v) => Self::U16(v),
            IntegerTag::I32(v) => Self::I32(v),
            IntegerTag::U32(v) => Self::U32(v),
            IntegerTag::I64(v) => Self::I64(v),
            IntegerTag::U64(v) => Self::U64(v),
            IntegerTag::I128(v) => Self::I128(v),
            IntegerTag::U128(v) => Self::U128(v),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructTag {
    Unit,

    /// object follows
    Newtype,

    /// `len` objects follow
    Tuple {
        len: usize,
    },

    /// `len` pairs of key-value strings and objects follow
    /// 
    /// Strings are encoded without tags
    Struct {
        len: usize,
    }
}

impl StructTag {
    pub const fn has_more_data(&self) -> bool {
        match self {
            StructTag::Unit => false,
            StructTag::Newtype => true,
            StructTag::Tuple { len } => *len > 0,
            StructTag::Struct { len } => *len > 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tag<'a> {
    Unit,
    Bool(bool),
    Integer(IntegerTag),
    F32(f32),
    F64(f64),
    Char(char),

    Str(RefArcStr<'a>),

    /// `len` bytes of utf-8 encoded string data follow
    StrDirect {
        len: usize
    },
    EmptyStr,

    /// `len` bytes of data follow
    Bytes {
        len: usize,
    },

    /// on Some, an object follows
    Option(OptionTag),

    /// See [`StructTag`] for following data
    Struct(StructTag),
    
    /// See [`StructTag`] for following data
    Variant {
        name: Arc<str>,
        ty: StructTag
    },

    /// when `len: Some(len)`, `len` objects follow,
    /// otherwise, objects follow until End tag
    Array {
        len: Option<usize>
    },

    /// when `len: Some(len)`, `len` pairs of objects follow,
    /// otherwise, pairs of objects follow until End tag
    Map {
        len: Option<usize>
    },

    /// `len` objects follow,
    Tuple {
        len: usize
    }
}

impl Tag<'_> {
    pub const fn has_more_data(&self) -> bool {
        match self {
            Tag::Unit => false,
            Tag::Bool(_) => false,
            Tag::Integer(_) => false,
            Tag::F32(_) => false,
            Tag::F64(_) => false,
            Tag::Char(_) => false,
            Tag::Str(_) => false,
            Tag::StrDirect { len } => *len > 0,
            Tag::EmptyStr => false,
            Tag::Bytes { len } => *len > 0,
            Tag::Option(OptionTag::None) => false,
            Tag::Option(OptionTag::Some) => true,
            Tag::Struct(tag) => tag.has_more_data(),
            Tag::Variant { name: _, ty } => ty.has_more_data(),
            Tag::Array { len: None } => true,
            Tag::Array { len: Some(len) } => *len > 0,
            Tag::Map { len: None } => true,
            Tag::Map { len: Some(len) } => *len > 0,
            Tag::Tuple { len } => *len > 0,
        }
    }

    pub fn into_static(self) -> Tag<'static> {
        match self {
            Tag::Unit => Tag::Unit,
            Tag::Bool(v) => Tag::Bool(v),
            Tag::Integer(v) => Tag::Integer(v),
            Tag::F32(v) => Tag::F32(v),
            Tag::F64(v) => Tag::F64(v),
            Tag::Char(v) => Tag::Char(v),
            Tag::Str(v) => Tag::Str(v.into_static()),
            Tag::StrDirect { len } => Tag::StrDirect  { len },
            Tag::EmptyStr => Tag::EmptyStr,
            Tag::Bytes { len } => Tag::Bytes { len },
            Tag::Option(v) => Tag::Option(v),
            Tag::Struct(v) => Tag::Struct(v),
            Tag::Variant { name, ty } => Tag::Variant { name, ty },
            Tag::Array { len } => Tag::Array { len },
            Tag::Map { len } => Tag::Map { len },
            Tag::Tuple { len } => Tag::Tuple { len },
        }
    }

    pub fn eq_with_nan(&self, other: &Tag) -> bool {
        match (self, other) {
            (Self::F32(a), Tag::F32(b)) => a.eq(b) || (a.is_nan() && b.is_nan()),
            (Self::F64(a), Tag::F64(b)) => a.eq(b) || (a.is_nan() && b.is_nan()),
            _ => self.eq(other)
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TagReadError {
    #[error(transparent)]
    IoError(#[from] io::Error),

    #[error(transparent)]
    InvalidTagError(#[from] InvalidTagError),
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid tag value {0}")]
pub struct InvalidTagError(u8);

// impl TypeTag {
//     #[rustfmt::skip]
//     pub const fn tag_params(self) -> &'static [TagParameter] {
//         match self {
//             TypeTag::Unit => &[],
//             TypeTag::Bool(_) => &[],

//             TypeTag::Integer { width: _, signed: _, varint: true }
//                 => &[TagParameter::Varint],
//             TypeTag::Integer { width: IntWidth::W8, signed: _, varint: false }
//                 => &[TagParameter::ShortBytes { len: 1 }],
//             TypeTag::Integer { width: IntWidth::W16, signed: _, varint: false }
//                 => &[TagParameter::ShortBytes { len: 2 }],
//             TypeTag::Integer { width: IntWidth::W32, signed: _, varint: false }
//                 => &[TagParameter::ShortBytes { len: 4 }],
//             TypeTag::Integer { width: IntWidth::W64, signed: _, varint: false }
//                 => &[TagParameter::ShortBytes { len: 8 }],
//             TypeTag::Integer { width: IntWidth::W128, signed: _, varint: false }
//                 => &[TagParameter::ShortBytes { len: 16 }],

//             TypeTag::Char { varint: false } => &[TagParameter::ShortBytes { len: 4 }],
//             TypeTag::Char { varint: true } => &[TagParameter::Varint],

//             TypeTag::Float(FloatWidth::F32) => &[TagParameter::ShortBytes { len: 4 }],
//             TypeTag::Float(FloatWidth::F64) => &[TagParameter::ShortBytes { len: 8 }],

//             TypeTag::Str => &[TagParameter::StringRef],
//             TypeTag::StrDirect => &[TagParameter::VarintLengthPrefixedBytearray],
//             TypeTag::EmptyStr => &[],

//             TypeTag::Bytes => &[TagParameter::VarintLengthPrefixedBytearray],
//             TypeTag::Option(OptionTag::None) => &[],
//             TypeTag::Option(OptionTag::Some) => &[],

//             TypeTag::Struct(StructType::Unit) => &[],
//             TypeTag::Struct(StructType::Newtype) => &[],
//             TypeTag::Struct(StructType::Tuple) => &[TagParameter::Varint],
//             TypeTag::Struct(StructType::Struct) => &[TagParameter::Varint],

//             TypeTag::EnumVariant(StructType::Unit)
//                 => &[TagParameter::StringRef],
//             TypeTag::EnumVariant(StructType::Newtype)
//                 => &[TagParameter::StringRef],
//             TypeTag::EnumVariant(StructType::Tuple)
//                 => &[TagParameter::StringRef, TagParameter::Varint],
//             TypeTag::EnumVariant(StructType::Struct)
//                 => &[TagParameter::StringRef, TagParameter::Varint],

//             TypeTag::Array { has_length: true } => &[TagParameter::Varint],
//             TypeTag::Array { has_length: false } => &[],
//             TypeTag::Tuple => &[TagParameter::Varint],
//             TypeTag::Map { has_length: true } => &[TagParameter::Varint],
//             TypeTag::Map { has_length: false } => &[],
//             TypeTag::End => &[],
//         }
//     }

//     pub fn write<W: io::Write + ?Sized>(&self, writer: &mut W) -> io::Result<()> {
//         writer.write_all(&[self.pack().into()])
//     }

//     pub fn read<R: io::Read + ?Sized>(reader: &mut R) -> Result<Self, TagReadError> {
//         let mut byte = 0u8;
//         reader.read_exact(slice::from_mut(&mut byte))?;
//         let packed = FlatTypeTag::try_from(byte)
//             .map_err(|v| TagReadError::InvalidTagError(InvalidTagError(v)))?;
//         Ok(packed.into())
//     }
// }

// pub enum TagParameter {
//     ShortBytes {
//         len: u8
//     },
//     Varint,
//     VarintLengthPrefixedBytearray,
//     StringRef,
// }
