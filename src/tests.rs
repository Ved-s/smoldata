use std::{borrow::Cow, collections::HashMap, fmt, io, ops::Deref};

use crate::{reader::{ReadError, UnexpectedValueResultExt}, SmolRead, SmolReadWrite, SmolWrite, FORMAT_VERSION};

#[cfg(feature = "raw_value")]
use crate::raw::RawValue;

const VECTOR_FIELD_NAME: &str = "vector";

#[derive(Debug, SmolReadWrite, PartialEq, Eq)]
#[sd(smoldata = crate)]
enum Enum {
    A(i32),
    B,

    #[sd(rename = "See")]
    C(String, i32, u32),

    D {
        #[sd(rename = VECTOR_FIELD_NAME)]
        v: NoLenSerialize<u32>,
    },
}

#[derive(PartialEq, Eq, Debug, SmolReadWrite)]
#[sd(smoldata = crate)]
struct Struct {
    values: HashMap<i32, String>,
    e: Vec<Enum>,
    tup: (bool, u128),
}

#[allow(unused)]
#[cfg(feature = "raw_value")]
#[derive(Debug, SmolReadWrite)]
#[sd(smoldata = crate)]
struct StructWithRaw {
    values: HashMap<i32, String>,
    e: RawValue,
    tup: (bool, u128),
}

#[derive(PartialEq, Eq)]
struct NoLenSerialize<V>(Vec<V>);

impl<V: SmolWrite> SmolWrite for NoLenSerialize<V> {
    fn write(&self, writer: crate::writer::ValueWriter) -> io::Result<()> {
        let mut writer = writer.write_seq(None)?;

        for i in &self.0 {
            i.write(writer.write_value())?;
        }

        writer.finish()
    }
}

impl<V: SmolRead> SmolRead for NoLenSerialize<V> {
    fn read(reader: crate::reader::ValueReader) -> crate::reader::ReadResult<Self> {
        Vec::read(reader).map(Self)
    }
}

impl<V: fmt::Debug> fmt::Debug for NoLenSerialize<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Bytes<'a>(pub Cow<'a, [u8]>);

impl SmolWrite for Bytes<'_> {
    fn write(&self, writer: crate::writer::ValueWriter) -> io::Result<()> {
        writer.write_bytes(self.0.deref())
    }
}

impl SmolRead for Bytes<'_> {
    fn read(reader: crate::reader::ValueReader) -> crate::reader::ReadResult<Self> {
        let bytes = reader.read()?.take_bytes().with_type_name_of::<Self>().map_err(ReadError::from)?;
        Ok(Self(Cow::Owned(bytes.read()?)))
    }
}

#[test]
fn test_reserialize_complex() {
    let data = Struct {
        values: HashMap::from_iter([
            (0, "somelongstring".into()),
            (1, "somelongstring".into()),
            (2, "somelongstring".into()),
        ]),
        e: vec![
            Enum::D {
                v: NoLenSerialize(vec![0, 5, 10, 15]),
            },
            Enum::C("somelongstring".into(), 32, 64),
            Enum::A(11),
            Enum::B,
            Enum::A(0),
            Enum::B,
        ],
        tup: (false, 786583289812096971589793284203998369),
    };
    test_reserialize(&data);
}

#[test]
fn test_repeats() {
    let data = vec![0, 0, 0, 0, 0, 8, 8, 8, 8, 4, 4, 4, 4, 4, 4, 4, 4, 4];

    do_test_repeats(&data, true);

    let data: Vec<Option<i32>> = vec![None, None, None, None, None, None, None, None, None, None];

    do_test_repeats(&data, true);

    let data = vec![
        Enum::C("".into(), 0, 0),
        Enum::C("".into(), 0, 0),
        Enum::B,
        Enum::B,
        Enum::B,
        Enum::B,
        Enum::B,
        Enum::B,
        Enum::B,
        Enum::B,
        Enum::B,
        Enum::B,
        Enum::B,
        Enum::B,
        Enum::C("".into(), 0, 0),
    ];

    do_test_repeats(&data, false);

    let d: [u8; 3] = [0x80, 0x99, 0xff];
    let data = [Bytes(Cow::Borrowed(&d)), Bytes(Cow::Borrowed(&d)), Bytes(Cow::Borrowed(&d)), Bytes(Cow::Borrowed(&d)), Bytes(Cow::Borrowed(&d))];

    do_test_repeats(&data, false);
}

fn do_test_repeats<T: Eq + std::fmt::Debug + SmolReadWrite>(data: &[T], test_smaller: bool) {
    println!("Vector: {data:?}");

    let mut vec = vec![];

    let mut writer = super::writer::Writer::new_headerless(&mut vec);

    data.write(writer.write()).unwrap();

    writer.finish().unwrap();

    println!();
    println!(
        "Vector size: {}, serialized size: {}",
        data.len(),
        vec.len()
    );
    println!();
    
    hexdump(&vec);
    
    println!();

    if test_smaller {
        assert!(vec.len() < data.len());
    }

    let mut cur = io::Cursor::new(vec);

    let mut reader = super::reader::Reader::new_headerless(&mut cur, FORMAT_VERSION);

    let re = Vec::<T>::read(reader.read()).unwrap();

    reader.finish();

    assert_eq!(data, re);
}

#[test]
#[cfg(feature = "raw_value")]
fn test_raw() {
    use crate::FORMAT_VERSION;

    let data = Struct {
        values: HashMap::from_iter([
            (0, "somelongstring".into()),
            (1, "somelongstring".into()),
            (2, "somelongstring".into()),
        ]),
        e: vec![
            Enum::D {
                v: NoLenSerialize(vec![0, 5, 10, 15]),
            },
            Enum::C("somelongstring".into(), 32, 64),
            Enum::A(11),
            Enum::B,
            Enum::A(0),
            Enum::B,
        ],
        tup: (false, 786583289812096971589793284203998369),
    };

    println!("Starting data: {data:?}\n");

    let mut vec = vec![];

    let mut writer = super::writer::Writer::new(&mut vec).unwrap();

    data.write(writer.write()).unwrap();

    writer.finish().unwrap();

    println!("Serialized data:");
    hexdump(&vec);
    println!();

    let mut cur = io::Cursor::new(vec);

    let mut reader = super::reader::Reader::new(&mut cur).unwrap();

    let with_raw = StructWithRaw::read(reader.read()).unwrap();

    println!("RawValue data:");
    hexdump(with_raw.e.bytes());
    println!();

    let mut cur = io::Cursor::new(with_raw.e.bytes());

    let mut reader = super::reader::Reader::new_headerless(&mut cur, FORMAT_VERSION);

    let inner = Vec::<Enum>::read(reader.read()).unwrap();

    reader.finish();

    println!("Deserialized RawValue: {inner:?}\n");

    if inner != data.e {
        panic!("RAWVALUE DATA MISMATCH!");
    }

    let mut re_vec = vec![];

    let mut writer = super::writer::Writer::new(&mut re_vec).unwrap();

    with_raw.write(writer.write()).unwrap();

    writer.finish().unwrap();

    println!("Reserialized data bytes:");
    hexdump(&re_vec);
    println!();

    let mut cur = io::Cursor::new(re_vec);

    let mut reader = super::reader::Reader::new(&mut cur).unwrap();

    let reserialized = Struct::read(reader.read()).unwrap();
    reader.finish();

    println!("Reserialized data: {data:?}\n");

    if reserialized != data {
        panic!("DATA MISMATCH!");
    }

    println!("Data matches");
}

fn test_reserialize<T: SmolReadWrite + Eq + fmt::Debug>(data: &T) {
    println!("Data before serializing: {data:?}");

    let mut vec = vec![];

    let mut writer = super::writer::Writer::new(&mut vec).unwrap();

    data.write(writer.write()).unwrap();

    writer.finish().unwrap();

    println!();

    hexdump(&vec);

    let mut cur = io::Cursor::new(vec);

    let mut reader = super::reader::Reader::new(&mut cur).unwrap();

    let re = T::read(reader.read()).unwrap();

    reader.finish();

    println!();

    println!("Data after deserializing: {re:?}");

    if &re != data {
        panic!("DATA DOESN'T MATCH!");
    }
}

fn hexdump(bytes: &[u8]) {
    for row in 0.. {
        if row * 16 >= bytes.len() {
            break;
        }

        print!("  ");

        for col in 0..16 {
            let index = row * 16 + col;
            if index >= bytes.len() {
                print!("   ");
            } else {
                print!("{:02x} ", bytes[index]);
            }

            if col % 4 == 3 {
                print!(" ");
            }
        }

        for col in 0..16 {
            let index = row * 16 + col;
            if index >= bytes.len() {
                print!(" ");
            } else {
                let char = char::from_u32(bytes[index] as u32);
                let char = char.unwrap_or('.');
                let char = if char.is_control() { '.' } else { char };
                print!("{char}");
            }

            if col % 8 == 7 {
                print!(" ");
            }
        }

        println!();
    }
}
