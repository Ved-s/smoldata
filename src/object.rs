use std::{collections::HashMap, io, sync::Arc};

use crate::{
    reader::{Primitive, ReadError, ReadResult, StructReading, ValueReader, ValueReading},
    str::SdString,
    writer::{SizedStructWriter, SizedTupleWriter, ValueWriter},
    SmolRead, SmolWrite,
};

#[cfg(feature = "raw_value")]
use crate::RawValue;

pub enum ObjectValue {
    Primitive(Primitive),
    String(SdString),
    Bytes(Vec<u8>),
    Option(Option<Box<Self>>),
    Struct(StructValue),
    Enum(Arc<str>, StructValue),
    Tuple(Vec<ObjectValue>),
    Array(Vec<ObjectValue>),
    Map(Vec<(ObjectValue, ObjectValue)>),
}

impl ObjectValue {
    /// Read data from a reader.<br>
    /// Reader preferred to be buffered, reading does many small reads
    pub fn read_from<R: io::Read>(mut reader: R) -> ReadResult<Self> {
        let mut reader = crate::reader::Reader::new(&mut reader).map_err(ReadError::from)?;
        let obj = Self::read(reader.read())?;
        reader.finish();
        Ok(obj)
    }

    /// Write data into a writer.<br>
    /// Writer preferred to be buffered, serialization does many small writes
    pub fn write_into<W: io::Write>(&self, mut writer: W) -> Result<(), io::Error> {
        let mut writer = crate::writer::Writer::new(&mut writer)?;
        self.write(writer.write())?;
        writer.finish()?;
        Ok(())
    }

    #[cfg(feature = "raw_value")]
    pub fn read_from_raw(raw: &RawValue) -> ReadResult<Self> {
        RawValue::read_object(raw)
    }

    #[cfg(feature = "raw_value")]
    pub fn write_into_raw(&self) -> Result<RawValue, io::Error> {
        RawValue::write_object(self)
    }
}

pub enum StructValue {
    Unit,
    Newtype(Box<ObjectValue>),
    Tuple(Vec<ObjectValue>),
    Struct(HashMap<Arc<str>, ObjectValue>),
}

impl StructValue {
    fn read(reader: StructReading) -> ReadResult<Self> {
        Ok(match reader {
            StructReading::Unit => Self::Unit,
            StructReading::Newtype(v) => Self::Newtype(Box::new(ObjectValue::read(v)?)),
            StructReading::Tuple(mut t) => {
                let mut vec = Vec::with_capacity(t.remaining());
                while let Some(v) = t.read_value() {
                    vec.push(ObjectValue::read(v)?);
                }
                Self::Tuple(vec)
            }
            StructReading::Struct(mut s) => {
                let mut map = HashMap::with_capacity(s.remaining());

                while let Some(f) = s.read_field()? {
                    let v = ObjectValue::read(f.1)?;
                    map.insert(f.0, v);
                }
                Self::Struct(map)
            }
        })
    }
}

impl SmolRead for ObjectValue {
    fn read(reader: ValueReader) -> ReadResult<Self> {
        let reader = reader.read()?;

        Ok(match reader {
            ValueReading::Primitive(p) => Self::Primitive(p),
            ValueReading::String(s) => Self::String(s.read()?),
            ValueReading::Bytes(b) => Self::Bytes(b.read()?),
            ValueReading::Option(o) => Self::Option(o.map(SmolRead::read).transpose()?),
            ValueReading::Struct(s) => Self::Struct(StructValue::read(s)?),
            ValueReading::Enum(e) => {
                let (name, s) = e.read_variant()?;
                Self::Enum(name, StructValue::read(s)?)
            }
            ValueReading::Tuple(mut t) => {
                let mut vec = Vec::with_capacity(t.remaining());
                while let Some(v) = t.read_value() {
                    vec.push(Self::read(v)?);
                }
                Self::Tuple(vec)
            }
            ValueReading::Array(mut a) => {
                let mut vec = match a.remaining() {
                    Some(c) => Vec::with_capacity(c),
                    None => Vec::new(),
                };
                while let Some(v) = a.read_value()? {
                    vec.push(Self::read(v)?);
                }
                Self::Array(vec)
            }
            ValueReading::Map(mut m) => {
                let mut vec = match m.remaining() {
                    Some(c) => Vec::with_capacity(c),
                    None => Vec::new(),
                };
                while let Some(mut p) = m.read_pair()? {
                    let k = Self::read(p.read_key())?;
                    let v = Self::read(p.read_value())?;
                    vec.push((k, v));
                }
                Self::Map(vec)
            }
        })
    }
}

impl SmolWrite for ObjectValue {
    fn write(&self, writer: ValueWriter) -> io::Result<()> {
        fn write_tuple(mut writer: SizedTupleWriter, vals: &[ObjectValue]) -> io::Result<()> {
            for val in vals {
                val.write(writer.write_value())?;
            }
            Ok(())
        }

        fn write_struct(
            mut writer: SizedStructWriter,
            fields: &HashMap<Arc<str>, ObjectValue>,
        ) -> io::Result<()> {
            for (k, v) in fields {
                v.write(writer.write_field(k.clone())?)?;
            }
            Ok(())
        }

        match self {
            ObjectValue::Primitive(p) => writer.write_primitive(*p),
            ObjectValue::String(s) => writer.write_string(s.as_ref_arc()),
            ObjectValue::Bytes(b) => writer.write_bytes(b),
            ObjectValue::Option(None) => writer.write_none(),
            ObjectValue::Option(Some(v)) => v.write(writer.write_some()?),
            ObjectValue::Struct(StructValue::Unit) => writer.write_unit_struct(),
            ObjectValue::Struct(StructValue::Newtype(v)) => v.write(writer.write_newtype_struct()?),
            ObjectValue::Struct(StructValue::Tuple(t)) => {
                write_tuple(writer.write_tuple_struct(t.len())?, t)
            }
            ObjectValue::Struct(StructValue::Struct(s)) => {
                write_struct(writer.write_struct(s.len())?, s)
            }
            ObjectValue::Enum(n, StructValue::Unit) => writer.write_unit_variant(n.clone()),
            ObjectValue::Enum(n, StructValue::Newtype(v)) => {
                v.write(writer.write_newtype_variant(n.clone())?)
            }
            ObjectValue::Enum(n, StructValue::Tuple(t)) => {
                write_tuple(writer.write_tuple_variant(n.clone(), t.len())?, t)
            }
            ObjectValue::Enum(n, StructValue::Struct(s)) => {
                write_struct(writer.write_struct_variant(n.clone(), s.len())?, s)
            }
            ObjectValue::Tuple(t) => write_tuple(writer.write_tuple(t.len())?, t),
            ObjectValue::Array(a) => {
                let mut arr = writer.write_array(Some(a.len()))?;
                for v in a {
                    v.write(arr.write_value())?;
                }
                arr.finish()
            }
            ObjectValue::Map(m) => {
                let mut map = writer.write_map(Some(m.len()))?;
                for (k, v) in m {
                    let mut pair = map.write_pair();
                    k.write(pair.write_key())?;
                    v.write(pair.write_value())?;
                }
                map.finish()
            }
        }
    }
}
