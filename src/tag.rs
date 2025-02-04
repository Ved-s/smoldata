use std::{io, slice};

define_tag! {
    #[repr(u8)]
    #[unpack(TypeTag)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FlatTypeTag {

        #[unpack(exact Unit)]
        #[doc = "(), no data"]
        Unit = 0,

        #[unpack(exact Bool(false))]
        #[doc = "bool false"]
        BoolFalse = 1,

        #[unpack(exact Bool(true))]
        #[doc = "bool true"]
        BoolTrue = 2,

        #[unpack(
            pack(Integer { width: IntWidth::W8, signed: false, varint: _ })
            unpack(Integer { width: IntWidth::W8, signed: false, varint: false })
        )]
        #[doc = "`u8`, one byte of `u8` follows"]
        U8 = 3,

        #[unpack(
            pack(Integer { width: IntWidth::W8, signed: true, varint: _ })
            unpack(Integer { width: IntWidth::W8, signed: true, varint: false })
        )]
        #[doc = "`i8`, one byte of `i8` follows"]
        I8 = 4,

        #[unpack(exact Integer { width: IntWidth::W16, signed: false, varint: false })]
        #[doc = "`u16`, 2 bytes of Little Endian encoded `u16` follows"]
        U16 = 5,

        #[unpack(exact Integer { width: IntWidth::W16, signed: true, varint: false })]
        #[doc = "`i16`, 2 bytes of Little Endian encoded `i16` follows"]
        I16 = 6,

        #[unpack(exact Integer { width: IntWidth::W32, signed: false, varint: false })]
        #[doc = "`u32`, 4 bytes of Little Endian encoded `u32` follows"]
        U32 = 7,

        #[unpack(exact Integer { width: IntWidth::W32, signed: true, varint: false })]
        #[doc = "`i32`, 4 bytes of Little Endian encoded `i32` follows"]
        I32 = 8,

        #[unpack(exact Integer { width: IntWidth::W64, signed: false, varint: false })]
        #[doc = "`u64`, 8 bytes of Little Endian encoded `u64` follows"]
        U64 = 9,

        #[unpack(exact Integer { width: IntWidth::W64, signed: true, varint: false })]
        #[doc = "`i64`, 8 bytes of Little Endian encoded `i64` follows"]
        I64 = 10,

        #[unpack(exact Integer { width: IntWidth::W128, signed: false, varint: false })]
        #[doc = "`u128`, 16 bytes of Little Endian encoded `u128` follows"]
        U128 = 11,

        #[unpack(exact Integer { width: IntWidth::W128, signed: true, varint: false })]
        #[doc = "`i128`, 16 bytes of Little Endian encoded `i128` follows"]
        I128 = 12,

        #[unpack(exact Integer { width: IntWidth::W16, signed: false, varint: true })]
        #[doc = "`u16`, varint encoded `u16` follows"]
        U16Var = 13,

        #[unpack(exact Integer { width: IntWidth::W16, signed: true, varint: true })]
        #[doc = "`i16`, varint encoded `i16` follows"]
        I16Var = 14,

        #[unpack(exact Integer { width: IntWidth::W32, signed: false, varint: true })]
        #[doc = "`u32`, varint encoded `u32` follows"]
        U32Var = 15,

        #[unpack(exact Integer { width: IntWidth::W32, signed: true, varint: true })]
        #[doc = "`i32`, varint encoded `i32` follows"]
        I32Var = 16,

        #[unpack(exact Integer { width: IntWidth::W64, signed: false, varint: true })]
        #[doc = "`u64`, varint encoded `u64` follows"]
        U64Var = 17,

        #[unpack(exact Integer { width: IntWidth::W64, signed: true, varint: true })]
        #[doc = "`i64`, varint encoded `i64` follows"]
        I64Var = 18,

        #[unpack(exact Integer { width: IntWidth::W128, signed: false, varint: true })]
        #[doc = "`u128`, varint encoded `u128` follows"]
        U128Var = 19,

        #[unpack(exact Integer { width: IntWidth::W128, signed: true, varint: true })]
        #[doc = "`i128`, varint encoded `i128` follows"]
        I128Var = 20,

        #[unpack(exact Float(FloatWidth::F32))]
        #[doc = "`f32`, 4 bytes of Little Endian encoded IEEE 754 binary32"]
        F32 = 21,

        #[unpack(exact Float(FloatWidth::F64))]
        #[doc = "`f64`, 8 bytes of Little Endian encoded IEEE 754 binary64"]
        F64 = 22,

        #[unpack(exact Char { varint: false })]
        #[doc = "`char as u32`, 4 bytes of Little Endian encoded `u32` follows"]
        Char32 = 23,

        #[unpack(exact Char { varint: true })]
        #[doc = "`char as u32`, varint encoded `u32` follows"]
        CharVar = 24,

        #[unpack(exact Str)]
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

        #[unpack(exact StrDirect)]
        #[doc = "New string without caching,"]
        #[doc = " strlen as varint encoded `usize`"]
        #[doc = " and string data encoded as utf8 follow"]
        StrDirect = 26,

        #[unpack(exact EmptyStr)]
        #[doc = "\"\", no data"]
        EmptyStr = 27,

        #[unpack(exact Bytes)]
        #[doc = "`[u8]`, length as varint encoded `usize` and byte data follow"]
        Bytes = 28,

        #[unpack(exact Option(OptionTag::None))]
        #[doc = "`Option::None`, no data"]
        None = 29,

        #[unpack(exact Option(OptionTag::Some))]
        #[doc = "`Option::Some`, object follows"]
        Some = 30,

        #[unpack(exact Struct(StructType::Unit))]
        #[doc = "unit struct, no data"]
        UnitStruct = 31,

        #[unpack(exact EnumVariant(StructType::Unit))]
        #[doc = "unit variant, name as `Self::Str` data follows"]
        UnitVariant = 32,

        #[unpack(exact Struct(StructType::Newtype))]
        #[doc = "newtype struct, object follows"]
        NewtypeStruct = 33,

        #[unpack(exact EnumVariant(StructType::Newtype))]
        #[doc = "newtype variant, name as `Self::Str` data and object follow"]
        NewtypeVariant = 34,

        #[unpack(exact Array { has_length: false })]
        #[doc = "`[T]`, objects follow until End tag"]
        Array = 35,

        #[unpack(exact Array { has_length: true })]
        #[doc = "`[T]`, length as varint encoded usize and objects follow"]
        LenArray = 36,

        #[unpack(exact Tuple)]
        #[doc = "`(T, ...)`, length as varint encoded usize and objects follow"]
        Tuple = 37,

        #[unpack(exact Struct(StructType::Tuple))]
        #[doc = "tuple struct, `Self::Tuple` data follows"]
        TupleStruct = 38,

        #[unpack(exact EnumVariant(StructType::Tuple))]
        #[doc = "tuple variant, name as `Self::Str` data and `Self::Tuple` data follow"]
        TupleVariant = 39,

        #[unpack(exact Map { has_length: false })]
        #[doc = "`[(T, T)]`, pairs of key-value objects follow until End tag"]
        Map = 40,

        #[unpack(exact Map { has_length: true })]
        #[doc = "`[(T, T)]`, length as varint encoded usize and pairs of key-value objects follow"]
        LenMap = 41,

        #[unpack(exact Struct(StructType::Struct))]
        #[doc = "`[(String, T)]`, length as varint encoded `usize` and pairs of key-value strings and objects follow"]
        #[doc = ""]
        #[doc = "Strings are encoded without tags, only `Self::Str` data"]
        Struct = 42,

        #[unpack(exact EnumVariant(StructType::Struct))]
        #[doc = "struct variant, name as `Self::Str` data and `Self::Struct` data follow"]
        StructVariant = 43,

        #[unpack(exact End)]
        #[doc = "End marker for Seq and Map"]
        End = 255,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntWidth {
    W8,
    W16,
    W32,
    W64,
    W128,
}

impl IntWidth {
    pub const MAX_BYTES: usize = 16;

    pub const fn bytes(self) -> usize {
        match self {
            IntWidth::W8 => 1,
            IntWidth::W16 => 2,
            IntWidth::W32 => 4,
            IntWidth::W64 => 8,
            IntWidth::W128 => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatWidth {
    F32,
    F64,
}

impl FloatWidth {
    pub const MAX_BYTES: usize = 8;

    pub const fn bytes(self) -> usize {
        match self {
            FloatWidth::F32 => 4,
            FloatWidth::F64 => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrNewIndex {
    New,
    Index,
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
pub enum TypeTag {
    Unit,
    Bool(bool),
    Integer {
        width: IntWidth,
        signed: bool,

        #[doc = "Ignored for n8"]
        varint: bool,
    },
    Char {
        varint: bool,
    },
    Float(FloatWidth),
    Str,
    StrDirect,
    EmptyStr,
    Bytes,
    Option(OptionTag),
    Struct(StructType),
    EnumVariant(StructType),
    Array {
        has_length: bool,
    },
    Tuple,
    Map {
        has_length: bool,
    },
    End,
}


#[derive(Debug, thiserror::Error)]
pub enum TagReadError {
    #[error(transparent)]
    IoError(#[from] io::Error),
    
    #[error(transparent)]
    InvalidTagError(#[from] InvalidTagError)
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid tag value {0}")]
pub struct InvalidTagError(u8);

impl TypeTag {
    #[rustfmt::skip]
    pub const fn tag_params(self) -> &'static [TagParameter] {
        match self {
            TypeTag::Unit => &[],
            TypeTag::Bool(_) => &[],

            TypeTag::Integer { width: _, signed: _, varint: true } 
                => &[TagParameter::Varint],
            TypeTag::Integer { width: IntWidth::W8, signed: _, varint: false } 
                => &[TagParameter::FixedIntBytes(IntWidth::W8)],
            TypeTag::Integer { width: IntWidth::W16, signed: _, varint: false } 
                => &[TagParameter::FixedIntBytes(IntWidth::W16)],
            TypeTag::Integer { width: IntWidth::W32, signed: _, varint: false } 
                => &[TagParameter::FixedIntBytes(IntWidth::W32)],
            TypeTag::Integer { width: IntWidth::W64, signed: _, varint: false } 
                => &[TagParameter::FixedIntBytes(IntWidth::W64)],
            TypeTag::Integer { width: IntWidth::W128, signed: _, varint: false } 
                => &[TagParameter::FixedIntBytes(IntWidth::W128)],

            TypeTag::Char { varint: false } => &[TagParameter::FixedIntBytes(IntWidth::W32)],
            TypeTag::Char { varint: true } => &[TagParameter::Varint],

            TypeTag::Float(FloatWidth::F32) => &[TagParameter::FixedIntBytes(IntWidth::W32)],
            TypeTag::Float(FloatWidth::F64) => &[TagParameter::FixedIntBytes(IntWidth::W64)],

            TypeTag::Str => &[TagParameter::StringRef],
            TypeTag::StrDirect => &[TagParameter::VarintLengthPrefixedBytearray],
            TypeTag::EmptyStr => &[],

            TypeTag::Bytes => &[TagParameter::VarintLengthPrefixedBytearray],
            TypeTag::Option(OptionTag::None) => &[],
            TypeTag::Option(OptionTag::Some) => &[],

            TypeTag::Struct(StructType::Unit) => &[],
            TypeTag::Struct(StructType::Newtype) => &[],
            TypeTag::Struct(StructType::Tuple) => &[TagParameter::Varint],
            TypeTag::Struct(StructType::Struct) => &[TagParameter::Varint],

            TypeTag::EnumVariant(StructType::Unit) 
                => &[TagParameter::StringRef],
            TypeTag::EnumVariant(StructType::Newtype) 
                => &[TagParameter::StringRef],
            TypeTag::EnumVariant(StructType::Tuple) 
                => &[TagParameter::StringRef, TagParameter::Varint],
            TypeTag::EnumVariant(StructType::Struct) 
                => &[TagParameter::StringRef, TagParameter::Varint],

            TypeTag::Array { has_length: true } => &[TagParameter::Varint],
            TypeTag::Array { has_length: false } => &[],
            TypeTag::Tuple => &[TagParameter::Varint],
            TypeTag::Map { has_length: true } => &[TagParameter::Varint],
            TypeTag::Map { has_length: false } => &[],
            TypeTag::End => &[],
        }
    }

    pub fn write<W: io::Write + ?Sized>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[self.pack().into()])
    }

    pub fn read<R: io::Read + ?Sized>(reader: &mut R) -> Result<Self, TagReadError> {
        let mut byte = 0u8;
        reader.read_exact(slice::from_mut(&mut byte))?;
        let packed = FlatTypeTag::try_from(byte).map_err(|v| TagReadError::InvalidTagError(InvalidTagError(v)))?;
        Ok(packed.into())
    }
}

pub enum TagParameter {
    FixedIntBytes(IntWidth),
    Varint,
    VarintLengthPrefixedBytearray,
    StringRef,
}