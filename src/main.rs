use std::{any::type_name, collections::HashMap, io, ops::Deref};

use smoldata::{SmolRead, SmolReadWrite, SmolWrite};

#[derive(Debug, PartialEq, Eq, SmolReadWrite)]

enum Enum {
    A(i32),
    B,

    #[sd(rename = "Test")]
    C(String, i32, u32),
    D {
        #[sd(rename = "vector")]
        v: Vec<u32>,
    },
}

const ENUM_FIELD_NAME: &str = "enumtestname";

#[derive(PartialEq, Eq, Debug, SmolReadWrite)]
struct Struct {
    values: HashMap<i32, String>,

    #[sd(rename = ENUM_FIELD_NAME)]
    r#enum: Vec<Enum>,

    #[sd(rename = "tuple")]
    tup: (bool, u128),
}

fn main() {
    use smoldata::{reader::Reader, writer::Writer};

    let data = Struct {
        values: HashMap::from_iter([
            (0, "somelongstring".into()),
            (1, "somelongstring".into()),
            (2, "somelongstring".into()),
        ]),
        r#enum: vec![
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

// impl SmolRead for Struct {
//     fn read(reader: ValueReader) -> ReadResult<Self> {
//         let mut struc = reader
//             .read()?
//             .take_field_struct()
//             .with_type_name_of::<Self>()
//             .map_err(ReadError::from)?;

//         let mut f_values = None;
//         let mut f_e = None;
//         let mut f_tup = None;

//         while let Some(field) = struc.read_field()? {
//             match field.0.deref() {
//                 "values" => {
//                     if f_values.is_some() {
//                         return Err(ReadError::DuplicateStructField {
//                             name: "values",
//                             type_name: type_name::<Self>(),
//                         }
//                         .into());
//                     }
//                     f_values = Some(SmolRead::read(field.1)?);
//                 }

//                 "e" => {
//                     if f_e.is_some() {
//                         return Err(ReadError::DuplicateStructField {
//                             name: "e",
//                             type_name: type_name::<Self>(),
//                         }
//                         .into());
//                     }
//                     f_e = Some(SmolRead::read(field.1)?);
//                 }

//                 "tup" => {
//                     if f_tup.is_some() {
//                         return Err(ReadError::DuplicateStructField {
//                             name: "tup",
//                             type_name: type_name::<Self>(),
//                         }
//                         .into());
//                     }
//                     f_tup = Some(SmolRead::read(field.1)?);
//                 }

//                 _ => {
//                     return Err(ReadError::UnexpectedStructField {
//                         name: field.0,
//                         type_name: type_name::<Self>(),
//                     }
//                     .into())
//                 }
//             }
//         }

//         let f_values = f_values.ok_or_else(|| ReadError::MissingStructField {
//             name: "values",
//             type_name: type_name::<Self>(),
//         })?;

//         let f_e = f_e.ok_or_else(|| ReadError::MissingStructField {
//             name: "e",
//             type_name: type_name::<Self>(),
//         })?;

//         let f_tup = f_tup.ok_or_else(|| ReadError::MissingStructField {
//             name: "tup",
//             type_name: type_name::<Self>(),
//         })?;

//         Ok(Self {
//             values: f_values,
//             e: f_e,
//             tup: f_tup,
//         })
//     }
// }

// impl SmolRead for Enum {
//     fn read(reader: ValueReader) -> ReadResult<Self> {
//         let var = reader
//             .read()?
//             .take_enum()
//             .with_type_name_of::<Self>()
//             .map_err(ReadError::from)?
//             .read_variant()?;

//         Ok(match var.0.deref() {
//             "A" => Self::A(SmolRead::read(
//                 var.1
//                     .take_newtype_variant()
//                     .with_variant_name(type_name::<Enum>(), "A")
//                     .map_err(ReadError::from)?,
//             )?),
//             "B" => {
//                 var.1
//                     .take_unit_variant()
//                     .with_variant_name(type_name::<Enum>(), "B")
//                     .map_err(ReadError::from)?;
//                 Self::B
//             }
//             "C" => {
//                 let mut tuple = var
//                     .1
//                     .take_tuple_variant()
//                     .with_variant_name(type_name::<Enum>(), "C")
//                     .map_err(ReadError::from)?;

//                 let length = tuple.remaining();
//                 'read: {
//                     if length != 3 {
//                         break 'read;
//                     }

//                     let Some(reader) = tuple.read_value() else {
//                         break 'read;
//                     };

//                     let v1 = SmolRead::read(reader)?;

//                     let Some(reader) = tuple.read_value() else {
//                         break 'read;
//                     };

//                     let v2 = SmolRead::read(reader)?;

//                     let Some(reader) = tuple.read_value() else {
//                         break 'read;
//                     };

//                     let v3 = SmolRead::read(reader)?;

//                     return Ok(Self::C(v1, v2, v3));
//                 }

//                 return Err(ReadError::UnexpectedLength {
//                     expected: 3,
//                     got: length,
//                     type_name: type_name::<Self>(),
//                 }
//                 .into());
//             }
//             "D" => {
//                 let mut struc = var
//                     .1
//                     .take_field_variant()
//                     .with_variant_name_of::<Enum>("D")
//                     .map_err(ReadError::from)?;

//                 let mut f_v = None;

//                 while let Some(field) = struc.read_field()? {
//                     match field.0.deref() {
//                         "v" => {
//                             if f_v.is_some() {
//                                 return Err(ReadError::DuplicateStructField {
//                                     name: "v",
//                                     type_name: type_name::<Self>(),
//                                 }
//                                 .into());
//                             }
//                             f_v = Some(SmolRead::read(field.1)?);
//                         }
//                         _ => {
//                             return Err(ReadError::UnexpectedStructField {
//                                 name: field.0,
//                                 type_name: type_name::<Self>(),
//                             }
//                             .into())
//                         }
//                     }
//                 }

//                 let f_v = f_v.ok_or_else(|| ReadError::MissingStructField {
//                     name: "v",
//                     type_name: type_name::<Self>(),
//                 })?;

//                 Self::D { v: f_v }
//             }
//             _ => {
//                 return Err(ReadError::UnexpectedEnumVariant {
//                     name: var.0,
//                     type_name: type_name::<Self>(),
//                 }
//                 .into())
//             }
//         })
//     }
// }
