use std::{any::type_name, collections::HashMap, hash::Hash, io, ops::Deref};

use smoldata::{
    reader::{ReadError, ReadResult, Reader, UnexpectedValueResultExt, ValueReader},
    writer::{ValueWriter, Writer},
};

#[derive(Debug, PartialEq, Eq)]
enum Enum {
    A(i32),
    B,
    C(String, i32, u32),
    D { v: Vec<u32> },
}

#[derive(PartialEq, Eq, Debug)]
struct Struct {
    values: HashMap<i32, String>,
    e: Vec<Enum>,
    tup: (bool, u128),
}

fn main() {
    let data = Struct {
        values: HashMap::from_iter([
            (0, "somelongstring".into()),
            (1, "somelongstring".into()),
            (2, "somelongstring".into()),
        ]),
        e: vec![
            Enum::D {
                v: vec![0, 5, 10, 15],
            },
            Enum::C("somelongstring".into(), 32, 64),
            Enum::A(11),
            Enum::B,
            Enum::A(0),
            Enum::B,
        ],
        tup: (false, 786583289812096971589793284203998369),
    };

    println!("write: {data:?}");

    let mut vec = vec![];
    let mut writer = Writer::new(&mut vec);
    data.write(writer.write()).unwrap();

    writer.finish();

    println!("written {} bytes:", vec.len());
    hexdump(&vec);

    let mut cursor = io::Cursor::new(&vec);

    let mut reader = Reader::new(&mut cursor);

    let struc = Struct::read(reader.read()).unwrap();

    reader.finish();

    println!("read: {struc:?}");
}

fn hexdump(bytes: &[u8]) {
    for row in 0.. {
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

        if row * 16 >= bytes.len() {
            break;
        }
    }
}

trait SmolWrite {
    fn write(&self, writer: ValueWriter) -> io::Result<()>;
}

trait SmolRead: Sized {
    fn read(reader: ValueReader) -> ReadResult<Self>;
}

impl SmolWrite for Struct {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        let mut struc = writer.write_struct(3)?;

        self.values.write(struc.write_field("values")?)?;
        self.e.write(struc.write_field("e")?)?;
        self.tup.write(struc.write_field("tup")?)?;

        Ok(())
    }
}

impl SmolWrite for Enum {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        match self {
            Enum::A(v) => v.write(writer.write_newtype_variant("A")?),
            Enum::B => writer.write_unit_variant("B"),
            Enum::C(v1, v2, v3) => {
                let mut tup = writer.write_tuple_variant("C", 3)?;
                v1.write(tup.write_value())?;
                v2.write(tup.write_value())?;
                v3.write(tup.write_value())?;
                Ok(())
            }
            Enum::D { v } => {
                let mut struc = writer.write_struct_variant("D", 1)?;
                v.write(struc.write_field("v")?)?;
                Ok(())
            }
        }
    }
}

impl SmolRead for Struct {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        let mut struc = reader
            .read()?
            .take_field_struct()
            .with_type_name_of::<Self>()
            .map_err(ReadError::from)?;

        let mut f_values = None;
        let mut f_e = None;
        let mut f_tup = None;

        while let Some(field) = struc.read_field()? {
            match field.0.deref() {
                "values" => {
                    if f_values.is_some() {
                        return Err(ReadError::DuplicateStructField {
                            name: "values",
                            type_name: type_name::<Self>(),
                        }
                        .into());
                    }
                    f_values = Some(SmolRead::read(field.1)?);
                }

                "e" => {
                    if f_e.is_some() {
                        return Err(ReadError::DuplicateStructField {
                            name: "e",
                            type_name: type_name::<Self>(),
                        }
                        .into());
                    }
                    f_e = Some(SmolRead::read(field.1)?);
                }

                "tup" => {
                    if f_tup.is_some() {
                        return Err(ReadError::DuplicateStructField {
                            name: "tup",
                            type_name: type_name::<Self>(),
                        }
                        .into());
                    }
                    f_tup = Some(SmolRead::read(field.1)?);
                }

                _ => {
                    return Err(ReadError::UnexpectedStructField {
                        name: field.0,
                        type_name: type_name::<Self>(),
                    }
                    .into())
                }
            }
        }

        let f_values = f_values.ok_or_else(|| ReadError::MissingStructField {
            name: "values",
            type_name: type_name::<Self>(),
        })?;

        let f_e = f_e.ok_or_else(|| ReadError::MissingStructField {
            name: "e",
            type_name: type_name::<Self>(),
        })?;

        let f_tup = f_tup.ok_or_else(|| ReadError::MissingStructField {
            name: "tup",
            type_name: type_name::<Self>(),
        })?;

        Ok(Self {
            values: f_values,
            e: f_e,
            tup: f_tup,
        })
    }
}

impl SmolRead for Enum {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        let var = reader
            .read()?
            .take_enum()
            .with_type_name_of::<Self>()
            .map_err(ReadError::from)?
            .read_variant()?;

        Ok(match var.0.deref() {
            "A" => Self::A(SmolRead::read(
                var.1
                    .take_newtype_variant()
                    .with_variant_name(type_name::<Enum>(), "A")
                    .map_err(ReadError::from)?,
            )?),
            "B" => {
                var.1
                    .take_unit_variant()
                    .with_variant_name(type_name::<Enum>(), "B")
                    .map_err(ReadError::from)?;
                Self::B
            }
            "C" => {
                let mut tuple = var
                    .1
                    .take_tuple_variant()
                    .with_variant_name(type_name::<Enum>(), "C")
                    .map_err(ReadError::from)?;

                let length = tuple.remaining();
                'read: {
                    if length != 3 {
                        break 'read;
                    }

                    let Some(reader) = tuple.read_value() else {
                        break 'read;
                    };

                    let v1 = SmolRead::read(reader)?;

                    let Some(reader) = tuple.read_value() else {
                        break 'read;
                    };

                    let v2 = SmolRead::read(reader)?;

                    let Some(reader) = tuple.read_value() else {
                        break 'read;
                    };

                    let v3 = SmolRead::read(reader)?;

                    return Ok(Self::C(v1, v2, v3));
                }

                return Err(ReadError::UnexpectedLength {
                    expected: 3,
                    got: length,
                    type_name: type_name::<Self>(),
                }
                .into());
            }
            "D" => {
                let mut struc = var
                    .1
                    .take_field_variant()
                    .with_variant_name(type_name::<Enum>(), "D")
                    .map_err(ReadError::from)?;

                let mut f_v = None;

                while let Some(field) = struc.read_field()? {
                    match field.0.deref() {
                        "v" => {
                            if f_v.is_some() {
                                return Err(ReadError::DuplicateStructField {
                                    name: "v",
                                    type_name: type_name::<Self>(),
                                }
                                .into());
                            }
                            f_v = Some(SmolRead::read(field.1)?);
                        }
                        _ => {
                            return Err(ReadError::UnexpectedStructField {
                                name: field.0,
                                type_name: type_name::<Self>(),
                            }
                            .into())
                        }
                    }
                }

                let f_v = f_v.ok_or_else(|| ReadError::MissingStructField {
                    name: "v",
                    type_name: type_name::<Self>(),
                })?;

                Self::D { v: f_v }
            }
            _ => {
                return Err(ReadError::UnexpectedEnumVariant {
                    name: var.0,
                    type_name: type_name::<Self>(),
                }
                .into())
            }
        })
    }
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
