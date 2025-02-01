use std::{
    collections::{BTreeSet, HashMap},
    io,
    num::NonZeroUsize,
    ops::Deref,
    sync::Arc,
};

use crate::{
    tag::{FloatWidth, IntWidth, OptionTag, StructType, TypeTag},
    varint::{self, Sign},
    RefArcStr,
};

const MAX_OPT_STR_LEN: usize = 255;

const LEVEL_LOGGING: bool = false;

// TODO: remove `&mut dyn WriterImpl`, replace with direct writer type erased reference

pub struct Writer<W: io::Write> {
    writer: W,
    data: WriterData,
}

struct WriterData {
    string_map: HashMap<Arc<str>, u32>,
    next_string_id: u32,
    finish_parent_levels: BTreeSet<NonZeroUsize>,
    level: usize,
}

struct WriterContext<'a> {
    writer: &'a mut dyn io::Write,
    data: &'a mut WriterData,
}

impl WriterContext<'_> {
    fn child(&mut self) -> WriterContext<'_> {
        WriterContext {
            writer: self.writer,
            data: self.data,
        }
    }
}

#[allow(unused)]
impl<W: io::Write> Writer<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            data: WriterData {
                string_map: Default::default(),
                next_string_id: 0,
                level: 0,
                finish_parent_levels: Default::default(),
            },
        }
    }

    fn ctx(&mut self) -> WriterContext<'_> {
        WriterContext { writer: &mut self.writer, data: &mut self.data }
    }

    pub fn write(&mut self) -> ValueWriter<'_> {
        if self.data.level != 0 {
            panic!("Attempt to begin new root object before finishing children")
        }
        let mut ctx = self.ctx();
        let level = ctx.new_level();
        ValueWriter {
            writer: ctx,
            level,
        }
    }

    pub fn finish(self) -> W {
        if self.data.level != 0 {
            panic!("Attempt to finish before finishing children")
        }

        self.writer
    }
}

impl WriterContext<'_> {
    fn write_tag(&mut self, tag: TypeTag) -> io::Result<()> {
        tag.write(self.writer)
    }

    fn write_str(&mut self, str: RefArcStr) -> io::Result<()> {
        match self.data.string_map.get(str.deref()) {
            Some(r) => {
                varint::write_varint_with_sign(&mut self.writer, *r, Sign::Positive)?;
            }
            None => {
                let index = self.data.next_string_id;
                self.data.next_string_id += 1;
                let arc: Arc<str> = str.into();

                varint::write_varint_with_sign(&mut self.writer, index, Sign::Negative)?;
                varint::write_unsigned_varint(&mut self.writer, arc.len())?;
                self.writer.write_all(arc.as_bytes())?;

                self.data.string_map.insert(arc, index);
            }
        }

        Ok(())
    }

    fn inner(&mut self) -> &mut dyn io::Write {
        &mut self.writer
    }

    fn new_level(&mut self) -> NonZeroUsize {
        self.data.level += 1;

        if LEVEL_LOGGING {
            println!("Begin level {}", self.data.level);
        }

        NonZeroUsize::new(self.data.level).expect("cosmic ray")
    }

    #[allow(clippy::comparison_chain)]
    fn check_level(&self, level: NonZeroUsize) {
        if LEVEL_LOGGING {
            println!("Check level {level} vs {}", self.data.level);
        }

        if level.get() < self.data.level {
            panic!("Attemt to use a Writer before finishing its children")
        } else if level.get() > self.data.level {
            panic!("Attemt to use a Writer after it finished")
        }
    }

    #[allow(clippy::comparison_chain)]
    fn finish_level(&mut self, level: NonZeroUsize) {
        let data = &mut *self.data;
        let deferred = level.get() < data.level;

        if LEVEL_LOGGING {
            println!(
                "Finish level {level} (current: {}, defer: {deferred})",
                data.level
            );
        }

        if deferred {
            data.finish_parent_levels.insert(level);
        } else if data.level > level.get() {
            panic!("Attempted to finish at a wrong layer")
        } else {
            data.level -= 1;
            loop {
                let Some(level) = NonZeroUsize::new(data.level) else {
                    break;
                };

                if !data.finish_parent_levels.remove(&level) {
                    break;
                }

                if LEVEL_LOGGING {
                    println!("Finish deferred level {}", data.level);
                }

                data.level -= 1;
            }
        }
    }
}

pub struct ValueWriter<'a> {
    writer: WriterContext<'a>,
    level: NonZeroUsize,
}

#[allow(unused)]
impl<'a> ValueWriter<'a> {
    pub fn write_primitive<P: Primitive>(mut self, pri: P) -> io::Result<()> {
        self.writer.check_level(self.level);
        pri.write(self.writer.child())?;
        self.writer.finish_level(self.level);
        Ok(())
    }

    pub fn write_string<'s>(mut self, str: impl Into<RefArcStr<'s>>) -> io::Result<()> {
        self.writer.check_level(self.level);
        let str = str.into();
        if str.is_empty() {
            self.writer.write_tag(TypeTag::EmptyStr)?;
        } else if str.len() > MAX_OPT_STR_LEN {
            self.writer.write_tag(TypeTag::StrDirect)?;
            varint::write_unsigned_varint(self.writer.inner(), str.len())?;
            self.writer.inner().write_all(str.as_bytes())?;
        } else {
            self.writer.write_tag(TypeTag::Str)?;
            self.writer.write_str(str)?;
        }

        self.writer.finish_level(self.level);
        Ok(())
    }

    pub fn write_bytes(mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.check_level(self.level);
        self.writer.write_tag(TypeTag::Bytes)?;
        varint::write_unsigned_varint(self.writer.inner(), bytes.len())?;
        self.writer.inner().write_all(bytes)?;
        self.writer.finish_level(self.level);

        Ok(())
    }

    pub fn write_none(mut self) -> io::Result<()> {
        self.writer.check_level(self.level);
        self.writer.write_tag(TypeTag::Option(OptionTag::None))?;
        self.writer.finish_level(self.level);

        Ok(())
    }

    pub fn write_some(mut self) -> io::Result<Self> {
        self.writer.check_level(self.level);
        self.writer.write_tag(TypeTag::Option(OptionTag::Some))?;

        /// Some() stays on the same level
        Ok(self)
    }

    pub fn write_unit_struct(mut self) -> io::Result<()> {
        self.writer.check_level(self.level);
        self.writer.write_tag(TypeTag::Struct(StructType::Unit))?;
        self.writer.finish_level(self.level);

        Ok(())
    }

    pub fn write_newtype_struct(mut self) -> io::Result<Self> {
        self.writer.check_level(self.level);
        self.writer
            .write_tag(TypeTag::Struct(StructType::Newtype))?;

        /// Newtype structs stay on the same level
        Ok(self)
    }

    pub fn write_tuple_struct(mut self, fields: usize) -> io::Result<SizedTupleWriter<'a>> {
        self.writer.check_level(self.level);
        self.writer.write_tag(TypeTag::Struct(StructType::Tuple))?;
        varint::write_unsigned_varint(self.writer.inner(), fields)?;

        if fields == 0 {
            self.writer.finish_level(self.level);
        }

        /// Containers continue on the same level
        Ok(SizedTupleWriter {
            writer: self.writer,
            remaining: fields,
            level: self.level,
        })
    }

    pub fn write_struct(mut self, fields: usize) -> io::Result<SizedStructWriter<'a>> {
        self.writer.check_level(self.level);
        self.writer.write_tag(TypeTag::Struct(StructType::Struct))?;
        varint::write_unsigned_varint(self.writer.inner(), fields)?;

        if fields == 0 {
            self.writer.finish_level(self.level);
        }

        /// Containers continue on the same level
        Ok(SizedStructWriter {
            writer: self.writer,
            remaining: fields,
            level: self.level,
        })
    }

    pub fn write_unit_variant<'n>(mut self, variant: impl Into<RefArcStr<'n>>) -> io::Result<()> {
        self.writer.check_level(self.level);
        self.writer
            .write_tag(TypeTag::EnumVariant(StructType::Unit))?;
        self.writer.write_str(variant.into())?;
        self.writer.finish_level(self.level);

        Ok(())
    }

    pub fn write_newtype_variant<'n>(
        mut self,
        variant: impl Into<RefArcStr<'n>>,
    ) -> io::Result<Self> {
        self.writer.check_level(self.level);
        self.writer
            .write_tag(TypeTag::EnumVariant(StructType::Newtype))?;
        self.writer.write_str(variant.into())?;

        /// Newtype variants stay on the same level
        Ok(self)
    }

    pub fn write_tuple_variant<'n>(
        mut self,
        variant: impl Into<RefArcStr<'n>>,
        fields: usize,
    ) -> io::Result<SizedTupleWriter<'a>> {
        self.writer.check_level(self.level);
        self.writer
            .write_tag(TypeTag::EnumVariant(StructType::Tuple))?;
        self.writer.write_str(variant.into())?;
        varint::write_unsigned_varint(self.writer.inner(), fields)?;

        if fields == 0 {
            self.writer.finish_level(self.level);
        }

        /// Containers stay on the same level
        Ok(SizedTupleWriter {
            writer: self.writer,
            level: self.level,
            remaining: fields,
        })
    }

    pub fn write_struct_variant<'n>(
        mut self,
        variant: impl Into<RefArcStr<'n>>,
        fields: usize,
    ) -> io::Result<SizedStructWriter<'a>> {
        self.writer.check_level(self.level);
        self.writer
            .write_tag(TypeTag::EnumVariant(StructType::Tuple))?;
        self.writer.write_str(variant.into())?;
        varint::write_unsigned_varint(self.writer.inner(), fields)?;

        if fields == 0 {
            self.writer.finish_level(self.level);
        }

        /// Containers stay on the same level
        Ok(SizedStructWriter {
            writer: self.writer,
            level: self.level,
            remaining: fields,
        })
    }

    pub fn write_tuple(mut self, fields: usize) -> io::Result<SizedTupleWriter<'a>> {
        self.writer.check_level(self.level);
        self.writer.write_tag(TypeTag::Tuple)?;
        varint::write_unsigned_varint(self.writer.inner(), fields)?;

        if fields == 0 {
            self.writer.finish_level(self.level);
        }

        /// Containers continue on the same level
        Ok(SizedTupleWriter {
            writer: self.writer,
            remaining: fields,
            level: self.level,
        })
    }

    pub fn write_seq(mut self, len: Option<usize>) -> io::Result<ArrayWriter<'a>> {
        self.writer.check_level(self.level);
        self.writer.write_tag(TypeTag::Array {
            has_length: len.is_some(),
        })?;

        if let Some(len) = len {
            varint::write_unsigned_varint(self.writer.inner(), len)?;
        }

        if len == Some(0) {
            self.writer.finish_level(self.level);
        }

        Ok(ArrayWriter {
            writer: self.writer,
            level: self.level,
            remaining: len,
        })
    }

    pub fn write_map(mut self, len: Option<usize>) -> io::Result<MapWriter<'a>> {
        self.writer.check_level(self.level);
        self.writer.write_tag(TypeTag::Map {
            has_length: len.is_some(),
        })?;

        if let Some(len) = len {
            varint::write_unsigned_varint(self.writer.inner(), len)?;
        }

        if len == Some(0) {
            self.writer.finish_level(self.level);
        }

        Ok(MapWriter {
            writer: self.writer,
            level: self.level,
            remaining: len,
        })
    }
}

pub struct SizedTupleWriter<'a> {
    writer: WriterContext<'a>,
    remaining: usize,
    level: NonZeroUsize,
}

impl SizedTupleWriter<'_> {
    pub fn write_value(&mut self) -> ValueWriter<'_> {
        self.writer.check_level(self.level);
        if self.remaining == 0 {
            panic!("Attempt to add more values to the tuple than specified")
        }

        self.remaining -= 1;

        let level = self.writer.new_level();
        if self.remaining == 0 {
            self.writer.finish_level(self.level);
        }

        ValueWriter {
            writer: self.writer.child(),
            level,
        }
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }
}

pub struct SizedStructWriter<'a> {
    writer: WriterContext<'a>,
    remaining: usize,
    level: NonZeroUsize,
}

impl SizedStructWriter<'_> {
    pub fn write_field<'n>(&mut self, name: impl Into<RefArcStr<'n>>) -> io::Result<ValueWriter> {
        self.writer.check_level(self.level);
        if self.remaining == 0 {
            panic!("Attempt to add more fields to the map than specified")
        }

        self.remaining -= 1;

        self.writer.write_str(name.into())?;

        let level = self.writer.new_level();
        if self.remaining == 0 {
            self.writer.finish_level(self.level);
        }

        Ok(ValueWriter {
            writer: self.writer.child(),
            level,
        })
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }
}

pub struct ArrayWriter<'a> {
    writer: WriterContext<'a>,
    remaining: Option<usize>,
    level: NonZeroUsize,
}

impl ArrayWriter<'_> {
    pub fn write_value(&mut self) -> ValueWriter {
        self.writer.check_level(self.level);
        if self.remaining == Some(0) {
            panic!("Attempt to add more values to the seq than specified")
        }

        if let Some(remaining) = &mut self.remaining {
            *remaining -= 1;
        }

        let level = self.writer.new_level();
        if self.remaining == Some(0) {
            self.writer.finish_level(self.level);
        }

        ValueWriter {
            writer: self.writer.child(),
            level,
        }
    }

    pub fn finish(mut self) -> io::Result<()> {
        match self.remaining {
            Some(0) => Ok(()),
            Some(_) => panic!("Attempt to finish before adding all specified values"),
            None => {
                self.writer.write_tag(TypeTag::End)?;
                self.writer.finish_level(self.level);
                Ok(())
            }
        }
    }

    pub fn remaining(&self) -> Option<usize> {
        self.remaining
    }
}

pub struct MapWriter<'a> {
    writer: WriterContext<'a>,
    remaining: Option<usize>,
    level: NonZeroUsize,
}

impl MapWriter<'_> {
    pub fn write_pair(&mut self) -> MapPairWtiter {
        self.writer.check_level(self.level);
        if self.remaining == Some(0) {
            panic!("Attempt to add more values to the seq than specified")
        }

        if let Some(remaining) = &mut self.remaining {
            *remaining -= 1;
        }

        let level = self.writer.new_level();
        if self.remaining == Some(0) {
            self.writer.finish_level(self.level);
        }

        MapPairWtiter {
            writer: self.writer.child(),
            key_done: false,
            level,
        }
    }

    pub fn finish(mut self) -> io::Result<()> {
        match self.remaining {
            Some(0) => Ok(()),
            Some(_) => panic!("Attempt to finish before adding all specified values"),
            None => {
                self.writer.write_tag(TypeTag::End)?;
                self.writer.finish_level(self.level);
                Ok(())
            }
        }
    }

    pub fn remaining(&self) -> Option<usize> {
        self.remaining
    }
}

pub struct MapPairWtiter<'a> {
    writer: WriterContext<'a>,
    key_done: bool,
    level: NonZeroUsize,
}

impl<'a> MapPairWtiter<'a> {
    pub fn write_key(&mut self) -> ValueWriter {
        self.writer.check_level(self.level);
        if self.key_done {
            panic!("Attempt to write duplicate map key")
        }

        let level = self.writer.new_level();

        ValueWriter {
            writer: self.writer.child(),
            level,
        }
    }

    pub fn write_value(self) -> ValueWriter<'a> {
        if !self.key_done {
            panic!("Attempt to write map value before key")
        }
        self.writer.check_level(self.level);
        ValueWriter {
            writer: self.writer,
            level: self.level,
        }
    }
}

#[allow(private_bounds)]
pub trait Primitive: PrimitiveImpl {}

impl<P: PrimitiveImpl> Primitive for P {}

trait PrimitiveImpl: Copy {
    fn write(self, writer: WriterContext) -> io::Result<()>;
}

impl PrimitiveImpl for () {
    fn write(self, mut writer: WriterContext) -> io::Result<()> {
        writer.write_tag(TypeTag::Unit)
    }
}

impl PrimitiveImpl for u8 {
    fn write(self, mut writer: WriterContext) -> io::Result<()> {
        writer.write_tag(TypeTag::Integer {
            width: IntWidth::W8,
            signed: false,
            varint: false,
        })?;
        writer.inner().write_all(&[self])?;
        Ok(())
    }
}

impl PrimitiveImpl for i8 {
    fn write(self, mut writer: WriterContext) -> io::Result<()> {
        writer.write_tag(TypeTag::Integer {
            width: IntWidth::W8,
            signed: true,
            varint: false,
        })?;
        writer.inner().write_all(&[self as u8])?;
        Ok(())
    }
}

impl PrimitiveImpl for bool {
    fn write(self, mut writer: WriterContext) -> io::Result<()> {
        writer.write_tag(TypeTag::Bool(self))?;
        Ok(())
    }
}

impl PrimitiveImpl for char {
    fn write(self, mut writer: WriterContext) -> io::Result<()> {
        let v = self as u32;

        let varint = varint::is_varint_better(v.leading_zeros(), 4, true);
        writer.write_tag(TypeTag::Char { varint })?;

        if varint {
            varint::write_unsigned_varint(writer.inner(), v)?;
        } else {
            writer.inner().write_all(&v.to_le_bytes())?;
        }

        Ok(())
    }
}

impl PrimitiveImpl for f32 {
    fn write(self, mut writer: WriterContext) -> io::Result<()> {
        writer.write_tag(TypeTag::Float(FloatWidth::F32))?;
        writer.inner().write_all(&self.to_le_bytes())?;
        Ok(())
    }
}

impl PrimitiveImpl for f64 {
    fn write(self, mut writer: WriterContext) -> io::Result<()> {
        writer.write_tag(TypeTag::Float(FloatWidth::F64))?;
        writer.inner().write_all(&self.to_le_bytes())?;
        Ok(())
    }
}

macro_rules! impl_primitive {
    (@leading_zeros $self:ident true) => {
        $self.unsigned_abs().leading_zeros()
    };
    (@leading_zeros $self:ident false) => {
        $self.leading_zeros()
    };
    (@write_varint $writer:ident $self:ident true) => {
        varint::write_signed_varint($writer.inner(), $self)?;
    };
    (@write_varint $writer:ident $self:ident false) => {
        varint::write_unsigned_varint($writer.inner(), $self)?;
    };
    ($ty:ident $width:literal $signed:ident $formatwidth:ident) => {
        impl PrimitiveImpl for $ty {
            fn write(self, mut writer: WriterContext) -> io::Result<()> {
                let varint = varint::is_varint_better(impl_primitive!(@leading_zeros self $signed), $width, $signed);
                writer.write_tag(TypeTag::Integer {
                    width: IntWidth::$formatwidth,
                    signed: $signed,
                    varint,
                })?;
                if varint {
                    impl_primitive!(@write_varint writer self $signed);
                } else {
                    writer.inner().write_all(&self.to_le_bytes())?;
                }
                Ok(())
            }
        }
    };
}

impl_primitive!(u16 2 false W16);
impl_primitive!(i16 2 true W16);

impl_primitive!(u32 4 false W32);
impl_primitive!(i32 4 true W32);

impl_primitive!(u64 8 false W64);
impl_primitive!(i64 8 true W64);

impl_primitive!(u128 16 false W128);
impl_primitive!(i128 16 true W128);
