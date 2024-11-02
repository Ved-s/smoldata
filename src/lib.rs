pub mod de;
mod macros;
pub mod ser;
pub mod varint;

#[cfg(test)]
mod tests;
pub mod raw;
mod tag;

use std::{io, ops::Deref, sync::Arc};

use de::DeserializeError;
use ser::SerializeError;
use serde::{de::DeserializeOwned, Serialize};

pub use ser::Serializer;
pub use de::Deserializer;
pub use raw::RawValue;

const MAGIC_HEADER: &[u8] = b"sd";

const FORMAT_VERSION: u8 = 0;

enum MaybeArcStr<'a> {
    Arc(Arc<str>),
    Str(&'a str),
}

impl<'a> From<&'a str> for MaybeArcStr<'a> {
    fn from(value: &'a str) -> Self {
        Self::Str(value)
    }
}

impl From<Arc<str>> for MaybeArcStr<'_> {
    fn from(value: Arc<str>) -> Self {
        Self::Arc(value)
    }
}

impl Deref for MaybeArcStr<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        match self {
            MaybeArcStr::Arc(arc) => arc.deref(),
            MaybeArcStr::Str(s) => s,
        }
    }
}

impl<'a> From<MaybeArcStr<'a>> for Arc<str> {
    fn from(val: MaybeArcStr<'a>) -> Self {
        match val {
            MaybeArcStr::Arc(a) => a,
            MaybeArcStr::Str(s) => s.into(),
        }
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

/// Serialize data into a RawValue.
pub fn to_raw<T: Serialize>(data: &T) -> Result<RawValue, SerializeError> {
    RawValue::serialize_from(data)
}

/// Deserialize data from a reader.<br>
/// Reader preferred to be buffered, deserialization does many small reads
pub fn from_reader<T: DeserializeOwned, R: io::Read>(reader: R) -> Result<T, DeserializeError> {
    let mut de = de::Deserializer::new(reader)?;
    T::deserialize(&mut de)
}

/// Deserialize data from a slice of bytes.
pub fn from_bytes<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DeserializeError> {
    let cur = std::io::Cursor::new(bytes);
    from_reader(cur)
}

/// Deserialize data from a RawValue.
pub fn from_raw<T: DeserializeOwned>(raw: &RawValue) -> Result<T, DeserializeError> {
    raw.deserialize_into()
}