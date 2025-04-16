// pub mod de;

#[macro_use]
mod macros;
mod tag;

// pub mod ser;
pub mod varint;

// #[cfg(test)]
// mod tests;
// pub mod raw;
pub mod reader;
pub mod str;
pub mod writer;

use std::{
    any::type_name,
    collections::HashMap,
    hash::Hash,
    io::{self, ErrorKind},
    ops::Deref,
};

use reader::{ReadError, ReadResult, UnexpectedValueResultExt, ValueReader};
use writer::ValueWriter;

pub use smoldata_derive::{SmolRead, SmolReadWrite, SmolWrite};

// use de::DeserializeError;
// use ser::SerializeError;
// use serde::{de::DeserializeOwned, Serialize};

// pub use ser::Serializer;
// pub use de::Deserializer;
// pub use raw::RawValue;

const MAGIC_HEADER: &[u8] = b"sd";

const FORMAT_VERSION: u8 = 1;

// /// Serialize data into a writer.<br>
// /// Writer preferred to be buffered, serialization does many small writes
// pub fn to_writer<T: Serialize, W: io::Write>(data: &T, writer: W) -> Result<(), SerializeError> {
//     let mut ser = ser::Serializer::new(writer, 255)?;
//     data.serialize(&mut ser)
// }

// /// Serialize data into a Vec of bytes.
// pub fn to_bytes<T: Serialize>(data: &T) -> Result<Vec<u8>, SerializeError> {
//     let mut vec = vec![];
//     to_writer(data, &mut vec)?;
//     Ok(vec)
// }

// /// Serialize data into a RawValue.
// pub fn to_raw<T: Serialize>(data: &T) -> Result<RawValue, SerializeError> {
//     RawValue::serialize_from(data)
// }

// /// Deserialize data from a reader.<br>
// /// Reader preferred to be buffered, deserialization does many small reads
// pub fn from_reader<T: DeserializeOwned, R: io::Read>(reader: R) -> Result<T, DeserializeError> {
//     let mut de = de::Deserializer::new(reader)?;
//     T::deserialize(&mut de)
// }

// /// Deserialize data from a slice of bytes.
// pub fn from_bytes<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DeserializeError> {
//     let cur = std::io::Cursor::new(bytes);
//     from_reader(cur)
// }

// /// Deserialize data from a RawValue.
// pub fn from_raw<T: DeserializeOwned>(raw: &RawValue) -> Result<T, DeserializeError> {
//     raw.deserialize_into()
// }

fn copy<R: io::Read + ?Sized, W: io::Write + ?Sized, const B: usize>(
    reader: &mut R,
    writer: &mut W,
    len: Option<usize>,
) -> io::Result<usize> {
    let mut buf = [0u8; B];

    let mut total_read = 0;

    while len.is_none_or(|v| total_read < v) {
        let remaining = len.map(|l| l - total_read);
        let read_size = remaining.map(|r| r.min(B)).unwrap_or(B);

        let buf = &mut buf[..read_size];
        let read = reader.read(buf);

        if read.as_ref().is_ok_and(|r| *r == 0)
            || read
                .as_ref()
                .is_err_and(|e| e.kind() == ErrorKind::UnexpectedEof)
        {
            if let Some(len) = len {
                let remaining = len - total_read;
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    format!("Expected {remaining} more bytes to copy"),
                ));
            } else {
                break;
            }
        }

        let read = read?;
        let buf = &buf[..read];

        writer.write_all(buf)?;
        total_read += read;
    }

    Ok(total_read)
}

pub trait SmolWrite {
    fn write(&self, writer: ValueWriter) -> io::Result<()>;
}

pub trait SmolRead: Sized {
    fn read(reader: ValueReader) -> ReadResult<Self>;
}

macro_rules! impl_smolwrite_primitive {
    ($($ty:ty),* $(,)?) => {
        $(
        impl SmolWrite for $ty {
            fn write(&self, writer: ValueWriter) -> io::Result<()> {
                writer.write_primitive(*self)
            }
        })*
    };
}

impl_smolwrite_primitive!(
    (),
    bool,
    char,
    f32,
    f64,
    i8,
    u8,
    i16,
    u16,
    i32,
    u32,
    i64,
    u64,
    i128,
    u128
);

impl SmolWrite for String {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        writer.write_string(self.deref())
    }
}

impl<T: SmolWrite> SmolWrite for Vec<T> {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        let mut seq = writer.write_seq(Some(self.len()))?;
        for i in self {
            i.write(seq.write_value())?;
        }
        seq.finish()
    }
}

impl<T1: SmolWrite, T2: SmolWrite> SmolWrite for (T1, T2) {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        let mut tup = writer.write_tuple(2)?;

        self.0.write(tup.write_value())?;
        self.1.write(tup.write_value())?;

        Ok(())
    }
}

impl<K: SmolWrite, V: SmolWrite, S> SmolWrite for HashMap<K, V, S> {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        let mut map = writer.write_map(Some(self.len()))?;

        for (k, v) in self {
            let mut pair = map.write_pair();

            k.write(pair.write_key())?;
            v.write(pair.write_value())?;
        }

        map.finish()
    }
}

macro_rules! impl_smolread_primitive {
    ($($ty:ty),* $(,)?) => {
        $(
            impl SmolRead for $ty {
                fn read(reader: ValueReader) -> ReadResult<Self> {
                    let pri = reader.read()?.take_primitive().map_err(ReadError::from)?;
                    let val = pri.try_into().with_type_name_of::<Self>().map_err(ReadError::from)?;
                    Ok(val)
                }
            }
        )*
    };
}

impl_smolread_primitive!(
    (),
    bool,
    char,
    f32,
    f64,
    i8,
    u8,
    i16,
    u16,
    i32,
    u32,
    i64,
    u64,
    i128,
    u128
);

impl SmolRead for String {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        Ok(reader
            .read()?
            .take_string()
            .with_type_name_of::<Self>()
            .map_err(ReadError::from)?
            .read()?
            .to_owned())
    }
}

impl<T: SmolRead> SmolRead for Vec<T> {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        let mut array = reader
            .read()?
            .take_array()
            .with_type_name_of::<Self>()
            .map_err(ReadError::from)?;

        let mut vec = if let Some(rem) = array.remaining() {
            Vec::with_capacity(rem)
        } else {
            Vec::new()
        };

        while let Some(reader) = array.read_value()? {
            vec.push(T::read(reader)?);
        }

        Ok(vec)
    }
}

impl<T1: SmolRead, T2: SmolRead> SmolRead for (T1, T2) {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        let mut tuple = reader
            .read()?
            .take_tuple()
            .with_type_name_of::<Self>()
            .map_err(ReadError::from)?;

        let length = tuple.remaining();
        'read: {
            if length != 2 {
                break 'read;
            }

            let Some(reader) = tuple.read_value() else {
                break 'read;
            };

            let v1 = T1::read(reader)?;

            let Some(reader) = tuple.read_value() else {
                break 'read;
            };

            let v2 = T2::read(reader)?;

            return Ok((v1, v2));
        }

        Err(ReadError::UnexpectedLength {
            expected: 2,
            got: length,
            type_name: type_name::<Self>(),
        }
        .into())
    }
}

impl<K: SmolRead + Hash + Eq, V: SmolRead> SmolRead for HashMap<K, V> {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        let mut map_read = reader
            .read()?
            .take_map()
            .with_type_name_of::<Self>()
            .map_err(ReadError::from)?;

        let mut map = match map_read.remaining() {
            Some(rem) => HashMap::with_capacity(rem),
            None => HashMap::new(),
        };

        while let Some(mut pair) = map_read.read_pair()? {
            let k = K::read(pair.read_key())?;
            let v = V::read(pair.read_value())?;
            map.insert(k, v);
        }

        Ok(map)
    }
}
