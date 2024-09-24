pub mod de;
mod macros;
pub mod ser;
pub mod varint;

#[cfg(test)]
mod tests;

use std::io;

use de::DeserializeError;
use ser::SerializeError;
pub use ser::Serializer;
use serde::{de::DeserializeOwned, Serialize};

const MAGIC_HEADER: &[u8] = b"sd";

const FORMAT_VERSION: u8 = 0;

enum_repr! {
    #[repr(u8)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TypeTag {
        /// (), no data
        Unit = 0,

        /// bool false
        BoolFalse = 1,

        /// bool true
        BoolTrue = 2,

        /// `u8`, one byte of `u8` follows
        U8 = 3,
        /// `i8`, one byte of `i8` follows
        I8 = 4,

        /// `u16`, 2 bytes of Little Endian encoded `u16` follows
        U16 = 5,
        /// `i16`, 2 bytes of Little Endian encoded `i16` follows
        I16 = 6,
        /// `u32`, 4 bytes of Little Endian encoded `u32` follows
        U32 = 7,
        /// `i32`, 4 bytes of Little Endian encoded `i32` follows
        I32 = 8,
        /// `u64`, 8 bytes of Little Endian encoded `u64` follows
        U64 = 9,
        /// `i64`, 8 bytes of Little Endian encoded `i64` follows
        I64 = 10,
        /// `u128`, 16 bytes of Little Endian encoded `u128` follows
        U128 = 11,
        /// `i128`, 16 bytes of Little Endian encoded `i128` follows
        I128 = 12,

        /// `u16`, varint encoded `u16` follows
        U16Var = 13,
        /// `i16`, varint encoded `i16` follows
        I16Var = 14,
        /// `u32`, varint encoded `u32` follows
        U32Var = 15,
        /// `i32`, varint encoded `i32` follows
        I32Var = 16,
        /// `u64`, varint encoded `u64` follows
        U64Var = 17,
        /// `i64`, varint encoded `i64` follows
        I64Var = 18,
        /// `u128`, varint encoded `u128` follows
        U128Var = 19,
        /// `i128`, varint encoded `i128` follows
        I128Var = 20,

        /// `f32`, 4 bytes of Little Endian encoded IEEE 754 binary32
        F32 = 21,
        /// `f64`, 8 bytes of Little Endian encoded IEEE 754 binary64
        F64 = 22,

        /// `char as u32`, 4 bytes of Little Endian encoded `u32` follows
        Char32 = 23,

        /// `char as u32`, varint encoded `u32` follows
        CharVar = 24,

        /// String index in string map as `u32`, varint encoded `u32` follow
        StrIndex = 25,

        /// New string for string map,
        ///  index as varint encoded `u32`,
        ///  strlen as varint encoded `usize`
        ///  and string data encoded as utf8 follow
        StrNew = 26,

        /// New string without caching,
        ///  strlen as varint encoded `usize`
        ///  and string data encoded as utf8 follow
        StrDirect = 27,

        /// "", no data
        EmptyStr = 28,

        /// `[u8]`, length as varint encoded `usize` and byte data follow
        Bytes = 29,

        /// `Option::None`, no data
        None = 30,

        /// `Option::Some`, object follows
        Some = 31,

        /// unit struct, no data
        UnitStruct = 32,

        /// unit variant, name as `Self::StrIndex` data follows
        UnitVariantStrIndex = 33,

        /// unit variant, name as `Self::StrNew` data follows
        UnitVariantStrNew = 34,

        /// newtype struct, object follows
        NewtypeStruct = 35,

        /// newtype variant, name as `Self::StrIndex` data and object follow
        NewtypeVariantStrIndex = 36,

        /// newtype variant, name as `Self::StrNew` data and object follow
        NewtypeVariantStrNew = 37,

        /// `[T]`, objects follow until End tag
        Seq = 38,

        /// `[T]`, length as varint encoded usize and objects follow
        LenSeq = 39,

        /// `(T, ...)`, length as varint encoded usize and objects follow
        Tuple = 40,

        /// tuple struct, `Self::Tuple` data follows
        TupleStruct = 41,

        /// tuple variant, name as `Self::StrIndex` data and `Self::Tuple` data follow
        TupleVariantStrIndex = 42,

        /// tuple variant, name as `Self::StrNew` data and `Self::Tuple` data follow
        TupleVariantStrNew = 43,

        /// `[(T, T)]`, pairs of key-value objects follow until End tag
        Map = 44,

        /// `[(T, T)]`, length as varint encoded usize and pairs of key-value objects follow
        LenMap = 45,

        /// `[(String, T)]`, length as varint encoded `usize` and pairs of key-value strings and objects follow
        Struct = 46,

        /// struct variant, name as `Self::StrIndex` data and `Self::Struct` data follow
        StructVariantStrIndex = 47,

        /// struct variant, name as `Self::StrNew` data and `Self::Struct` data follow
        StructVariantStrNew = 48,

        /// End marker for Seq and Map
        End = 255,
    }
}

/// Serialize data into a writer.<br>
/// Writer preferred to be buffered, serialization does many small writes
pub fn to_writer<T: Serialize, W: io::Write>(data: &T, writer: W) -> Result<(), SerializeError> {
    let mut ser = ser::Serializer::new(writer, 255)?;
    data.serialize(&mut ser)
}

/// Serialize data into a Vec of bytes.
pub fn to_bytes<T: Serialize>(data: &T) -> Result<Vec<u8>, SerializeError> {
    let mut vec = vec![];
    to_writer(data, &mut vec)?;
    Ok(vec)
}

/// Deserialize data from a reader.<br>
/// Reader preferred to be buffered, deserialization does many small reads
pub fn from_reader<T: DeserializeOwned, R: io::Read>(reader: R) -> Result<T, DeserializeError> {
    let mut de = de::Deserializer::new(reader)?;
    T::deserialize(&mut de)
}

/// Deserialize data from a slice of bytes.<br>
pub fn from_bytes<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DeserializeError> {
    let cur = std::io::Cursor::new(bytes);
    from_reader(cur)
}