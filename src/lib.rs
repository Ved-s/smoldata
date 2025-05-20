// pub mod de;

#[macro_use]
mod macros;
mod tag;

pub mod varint;

#[cfg(test)]
mod tests;

#[cfg(feature = "raw_value")]
pub mod raw;
pub mod reader;
pub mod str;
pub mod writer;

use std::{
    any::type_name,
    collections::{BTreeMap, HashMap},
    hash::Hash,
    io::{self, ErrorKind},
    ops::Deref, rc::Rc, sync::Arc,
};

#[cfg(feature = "raw_value")]
use raw::RawValue;
use reader::{ReadError, ReadResult, UnexpectedValueResultExt, ValueReader};
use writer::ValueWriter;

pub use smoldata_derive::{SmolRead, SmolReadWrite, SmolWrite};

pub const MAGIC_HEADER: &[u8] = b"sd";
pub const FORMAT_VERSION: u32 = 0;

/// Write data into a writer.<br>
/// Writer preferred to be buffered, serialization does many small writes
pub fn write_into<T: SmolWrite, W: io::Write>(data: &T, mut writer: W) -> Result<(), io::Error> {
    let mut writer = crate::writer::Writer::new(&mut writer)?;
    data.write(writer.write())?;
    writer.finish()?;
    Ok(())
}

/// Write data into a Vec of bytes.
pub fn write_into_bytes<T: SmolWrite>(data: &T) -> Result<Vec<u8>, io::Error> {
    let mut vec = vec![];
    write_into(data, &mut vec)?;
    Ok(vec)
}

#[cfg(feature = "raw_value")]
/// Write data into a RawValue.
pub fn write_into_raw<T: SmolWrite>(data: &T) -> Result<RawValue, io::Error> {
    RawValue::write_object(data)
}

/// Read data from a reader.<br>
/// Reader preferred to be buffered, reading does many small reads
pub fn read_from<T: SmolRead, R: io::Read>(mut reader: R) -> ReadResult<T> {
    let mut reader = crate::reader::Reader::new(&mut reader).map_err(ReadError::from)?;
    let obj = T::read(reader.read())?;
    reader.finish();
    Ok(obj)
}

/// Read data from a slice of bytes.
pub fn read_from_bytes<T: SmolRead>(bytes: &[u8]) -> ReadResult<T> {
    let cur = std::io::Cursor::new(bytes);
    read_from(cur)
}

#[cfg(feature = "raw_value")]
/// Read data from a RawValue.
pub fn from_raw<T: SmolRead>(raw: &RawValue) -> ReadResult<T> {
    raw.read_object()
}

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

pub trait SmolReadWrite: SmolRead + SmolWrite {}
impl<T: SmolRead + SmolWrite> SmolReadWrite for T {}

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

impl<T: SmolWrite> SmolWrite for [T] {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        let mut seq = writer.write_seq(Some(self.len()))?;
        for i in self {
            i.write(seq.write_value())?;
        }
        seq.finish()
    }
}

impl<T: SmolWrite, const N: usize> SmolWrite for [T; N] {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        let mut seq = writer.write_seq(Some(self.len()))?;
        for i in self {
            i.write(seq.write_value())?;
        }
        seq.finish()
    }
}

macro_rules! impl_write_tuple {
    ($len:literal, $($tname:ident $field:tt),*) => {
        impl<$($tname: SmolWrite),*> SmolWrite for ($($tname),*) {
            fn write(&self, writer: ValueWriter) -> io::Result<()> {
                let mut tup = writer.write_tuple($len)?;

                $(
                    self.$field.write(tup.write_value())?;
                )*

                Ok(())
            }
        }
    };
}

impl_write_tuple!(2, T1 0, T2 1);
impl_write_tuple!(3, T1 0, T2 1, T3 2);
impl_write_tuple!(4, T1 0, T2 1, T3 2, T4 3);
impl_write_tuple!(5, T1 0, T2 1, T3 2, T4 3, T5 4);
impl_write_tuple!(6, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5);
impl_write_tuple!(7, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6);
impl_write_tuple!(8, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7);
impl_write_tuple!(9, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8);
impl_write_tuple!(10, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9);
impl_write_tuple!(11, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10);
impl_write_tuple!(12, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10, T12 11);
impl_write_tuple!(13, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10, T12 11, T13 12);
impl_write_tuple!(14, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10, T12 11, T13 12, T14 13);
impl_write_tuple!(15, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10, T12 11, T13 12, T14 13, T15 14);
impl_write_tuple!(16, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10, T12 11, T13 12, T14 13, T15 14, T16 15);

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

impl<K: SmolWrite, V: SmolWrite> SmolWrite for BTreeMap<K, V> {
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

impl<T: SmolWrite> SmolWrite for &mut T {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        <T as SmolWrite>::write(self, writer)
    }
}

impl<T: SmolWrite> SmolWrite for &T {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        <T as SmolWrite>::write(self, writer)
    }
}

impl<T: SmolWrite> SmolWrite for Box<T> {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        <T as SmolWrite>::write(self, writer)
    }
}

impl<T: SmolWrite> SmolWrite for Rc<T> {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        <T as SmolWrite>::write(self, writer)
    }
}

impl<T: SmolWrite> SmolWrite for Arc<T> {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        <T as SmolWrite>::write(self, writer)
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

impl<T: SmolRead, const N: usize> SmolRead for [T; N] {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        let mut array = reader
            .read()?
            .take_array()
            .with_type_name_of::<Self>()
            .map_err(ReadError::from)?;

        if let Some(rem) = array.remaining() {
            if rem != N {
                return Err(ReadError::UnexpectedLength {
                    expected: N,
                    got: rem,
                    type_name: std::any::type_name::<Self>(),
                }.into());
            }
        }

        let mut arr = std::array::from_fn(|_| None);
        let mut i = 0;

        while let Some(reader) = array.read_value()? {
            if i >= arr.len() {
                return Err(ReadError::UnexpectedLength {
                    expected: N,
                    got: i + 1,
                    type_name: std::any::type_name::<Self>(),
                }.into());
            }
            arr[i] = Some(T::read(reader)?);
            i += 1;
        }

        if i != N {
            return Err(ReadError::UnexpectedLength {
                expected: N,
                got: i,
                type_name: std::any::type_name::<Self>(),
            }.into());
        }

        let arr = arr.map(|v| v.expect("sanity"));

        Ok(arr)
    }
}

macro_rules! impl_read_tuple {
    ($len:literal, $($tname:ident $field:tt),*) => {
        impl<$($tname: SmolRead),*> SmolRead for ($($tname),*) {

            #[allow(non_snake_case)]
            fn read(reader: ValueReader) -> ReadResult<Self> {
                let mut tuple = reader
                    .read()?
                    .take_tuple()
                    .with_type_name_of::<Self>()
                    .map_err(ReadError::from)?;

                let length = tuple.remaining();
                'read: {
                    if length != $len {
                        break 'read;
                    }

                    $(
                        let Some(reader) = tuple.read_value() else {
                            break 'read;
                        };

                        let $tname = $tname::read(reader)?;
                    )*

                    return Ok(($($tname),*));
                }

                Err(ReadError::UnexpectedLength {
                    expected: $len,
                    got: length,
                    type_name: type_name::<Self>(),
                }
                .into())
            }
        }
    };
}

impl_read_tuple!(2, T1 0, T2 1);
impl_read_tuple!(3, T1 0, T2 1, T3 2);
impl_read_tuple!(4, T1 0, T2 1, T3 2, T4 3);
impl_read_tuple!(5, T1 0, T2 1, T3 2, T4 3, T5 4);
impl_read_tuple!(6, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5);
impl_read_tuple!(7, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6);
impl_read_tuple!(8, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7);
impl_read_tuple!(9, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8);
impl_read_tuple!(10, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9);
impl_read_tuple!(11, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10);
impl_read_tuple!(12, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10, T12 11);
impl_read_tuple!(13, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10, T12 11, T13 12);
impl_read_tuple!(14, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10, T12 11, T13 12, T14 13);
impl_read_tuple!(15, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10, T12 11, T13 12, T14 13, T15 14);
impl_read_tuple!(16, T1 0, T2 1, T3 2, T4 3, T5 4, T6 5, T7 6, T8 7, T9 8, T10 9, T11 10, T12 11, T13 12, T14 13, T15 14, T16 15);

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

impl<K: SmolRead + Ord, V: SmolRead> SmolRead for BTreeMap<K, V> {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        let mut map_read = reader
            .read()?
            .take_map()
            .with_type_name_of::<Self>()
            .map_err(ReadError::from)?;

        let mut map = BTreeMap::new();

        while let Some(mut pair) = map_read.read_pair()? {
            let k = K::read(pair.read_key())?;
            let v = V::read(pair.read_value())?;
            map.insert(k, v);
        }

        Ok(map)
    }
}

impl<T: SmolRead + Sized> SmolRead for Box<T> {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        <T as SmolRead>::read(reader).map(Into::into)
    }
}

impl<T: SmolRead + Sized> SmolRead for Rc<T> {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        <T as SmolRead>::read(reader).map(Into::into)
    }
}

impl<T: SmolRead + Sized> SmolRead for Arc<T> {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        <T as SmolRead>::read(reader).map(Into::into)
    }
}