use std::{collections::HashMap, fmt, io};

use serde::{ser::SerializeSeq, Deserialize, Serialize};

use crate::{RawValue, FORMAT_VERSION};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Enum {
    A(i32),
    B,
    C(String, i32, u32),
    D {
        v: NoLenSerialize<u32>
    }
}

#[derive(PartialEq, Eq, Debug, Serialize, Deserialize)]
struct Struct {
    values: HashMap<i32, String>,
    e: Vec<Enum>,
    tup: (bool, u128)
}

#[allow(unused)]
#[derive(Debug, Serialize, Deserialize)]
struct StructWithRaw {
    values: HashMap<i32, String>,
    e: RawValue,
    tup: (bool, u128)
}

#[derive(PartialEq, Eq)]
struct NoLenSerialize<V>(Vec<V>);

impl<V: Serialize> Serialize for NoLenSerialize<V> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        
        let mut seq = serializer.serialize_seq(None)?;

        for v in &self.0 {
            seq.serialize_element(v)?;
        }

        seq.end()
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for NoLenSerialize<V> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        Ok(Self(Vec::deserialize(deserializer)?))
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
                v: NoLenSerialize(vec![0, 5, 10, 15])
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
fn test_raw() {
    let data = Struct {
        values: HashMap::from_iter([
            (0, "somelongstring".into()),
            (1, "somelongstring".into()),
            (2, "somelongstring".into()),
        ]),
        e: vec![
            Enum::D {
                v: NoLenSerialize(vec![0, 5, 10, 15])
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
    let mut ser = super::ser::Serializer::new(&mut vec, 256).unwrap();
    data.serialize(&mut ser).unwrap();

    println!("Serialized data:");
    hexdump(&vec);

    let mut de = super::de::Deserializer::new(io::Cursor::new(vec)).unwrap();
    let with_raw = StructWithRaw::deserialize(&mut de).unwrap();

    println!("RawValue data:");
    hexdump(with_raw.e.bytes());

    let mut de = super::de::Deserializer::new_bare(io::Cursor::new(with_raw.e.bytes()), FORMAT_VERSION);
    let inner = Vec::<Enum>::deserialize(&mut de).unwrap();

    println!("Deserialized RawValue: {inner:?}\n");

    let mut re_vec = vec![];
    let mut ser = super::ser::Serializer::new(&mut re_vec, 256).unwrap();
    with_raw.serialize(&mut ser).unwrap();

    println!("Reserialized data bytes:");
    hexdump(&re_vec);

    let mut de = super::de::Deserializer::new(io::Cursor::new(re_vec)).unwrap();
    let reserialized = Struct::deserialize(&mut de).unwrap();

    println!("Reserialized data: {data:?}\n");

    if reserialized != data {
        panic!("DATA MISMATCH!")
    }

    println!("Data matches")

}

fn test_reserialize<'de, T: Serialize + Deserialize<'de> + Eq + fmt::Debug>(data: &T) {
    println!("Data before serializing: {data:?}");

    let mut vec = vec![];

    let mut ser = super::ser::Serializer::new(&mut vec, 256).unwrap();

    data.serialize(&mut ser).unwrap();

    hexdump(&vec);

    let mut de = super::de::Deserializer::new(io::Cursor::new(vec)).unwrap();

    let re = T::deserialize(&mut de).unwrap();

    println!("Data after deserializing: {re:?}");

    if &re != data {
        panic!("DATA DOESN'T MATCH!");
    }
}

fn hexdump(bytes: &[u8]) {
    for row in 0.. {

        print!("  ");

        for col in 0..16 {
            let index = row * 16 + col;
            if index >= bytes.len() {
                print!("   ");
            }
            else {
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
            }
            else {
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

        if row * 16 >= bytes.len() {
            break;
        }
    }
}