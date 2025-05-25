use std::{
    collections::HashMap,
    io,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use crate::{
    str::RefArcStr,
    tag::{IntegerTag, OptionTag, StructTag, Tag, TagType},
    varint::{self, Sign},
    FORMAT_VERSION, MAGIC_HEADER,
};

#[cfg(smoldata_int_dev_error_checks)]
use std::{collections::BTreeSet, num::NonZeroUsize};

const MAX_OPT_STR_LEN: usize = 255;

pub struct Writer<'a> {
    writer: &'a mut dyn io::Write,
    string_map: HashMap<Arc<str>, u32>,
    next_string_id: u32,

    last_tag: Option<Tag<'static>>,
    last_tag_extra_repeat_times: usize,

    #[cfg(smoldata_int_dev_error_checks)]
    finish_parent_levels: BTreeSet<NonZeroUsize>,

    #[cfg(smoldata_int_dev_error_checks)]
    level: usize,
}

#[allow(unused)]
impl<'a> Writer<'a> {
    pub fn new(writer: &'a mut dyn io::Write) -> Result<Self, io::Error> {
        writer.write_all(MAGIC_HEADER)?;
        crate::varint::write_unsigned_varint(&mut *writer, FORMAT_VERSION)?;
        Ok(Self::new_headerless(writer))
    }

    pub fn new_headerless(writer: &'a mut dyn io::Write) -> Self {
        Self {
            writer,
            string_map: Default::default(),
            next_string_id: 0,

            last_tag: None,
            last_tag_extra_repeat_times: 0,

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
    pub fn finish(mut self) -> io::Result<&'a mut dyn io::Write> {
        #[cfg(smoldata_int_dev_error_checks)]
        if self.level != 0 {
            panic!("Attempt to finish before finishing children")
        }

        self.flush_repeats()?;

        Ok(self.writer)
    }

    #[allow(unused)]
    pub(crate) fn get_ref(&mut self) -> WriterRef<'a, '_> {
        WriterRef { writer: self }
    }

    fn write_integer(&mut self, int: &IntegerTag) -> io::Result<()> {
        match int {
            IntegerTag::I8(v) => self.writer.write_all(&[TagType::I8.into(), *v as u8]),
            IntegerTag::U8(v) => self.writer.write_all(&[TagType::U8.into(), *v]),
            IntegerTag::I16(v) => {
                let var = varint::is_varint_better(v.unsigned_abs().leading_zeros(), 2, true);
                if var {
                    self.writer.write_all(&[TagType::I16Var.into()])?;
                    varint::write_signed_varint(&mut *self.writer, *v)?;
                } else {
                    self.writer.write_all(&[TagType::I16.into()])?;
                    self.writer.write_all(&v.to_le_bytes())?;
                }
                Ok(())
            }
            IntegerTag::U16(v) => {
                let var = varint::is_varint_better(v.leading_zeros(), 2, false);
                if var {
                    self.writer.write_all(&[TagType::U16Var.into()])?;
                    varint::write_unsigned_varint(&mut *self.writer, *v)?;
                } else {
                    self.writer.write_all(&[TagType::U16.into()])?;
                    self.writer.write_all(&v.to_le_bytes())?;
                }
                Ok(())
            }
            IntegerTag::I32(v) => {
                let var = varint::is_varint_better(v.unsigned_abs().leading_zeros(), 4, true);
                if var {
                    self.writer.write_all(&[TagType::I32Var.into()])?;
                    varint::write_signed_varint(&mut *self.writer, *v)?;
                } else {
                    self.writer.write_all(&[TagType::I32.into()])?;
                    self.writer.write_all(&v.to_le_bytes())?;
                }
                Ok(())
            }
            IntegerTag::U32(v) => {
                let var = varint::is_varint_better(v.leading_zeros(), 4, false);
                if var {
                    self.writer.write_all(&[TagType::U32Var.into()])?;
                    varint::write_unsigned_varint(&mut *self.writer, *v)?;
                } else {
                    self.writer.write_all(&[TagType::U32.into()])?;
                    self.writer.write_all(&v.to_le_bytes())?;
                }
                Ok(())
            }
            IntegerTag::I64(v) => {
                let var = varint::is_varint_better(v.unsigned_abs().leading_zeros(), 8, true);
                if var {
                    self.writer.write_all(&[TagType::I64Var.into()])?;
                    varint::write_signed_varint(&mut *self.writer, *v)?;
                } else {
                    self.writer.write_all(&[TagType::I64.into()])?;
                    self.writer.write_all(&v.to_le_bytes())?;
                }
                Ok(())
            }
            IntegerTag::U64(v) => {
                let var = varint::is_varint_better(v.leading_zeros(), 8, false);
                if var {
                    self.writer.write_all(&[TagType::U64Var.into()])?;
                    varint::write_unsigned_varint(&mut *self.writer, *v)?;
                } else {
                    self.writer.write_all(&[TagType::U64.into()])?;
                    self.writer.write_all(&v.to_le_bytes())?;
                }
                Ok(())
            }
            IntegerTag::I128(v) => {
                let v = v.0;
                let var = varint::is_varint_better(v.unsigned_abs().leading_zeros(), 16, true);
                if var {
                    self.writer.write_all(&[TagType::I128Var.into()])?;
                    varint::write_signed_varint(&mut *self.writer, v)?;
                } else {
                    self.writer.write_all(&[TagType::I128.into()])?;
                    self.writer.write_all(&v.to_le_bytes())?;
                }
                Ok(())
            }
            IntegerTag::U128(v) => {
                let v = v.0;
                let var = varint::is_varint_better(v.leading_zeros(), 16, false);
                if var {
                    self.writer.write_all(&[TagType::U128Var.into()])?;
                    varint::write_unsigned_varint(&mut *self.writer, v)?;
                } else {
                    self.writer.write_all(&[TagType::U128.into()])?;
                    self.writer.write_all(&v.to_le_bytes())?;
                }
                Ok(())
            }
        }
    }

    fn flush_repeats(&mut self) -> io::Result<()> {
        match self.last_tag_extra_repeat_times {
            0 => {}
            1 => {
                self.writer.write_all(&[TagType::RepeatTag.into()])?;
                self.last_tag_extra_repeat_times = 0;
            }
            rep @ 2.. => {
                self.writer.write_all(&[TagType::RepeatTagMany.into()])?;
                varint::write_unsigned_varint(&mut *self.writer, rep - 2)?;
                self.last_tag_extra_repeat_times = 0;
            }
        }
        Ok(())
    }

    fn write_tag_inner(&mut self, mut tag: Tag) -> io::Result<()> {
        // todo: make it reserve unused tag type values for frequently written tags

        let equal_tag = self.last_tag.as_ref().filter(|t| t.eq_with_nan(&tag));
        if let Some(last) = &self.last_tag {
            if last.eq_with_nan(&tag) {
                if last.has_more_data() {
                    assert_eq!(self.last_tag_extra_repeat_times, 0);
                    self.writer.write_all(&[TagType::RepeatTag.into()])?;
                    return Ok(());
                } else {
                    self.last_tag_extra_repeat_times += 1;
                    return Ok(());
                }
            } else {
                self.flush_repeats()?;
            }
        }

        match &mut tag {
            Tag::Unit => {
                self.writer.write_all(&[TagType::Unit.into()])?;
            }
            Tag::Bool(true) => {
                self.writer.write_all(&[TagType::BoolTrue.into()])?;
            }
            Tag::Bool(false) => {
                self.writer.write_all(&[TagType::BoolFalse.into()])?;
            }
            Tag::Integer(v) => {
                self.write_integer(v)?;
            }
            Tag::F32(v) => {
                self.writer.write_all(&[TagType::F32.into()])?;
                self.writer.write_all(&v.to_le_bytes())?;
            }
            Tag::F64(v) => {
                self.writer.write_all(&[TagType::F64.into()])?;
                self.writer.write_all(&v.to_le_bytes())?;
            }
            Tag::Char(v) => {
                let v = *v as u32;
                let var = varint::is_varint_better(v.leading_zeros(), 4, false);
                if var {
                    self.writer.write_all(&[TagType::CharVar.into()])?;
                    varint::write_unsigned_varint(&mut *self.writer, v)?;
                } else {
                    self.writer.write_all(&[TagType::Char32.into()])?;
                    self.writer.write_all(&v.to_le_bytes())?;
                }
            }
            Tag::Str(s) => {
                self.writer.write_all(&[TagType::Str.into()])?;
                let arc = self.write_str_inner(s.clone())?;
                *s = RefArcStr::Arc(arc);
            }
            Tag::StrDirect { len } => {
                self.writer.write_all(&[TagType::StrDirect.into()])?;
                varint::write_unsigned_varint(&mut *self.writer, *len)?;
            }
            Tag::EmptyStr => {
                self.writer.write_all(&[TagType::EmptyStr.into()])?;
            }
            Tag::Bytes { len } => {
                self.writer.write_all(&[TagType::Bytes.into()])?;
                varint::write_unsigned_varint(&mut *self.writer, *len)?;
            }
            Tag::Option(OptionTag::None) => {
                self.writer.write_all(&[TagType::None.into()])?;
            }
            Tag::Option(OptionTag::Some) => {
                self.writer.write_all(&[TagType::Some.into()])?;
            }
            Tag::Struct(StructTag::Unit) => {
                self.writer.write_all(&[TagType::UnitStruct.into()])?;
            }
            Tag::Struct(StructTag::Newtype) => {
                self.writer.write_all(&[TagType::NewtypeStruct.into()])?;
            }
            Tag::Struct(StructTag::Tuple { len }) => {
                self.writer.write_all(&[TagType::TupleStruct.into()])?;
                varint::write_unsigned_varint(&mut *self.writer, *len)?;
            }
            Tag::Struct(StructTag::Struct { len }) => {
                self.writer.write_all(&[TagType::Struct.into()])?;
                varint::write_unsigned_varint(&mut *self.writer, *len)?;
            }
            Tag::Variant {
                name,
                ty: StructTag::Unit,
            } => {
                self.writer.write_all(&[TagType::UnitVariant.into()])?;
                self.write_str_inner(name.clone().into())?;
            }
            Tag::Variant {
                name,
                ty: StructTag::Newtype,
            } => {
                self.writer.write_all(&[TagType::NewtypeVariant.into()])?;
                self.write_str_inner(name.clone().into())?;
            }
            Tag::Variant {
                name,
                ty: StructTag::Tuple { len },
            } => {
                self.writer.write_all(&[TagType::TupleVariant.into()])?;
                self.write_str_inner(name.clone().into())?;
                varint::write_unsigned_varint(&mut *self.writer, *len)?;
            }
            Tag::Variant {
                name,
                ty: StructTag::Struct { len },
            } => {
                self.writer.write_all(&[TagType::StructVariant.into()])?;
                self.write_str_inner(name.clone().into())?;
                varint::write_unsigned_varint(&mut *self.writer, *len)?;
            }
            Tag::Array { len: None } => {
                self.writer.write_all(&[TagType::Array.into()])?;
            }
            Tag::Array { len: Some(len) } => {
                self.writer.write_all(&[TagType::LenArray.into()])?;
                varint::write_unsigned_varint(&mut *self.writer, *len)?;
            }
            Tag::Map { len: None } => {
                self.writer.write_all(&[TagType::Map.into()])?;
            }
            Tag::Map { len: Some(len) } => {
                self.writer.write_all(&[TagType::LenMap.into()])?;
                varint::write_unsigned_varint(&mut *self.writer, *len)?;
            }
            Tag::Tuple { len } => {
                self.writer.write_all(&[TagType::Tuple.into()])?;
                varint::write_unsigned_varint(&mut *self.writer, *len)?;
            }
        };

        let tag = tag.into_static();
        self.last_tag = Some(tag);
        self.last_tag_extra_repeat_times = 0;

        Ok(())
    }

    fn write_seq_end_inner(&mut self) -> io::Result<()> {
        self.flush_repeats()?;
        self.writer.write_all(&[TagType::End.into()])
    }

    fn try_get_cached_str(&self, str: &str) -> Option<Arc<str>> {
        self.string_map.get_key_value(str).map(|kv| kv.0.clone())
    }

    fn write_str_inner(&mut self, str: RefArcStr) -> io::Result<Arc<str>> {
        match self.string_map.get_key_value(str.deref()) {
            Some((s, i)) => {
                varint::write_varint_with_sign(&mut self.writer, *i, Sign::Positive)?;
                Ok(s.clone())
            }
            None => {
                let index = self.next_string_id;
                self.next_string_id += 1;
                let arc: Arc<str> = str.into();

                varint::write_varint_with_sign(&mut self.writer, index, Sign::Negative)?;
                varint::write_unsigned_varint(&mut self.writer, arc.len())?;
                self.writer.write_all(arc.as_bytes())?;

                let arc_clone = arc.clone();

                self.string_map.insert(arc, index);

                Ok(arc_clone)
            }
        }
    }
}

pub(crate) struct WriterLevel<'rf, 'wr> {
    writer: &'rf mut Writer<'wr>,

    #[cfg(smoldata_int_dev_error_checks)]
    level: Option<NonZeroUsize>,
}

impl<'wr> WriterLevel<'_, 'wr> {
    #[track_caller]
    pub fn get(&mut self) -> WriterRef<'_, 'wr> {
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
    pub fn finish(&mut self) {
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
    pub fn begin_sub_level(&mut self) -> WriterLevel<'_, 'wr> {
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
    pub fn continue_level(&mut self) -> WriterLevel<'_, 'wr> {
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

pub(crate) struct WriterRef<'rf, 'wr> {
    writer: &'rf mut Writer<'wr>,
}

#[allow(unused)]
impl<'wr> WriterRef<'_, 'wr> {
    pub fn write_tag(&mut self, tag: Tag) -> io::Result<()> {
        self.writer.write_tag_inner(tag)
    }

    pub fn write_seq_end(&mut self) -> io::Result<()> {
        self.writer.write_seq_end_inner()
    }

    pub fn try_get_cached_str(&self, str: &str) -> Option<Arc<str>> {
        self.writer.try_get_cached_str(str)
    }

    pub fn write_str(&mut self, str: RefArcStr) -> io::Result<()> {
        self.writer.write_str_inner(str)?;
        Ok(())
    }

    pub fn inner(&mut self) -> &mut dyn io::Write {
        &mut self.writer.writer
    }

    pub fn clone(&mut self) -> WriterRef<'_, 'wr> {
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
    pub(crate) writer: WriterLevel<'rf, 'wr>,
}

#[allow(unused)]
impl<'rf, 'wr> ValueWriter<'rf, 'wr> {
    #[track_caller]
    pub fn write_primitive<P: WritePrimitive>(mut self, pri: P) -> io::Result<()> {
        let mut writer = self.writer.get();
        writer.write_tag(pri.into_tag())?;
        self.writer.finish();
        Ok(())
    }

    #[track_caller]
    pub fn write_string<'s>(mut self, str: impl Into<RefArcStr<'s>>) -> io::Result<()> {
        let mut writer = self.writer.get();
        let str = str.into();
        if str.is_empty() {
            writer.write_tag(Tag::EmptyStr)?;
        } else if str.len() > MAX_OPT_STR_LEN {
            writer.write_tag(Tag::StrDirect { len: str.len() })?;
            writer.write_all(str.as_bytes())?;
        } else {
            writer.write_tag(Tag::Str(str))?;
        }

        self.writer.finish();
        Ok(())
    }

    #[track_caller]
    pub fn write_bytes(mut self, bytes: &[u8]) -> io::Result<()> {
        let mut writer = self.writer.get();
        writer.write_tag(Tag::Bytes { len: bytes.len() })?;
        writer.write_all(bytes)?;
        self.writer.finish();

        Ok(())
    }

    #[track_caller]
    pub fn write_none(mut self) -> io::Result<()> {
        let mut writer = self.writer.get();
        writer.write_tag(Tag::Option(OptionTag::None))?;
        self.writer.finish();

        Ok(())
    }

    #[track_caller]
    pub fn write_some(mut self) -> io::Result<Self> {
        let mut writer = self.writer.get();
        writer.write_tag(Tag::Option(OptionTag::Some))?;

        /// Some() stays on the same level
        Ok(self)
    }

    #[track_caller]
    pub fn write_unit_struct(mut self) -> io::Result<()> {
        let mut writer = self.writer.get();
        writer.write_tag(Tag::Struct(StructTag::Unit))?;
        self.writer.finish();

        Ok(())
    }

    #[track_caller]
    pub fn write_newtype_struct(mut self) -> io::Result<Self> {
        let mut writer = self.writer.get();
        writer.write_tag(Tag::Struct(StructTag::Newtype))?;

        /// Newtype structs stay on the same level
        Ok(self)
    }

    #[track_caller]
    pub fn write_tuple_struct(mut self, fields: usize) -> io::Result<SizedTupleWriter<'rf, 'wr>> {
        let mut writer = self.writer.get();
        writer.write_tag(Tag::Struct(StructTag::Tuple { len: fields }))?;

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
        writer.write_tag(Tag::Struct(StructTag::Struct { len: fields }))?;

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
        let name = match variant.into() {
            RefArcStr::Arc(a) => a,
            RefArcStr::Str(s) => writer.try_get_cached_str(s).unwrap_or_else(|| s.into()),
        };
        writer.write_tag(Tag::Variant {
            name,
            ty: StructTag::Unit,
        })?;
        self.writer.finish();

        Ok(())
    }

    #[track_caller]
    pub fn write_newtype_variant<'n>(
        mut self,
        variant: impl Into<RefArcStr<'n>>,
    ) -> io::Result<Self> {
        let mut writer = self.writer.get();
        let name = match variant.into() {
            RefArcStr::Arc(a) => a,
            RefArcStr::Str(s) => writer.try_get_cached_str(s).unwrap_or_else(|| s.into()),
        };
        writer.write_tag(Tag::Variant {
            name,
            ty: StructTag::Newtype,
        })?;

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
        let name = match variant.into() {
            RefArcStr::Arc(a) => a,
            RefArcStr::Str(s) => writer.try_get_cached_str(s).unwrap_or_else(|| s.into()),
        };
        writer.write_tag(Tag::Variant {
            name,
            ty: StructTag::Tuple { len: fields },
        })?;

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
        let name = match variant.into() {
            RefArcStr::Arc(a) => a,
            RefArcStr::Str(s) => writer.try_get_cached_str(s).unwrap_or_else(|| s.into()),
        };
        writer.write_tag(Tag::Variant {
            name,
            ty: StructTag::Struct { len: fields },
        })?;

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
        writer.write_tag(Tag::Tuple { len: fields })?;

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
    pub fn write_array(mut self, len: Option<usize>) -> io::Result<ArrayWriter<'rf, 'wr>> {
        let mut writer = self.writer.get();
        writer.write_tag(Tag::Array { len })?;

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
        writer.write_tag(Tag::Map { len })?;

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
                self.writer.get().write_seq_end()?;
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
                self.writer.get().write_seq_end()?;
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
pub trait WritePrimitive: InternalWritePrimitive {}

impl<T: InternalWritePrimitive> WritePrimitive for T {}

pub(crate) trait InternalWritePrimitive {
    fn into_tag(self) -> Tag<'static>;
}

impl InternalWritePrimitive for () {
    fn into_tag(self) -> Tag<'static> {
        Tag::Unit
    }
}

macro_rules! impl_primitive {
    ($($member:ident $ty:ty),* $(,)?) => {
        $(impl InternalWritePrimitive for $ty {
            fn into_tag(self) -> Tag<'static> {
                Tag::$member(self)
            }
        })*
    };
}

macro_rules! impl_primitive_int {
    ($($member:ident $ty:ty),* $(,)?) => {
        $(impl InternalWritePrimitive for $ty {
            fn into_tag(self) -> Tag<'static> {
                Tag::Integer(IntegerTag::$member(self.into()))
            }
        })*
    };
}

impl_primitive!(Bool bool, Char char, F32 f32, F64 f64);
impl_primitive_int!(I8 i8, U8 u8, I16 i16, U16 u16, I32 i32, U32 u32, I64 i64, U64 u64, I128 i128, U128 u128);
