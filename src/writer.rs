use std::{
    collections::HashMap,
    io,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use crate::{
    str::RefArcStr,
    tag::{FloatWidth, IntWidth, OptionTag, StructType, TypeTag},
    varint::{self, Sign},
};

#[cfg(smoldata_int_dev_error_checks)]
use std::{num::NonZeroUsize, collections::BTreeSet};

const MAX_OPT_STR_LEN: usize = 255;

pub struct Writer<'a> {
    writer: &'a mut dyn io::Write,
    string_map: HashMap<Arc<str>, u32>,
    next_string_id: u32,

    #[cfg(smoldata_int_dev_error_checks)]
    finish_parent_levels: BTreeSet<NonZeroUsize>,

    #[cfg(smoldata_int_dev_error_checks)]
    level: usize,
}

#[allow(unused)]
impl<'a> Writer<'a> {
    pub fn new(writer: &'a mut dyn io::Write) -> Self {
        Self {
            writer,
            string_map: Default::default(),
            next_string_id: 0,

            #[cfg(smoldata_int_dev_error_checks)]
            finish_parent_levels: Default::default(),

            #[cfg(smoldata_int_dev_error_checks)]
            level: 0,
        }
    }

    #[track_caller]
    pub fn write(&mut self) -> ValueWriter<'_, 'a> {
        #[cfg(smoldata_int_dev_error_checks)]
        let level = {
            if self.level != 0 {
                panic!("Attempt to begin writing new root object before finishing children")
            }
            self.level += 1;
            NonZeroUsize::new(self.level).expect("cosmic ray")
        };
        ValueWriter {
            writer: WriterLevel {
                writer: self,

                #[cfg(smoldata_int_dev_error_checks)]
                level: Some(level),
            },
        }
    }

    #[track_caller]
    pub fn finish(self) -> &'a mut dyn io::Write {
        #[cfg(smoldata_int_dev_error_checks)]
        if self.level != 0 {
            panic!("Attempt to finish before finishing children")
        }

        self.writer
    }
}

struct WriterLevel<'rf, 'wr> {
    writer: &'rf mut Writer<'wr>,

    #[cfg(smoldata_int_dev_error_checks)]
    level: Option<NonZeroUsize>,
}

impl<'wr> WriterLevel<'_, 'wr> {
    #[track_caller]
    fn get(&mut self) -> WriterRef<'_, 'wr> {
        #[cfg(smoldata_int_dev_error_checks)]
        if self.level.is_some_and(|l| l.get() < self.writer.level) {
            panic!("Attempt to use a Writer before finishing its children")
        } else if self.level.is_none_or(|l| l.get() > self.writer.level) {
            panic!("Attempt to use a Writer after it finished")
        }
        WriterRef {
            writer: self.writer,
        }
    }

    #[track_caller]
    fn finish(&mut self) {
        #[cfg(smoldata_int_dev_error_checks)]
        {
            let level = match self.level {
                None => panic!("Attempted to finish already finished writer"),
                Some(l) if l.get() > self.writer.level => {
                    panic!("Attempted to finish already finished writer")
                }
                Some(l) => l,
            };

            if level.get() < self.writer.level {
                self.writer.finish_parent_levels.insert(level);
            } else {
                self.writer.level -= 1;
                loop {
                    let Some(level) = NonZeroUsize::new(self.writer.level) else {
                        break;
                    };

                    if !self.writer.finish_parent_levels.remove(&level) {
                        break;
                    }

                    self.writer.level -= 1;
                }
            }

            self.level = None;
        }
    }

    /// Begin a new writer below this one
    #[track_caller]
    fn begin_sub_level(&mut self) -> WriterLevel<'_, 'wr> {
        #[cfg(smoldata_int_dev_error_checks)]
        let level = {
            let level = match self.level {
                None => panic!("Attempt to begin a new sub-writer from a finished writer"),
                Some(l) if l.get() > self.writer.level => {
                    panic!("Attempt to begin a new sub-writer from a finished writer")
                }
                Some(l) => l,
            };

            self.writer.level += 1;
            level.checked_add(1).expect("too deep")
        };
        WriterLevel {
            writer: self.writer,

            #[cfg(smoldata_int_dev_error_checks)]
            level: Some(level),
        }
    }

    /// Finish this writer and continue current level on a new one
    #[track_caller]
    fn continue_level(&mut self) -> WriterLevel<'_, 'wr> {
        #[cfg(smoldata_int_dev_error_checks)]
        let level = {
            let level = match self.level {
                None => panic!("Attempt to continue level from a finished writer"),
                Some(l) if l.get() > self.writer.level => {
                    panic!("Attempt to continue level from a finished writer")
                }
                Some(l) => l,
            };

            self.level = None;
            level
        };

        WriterLevel {
            writer: self.writer,

            #[cfg(smoldata_int_dev_error_checks)]
            level: Some(level),
        }
    }
}

struct WriterRef<'rf, 'wr> {
    writer: &'rf mut Writer<'wr>,
}

#[allow(unused)]
impl<'wr> WriterRef<'_, 'wr> {
    fn write_tag(&mut self, tag: TypeTag) -> io::Result<()> {
        tag.write(self.deref_mut())
    }

    fn write_str(&mut self, str: RefArcStr) -> io::Result<()> {
        match self.writer.string_map.get(str.deref()) {
            Some(r) => {
                varint::write_varint_with_sign(&mut self.writer.writer, *r, Sign::Positive)?;
            }
            None => {
                let index = self.writer.next_string_id;
                self.writer.next_string_id += 1;
                let arc: Arc<str> = str.into();

                varint::write_varint_with_sign(&mut self.writer.writer, index, Sign::Negative)?;
                varint::write_unsigned_varint(&mut self.writer.writer, arc.len())?;
                self.writer.writer.write_all(arc.as_bytes())?;

                self.writer.string_map.insert(arc, index);
            }
        }

        Ok(())
    }

    fn inner(&mut self) -> &mut dyn io::Write {
        &mut self.writer.writer
    }

    fn clone(&mut self) -> WriterRef<'_, 'wr> {
        WriterRef {
            writer: self.writer,
        }
    }
}

impl<'wr> Deref for WriterRef<'_, 'wr> {
    type Target = dyn io::Write + 'wr;

    fn deref(&self) -> &Self::Target {
        self.writer.writer
    }
}

impl DerefMut for WriterRef<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.writer.writer
    }
}

pub struct ValueWriter<'rf, 'wr> {
    writer: WriterLevel<'rf, 'wr>,
}

#[allow(unused)]
impl<'rf, 'wr> ValueWriter<'rf, 'wr> {
    #[track_caller]
    pub fn write_primitive<P: Primitive>(mut self, pri: P) -> io::Result<()> {
        let mut writer = self.writer.get();
        pri.write(writer)?;
        self.writer.finish();
        Ok(())
    }

    #[track_caller]
    pub fn write_string<'s>(mut self, str: impl Into<RefArcStr<'s>>) -> io::Result<()> {
        let mut writer = self.writer.get();
        let str = str.into();
        if str.is_empty() {
            writer.write_tag(TypeTag::EmptyStr)?;
        } else if str.len() > MAX_OPT_STR_LEN {
            writer.write_tag(TypeTag::StrDirect)?;
            varint::write_unsigned_varint(writer.inner(), str.len())?;
            writer.write_all(str.as_bytes())?;
        } else {
            writer.write_tag(TypeTag::Str)?;
            writer.write_str(str)?;
        }

        self.writer.finish();
        Ok(())
    }

    #[track_caller]
    pub fn write_bytes(mut self, bytes: &[u8]) -> io::Result<()> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::Bytes)?;
        varint::write_unsigned_varint(writer.inner(), bytes.len())?;
        writer.write_all(bytes)?;
        self.writer.finish();

        Ok(())
    }

    #[track_caller]
    pub fn write_none(mut self) -> io::Result<()> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::Option(OptionTag::None))?;
        self.writer.finish();

        Ok(())
    }

    #[track_caller]
    pub fn write_some(mut self) -> io::Result<Self> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::Option(OptionTag::Some))?;

        /// Some() stays on the same level
        Ok(self)
    }

    #[track_caller]
    pub fn write_unit_struct(mut self) -> io::Result<()> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::Struct(StructType::Unit))?;
        self.writer.finish();

        Ok(())
    }

    #[track_caller]
    pub fn write_newtype_struct(mut self) -> io::Result<Self> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::Struct(StructType::Newtype))?;

        /// Newtype structs stay on the same level
        Ok(self)
    }

    #[track_caller]
    pub fn write_tuple_struct(mut self, fields: usize) -> io::Result<SizedTupleWriter<'rf, 'wr>> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::Struct(StructType::Tuple))?;
        varint::write_unsigned_varint(writer.inner(), fields)?;

        if fields == 0 {
            self.writer.finish();
        }

        /// Containers continue on the same level
        Ok(SizedTupleWriter {
            writer: self.writer,
            remaining: fields,
        })
    }

    #[track_caller]
    pub fn write_struct(mut self, fields: usize) -> io::Result<SizedStructWriter<'rf, 'wr>> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::Struct(StructType::Struct))?;
        varint::write_unsigned_varint(writer.inner(), fields)?;

        if fields == 0 {
            self.writer.finish();
        }

        /// Containers continue on the same level
        Ok(SizedStructWriter {
            writer: self.writer,
            remaining: fields,
        })
    }

    #[track_caller]
    pub fn write_unit_variant<'n>(mut self, variant: impl Into<RefArcStr<'n>>) -> io::Result<()> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::EnumVariant(StructType::Unit))?;
        writer.write_str(variant.into())?;
        self.writer.finish();

        Ok(())
    }

    #[track_caller]
    pub fn write_newtype_variant<'n>(
        mut self,
        variant: impl Into<RefArcStr<'n>>,
    ) -> io::Result<Self> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::EnumVariant(StructType::Newtype))?;
        writer.write_str(variant.into())?;

        /// Newtype variants stay on the same level
        Ok(self)
    }

    #[track_caller]
    pub fn write_tuple_variant<'n>(
        mut self,
        variant: impl Into<RefArcStr<'n>>,
        fields: usize,
    ) -> io::Result<SizedTupleWriter<'rf, 'wr>> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::EnumVariant(StructType::Tuple))?;
        writer.write_str(variant.into())?;
        varint::write_unsigned_varint(writer.inner(), fields)?;

        if fields == 0 {
            self.writer.finish();
        }

        /// Containers stay on the same level
        Ok(SizedTupleWriter {
            writer: self.writer,
            remaining: fields,
        })
    }

    #[track_caller]
    pub fn write_struct_variant<'n>(
        mut self,
        variant: impl Into<RefArcStr<'n>>,
        fields: usize,
    ) -> io::Result<SizedStructWriter<'rf, 'wr>> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::EnumVariant(StructType::Struct))?;
        writer.write_str(variant.into())?;
        varint::write_unsigned_varint(writer.inner(), fields)?;

        if fields == 0 {
            self.writer.finish();
        }

        /// Containers stay on the same level
        Ok(SizedStructWriter {
            writer: self.writer,
            remaining: fields,
        })
    }

    #[track_caller]
    pub fn write_tuple(mut self, fields: usize) -> io::Result<SizedTupleWriter<'rf, 'wr>> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::Tuple)?;
        varint::write_unsigned_varint(writer.inner(), fields)?;

        if fields == 0 {
            self.writer.finish();
        }

        /// Containers continue on the same level
        Ok(SizedTupleWriter {
            writer: self.writer,
            remaining: fields,
        })
    }

    #[track_caller]
    pub fn write_seq(mut self, len: Option<usize>) -> io::Result<ArrayWriter<'rf, 'wr>> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::Array {
            has_length: len.is_some(),
        })?;

        if let Some(len) = len {
            varint::write_unsigned_varint(writer.inner(), len)?;
        }

        if len == Some(0) {
            self.writer.finish();
        }

        Ok(ArrayWriter {
            writer: self.writer,
            remaining: len,
        })
    }

    #[track_caller]
    pub fn write_map(mut self, len: Option<usize>) -> io::Result<MapWriter<'rf, 'wr>> {
        let mut writer = self.writer.get();
        writer.write_tag(TypeTag::Map {
            has_length: len.is_some(),
        })?;

        if let Some(len) = len {
            varint::write_unsigned_varint(writer.inner(), len)?;
        }

        if len == Some(0) {
            self.writer.finish();
        }

        Ok(MapWriter {
            writer: self.writer,
            remaining: len,
        })
    }
}

pub struct SizedTupleWriter<'rf, 'wr> {
    writer: WriterLevel<'rf, 'wr>,
    remaining: usize,
}

impl<'wr> SizedTupleWriter<'_, 'wr> {
    #[track_caller]
    pub fn write_value(&mut self) -> ValueWriter<'_, 'wr> {
        if self.remaining == 0 {
            panic!("Attempt to add more values to the tuple than specified")
        }

        self.remaining -= 1;

        let writer = if self.remaining == 0 {
            self.writer.continue_level()
        } else {
            self.writer.begin_sub_level()
        };

        ValueWriter { writer }
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }
}

pub struct SizedStructWriter<'rf, 'wr> {
    writer: WriterLevel<'rf, 'wr>,
    remaining: usize,
}

impl<'wr> SizedStructWriter<'_, 'wr> {
    #[track_caller]
    pub fn write_field<'n>(
        &mut self,
        name: impl Into<RefArcStr<'n>>,
    ) -> io::Result<ValueWriter<'_, 'wr>> {
        let mut writer = self.writer.get();
        if self.remaining == 0 {
            panic!("Attempt to add more fields to the map than specified")
        }

        self.remaining -= 1;

        writer.write_str(name.into())?;

        let writer = if self.remaining == 0 {
            self.writer.continue_level()
        } else {
            self.writer.begin_sub_level()
        };

        Ok(ValueWriter { writer })
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }
}

pub struct ArrayWriter<'rf, 'wr> {
    writer: WriterLevel<'rf, 'wr>,
    remaining: Option<usize>,
}

impl<'wr> ArrayWriter<'_, 'wr> {
    #[track_caller]
    pub fn write_value(&mut self) -> ValueWriter<'_, 'wr> {
        if self.remaining == Some(0) {
            panic!("Attempt to add more values to the seq than specified")
        }

        if let Some(remaining) = &mut self.remaining {
            *remaining -= 1;
        }

        let writer = if self.remaining == Some(0) {
            self.writer.continue_level()
        } else {
            self.writer.begin_sub_level()
        };

        ValueWriter { writer }
    }

    #[track_caller]
    pub fn finish(mut self) -> io::Result<()> {
        match self.remaining {
            Some(0) => Ok(()),
            Some(_) => panic!("Attempt to finish before adding all specified values"),
            None => {
                self.writer.get().write_tag(TypeTag::End)?;
                self.writer.finish();
                Ok(())
            }
        }
    }

    pub fn remaining(&self) -> Option<usize> {
        self.remaining
    }
}

pub struct MapWriter<'rf, 'wr> {
    writer: WriterLevel<'rf, 'wr>,
    remaining: Option<usize>,
}

impl<'wr> MapWriter<'_, 'wr> {
    pub fn write_pair(&mut self) -> MapPairWtiter<'_, 'wr> {
        if self.remaining == Some(0) {
            panic!("Attempt to add more values to the seq than specified")
        }

        if let Some(remaining) = &mut self.remaining {
            *remaining -= 1;
        }

        let writer = if self.remaining == Some(0) {
            self.writer.continue_level()
        } else {
            self.writer.begin_sub_level()
        };

        MapPairWtiter {
            writer,
            key_done: false,
        }
    }

    pub fn finish(mut self) -> io::Result<()> {
        match self.remaining {
            Some(0) => Ok(()),
            Some(_) => panic!("Attempt to finish before adding all specified values"),
            None => {
                self.writer.get().write_tag(TypeTag::End)?;
                self.writer.finish();
                Ok(())
            }
        }
    }

    pub fn remaining(&self) -> Option<usize> {
        self.remaining
    }
}

pub struct MapPairWtiter<'rf, 'wr> {
    writer: WriterLevel<'rf, 'wr>,
    key_done: bool,
}

impl<'rf, 'wr> MapPairWtiter<'rf, 'wr> {
    pub fn write_key(&mut self) -> ValueWriter<'_, 'wr> {
        if self.key_done {
            panic!("Attempt to write duplicate map key")
        }

        self.key_done = true;

        ValueWriter {
            writer: self.writer.begin_sub_level(),
        }
    }

    pub fn write_value(self) -> ValueWriter<'rf, 'wr> {
        if !self.key_done {
            panic!("Attempt to write map value before key")
        }
        ValueWriter {
            writer: self.writer,
        }
    }
}

#[allow(private_bounds)]
pub trait Primitive: PrimitiveImpl {}

impl<P: PrimitiveImpl> Primitive for P {}

trait PrimitiveImpl: Copy {
    fn write(self, writer: WriterRef) -> io::Result<()>;
}

impl PrimitiveImpl for () {
    fn write(self, mut writer: WriterRef) -> io::Result<()> {
        writer.write_tag(TypeTag::Unit)
    }
}

impl PrimitiveImpl for u8 {
    fn write(self, mut writer: WriterRef) -> io::Result<()> {
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
    fn write(self, mut writer: WriterRef) -> io::Result<()> {
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
    fn write(self, mut writer: WriterRef) -> io::Result<()> {
        writer.write_tag(TypeTag::Bool(self))?;
        Ok(())
    }
}

impl PrimitiveImpl for char {
    fn write(self, mut writer: WriterRef) -> io::Result<()> {
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
    fn write(self, mut writer: WriterRef) -> io::Result<()> {
        writer.write_tag(TypeTag::Float(FloatWidth::F32))?;
        writer.inner().write_all(&self.to_le_bytes())?;
        Ok(())
    }
}

impl PrimitiveImpl for f64 {
    fn write(self, mut writer: WriterRef) -> io::Result<()> {
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
            fn write(self, mut writer: WriterRef) -> io::Result<()> {
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
