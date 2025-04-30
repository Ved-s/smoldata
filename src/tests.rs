use std::{collections::HashMap, fmt, io};

use crate::{SmolRead, SmolReadWrite, SmolWrite};

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

    println!("Deserialized RawValue: {inner:?}\n");

    if inner != data.e {
        panic!("RAWVALUE DATA MISMATCH!");
    }

    let mut re_vec = vec![];

    let mut writer = super::writer::Writer::new(&mut re_vec).unwrap();

    with_raw.write(writer.write()).unwrap();

    println!("Reserialized data bytes:");
    hexdump(&re_vec);
    println!();

    let mut cur = io::Cursor::new(re_vec);

    let mut reader = super::reader::Reader::new(&mut cur).unwrap();

    let reserialized = Struct::read(reader.read()).unwrap();

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

    println!();

    hexdump(&vec);

    let mut cur = io::Cursor::new(vec);

    let mut reader = super::reader::Reader::new(&mut cur).unwrap();

    let re = T::read(reader.read()).unwrap();

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
