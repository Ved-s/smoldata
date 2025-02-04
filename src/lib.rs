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
pub mod writer;
pub mod str;

use std::io::{self, ErrorKind};

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