use std::{
    any::type_name,
    collections::BTreeMap,
    fmt::Debug,
    io,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use crate::{
    str::{RefArcStr, SdString},
    tag::{IntegerTag, OptionTag, StructTag, StructType, Tag, TagType},
    varint, FORMAT_VERSION, MAGIC_HEADER,
};

#[cfg(smoldata_int_dev_error_checks)]
use std::{collections::BTreeSet, num::NonZeroUsize};

#[derive(Debug, thiserror::Error)]
pub enum ReaderInitError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    VarIntRead(#[from] crate::varint::VarIntReadError),

    #[error("Input stream contains invalid magic value")]
    InvalidMagic,

    #[error("Input stream is of unsupported version {0}, max supported is {FORMAT_VERSION}")]
    UnsupportedVersion(u32),
}

#[derive(Clone, Copy)]
pub enum TagPeek {
    None,
    LastTag,
}

#[derive(Clone)]
pub enum TagOrEnd {
    Tag(Tag<'static>),
    End,
}

pub struct Reader<'a> {
    reader: &'a mut dyn io::Read,

    string_map: BTreeMap<u32, Arc<str>>,
    format_version: u32,
    tag_peek: TagPeek,

    last_tag: Option<Tag<'static>>,
    last_tag_repeat: usize,

    #[cfg(smoldata_int_dev_error_checks)]
    finish_parent_levels: BTreeSet<NonZeroUsize>,

    #[cfg(smoldata_int_dev_error_checks)]
    level: usize,
}

impl<'a> Reader<'a> {
    pub fn new(reader: &'a mut dyn io::Read) -> Result<Self, ReaderInitError> {
        let mut magicbuf = [0u8; MAGIC_HEADER.len()];
        reader.read_exact(&mut magicbuf)?;
        if magicbuf != MAGIC_HEADER {
            return Err(ReaderInitError::InvalidMagic);
        }

        let version = crate::varint::read_unsigned_varint(&mut *reader)?;

        if version > FORMAT_VERSION {
            return Err(ReaderInitError::UnsupportedVersion(version));
        }

        Ok(Self::new_headerless(reader, version))
    }

    /// Panics if `format_version > crate::FORMAT_VERSION`
    #[allow(clippy::absurd_extreme_comparisons)]
    pub fn new_headerless(reader: &'a mut dyn io::Read, format_version: u32) -> Self {
        assert!(format_version <= FORMAT_VERSION);

        Self {
            reader,
            tag_peek: TagPeek::None,
            string_map: Default::default(),

            last_tag: None,
            last_tag_repeat: 0,

            format_version,

            #[cfg(smoldata_int_dev_error_checks)]
            finish_parent_levels: Default::default(),

            #[cfg(smoldata_int_dev_error_checks)]
            level: 0,
        }
    }

    #[track_caller]
    pub fn read(&mut self) -> ValueReader<'_, 'a> {
        #[cfg(smoldata_int_dev_error_checks)]
        let level = {
            if self.level != 0 {
                panic!("Attempt to begin reading new root object before finishing children")
            }
            self.level += 1;
            NonZeroUsize::new(self.level).expect("cosmic ray")
        };
        ValueReader {
            reader: ReaderLevel {
                reader: self,

                #[cfg(smoldata_int_dev_error_checks)]
                level: Some(level),
            },
        }
    }

    #[track_caller]
    pub fn finish(self) -> &'a mut dyn io::Read {
        #[cfg(smoldata_int_dev_error_checks)]
        if self.level != 0 {
            panic!("Attempt to finish before finishing children")
        }

        self.reader
    }

    #[allow(unused)]
    pub(crate) fn get_ref(&mut self) -> ReaderRef<'_, 'a> {
        ReaderRef { reader: self }
    }

    fn read_tag_or_end(&mut self) -> ReadResult<TagOrEnd> {

        if self.last_tag_repeat > 0 {
            let Some(tag) = self.last_tag.clone() else {
                panic!("last_tag_repeat > 0 but last_tag is None");
            };
            self.last_tag_repeat -= 1;
            return Ok(TagOrEnd::Tag(tag));
        }

        let mut tag_byte = 0u8;
        self.reader
            .read_exact(std::slice::from_mut(&mut tag_byte))
            .map_err(ReadError::from)?;
        let tag = match TagType::try_from(tag_byte) {
            Ok(t) => t,
            Err(v) => {
                // todo: reserved tags
                return Err(ReadError::InvalidTag(v).into());
            }
        };

        let mut repeat = false;

        let tag = match tag {
            TagType::Unit => Tag::Unit,
            TagType::BoolFalse => Tag::Bool(false),
            TagType::BoolTrue => Tag::Bool(true),
            TagType::U8 => {
                let mut v = [0u8; 1];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::Integer(IntegerTag::U8(v[0]))
            }
            TagType::I8 => {
                let mut v = [0u8; 1];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::Integer(IntegerTag::I8(v[0] as i8))
            }
            TagType::U16 => {
                let mut v = [0u8; 2];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::Integer(IntegerTag::U16(u16::from_le_bytes(v)))
            }
            TagType::I16 => {
                let mut v = [0u8; 2];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::Integer(IntegerTag::I16(i16::from_le_bytes(v)))
            }
            TagType::U32 => {
                let mut v = [0u8; 4];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::Integer(IntegerTag::U32(u32::from_le_bytes(v)))
            }
            TagType::I32 => {
                let mut v = [0u8; 4];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::Integer(IntegerTag::I32(i32::from_le_bytes(v)))
            }
            TagType::U64 => {
                let mut v = [0u8; 8];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::Integer(IntegerTag::U64(u64::from_le_bytes(v)))
            }
            TagType::I64 => {
                let mut v = [0u8; 8];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::Integer(IntegerTag::I64(i64::from_le_bytes(v)))
            }
            TagType::U128 => {
                let mut v = [0u8; 16];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::Integer(IntegerTag::U128(u128::from_le_bytes(v).into()))
            }
            TagType::I128 => {
                let mut v = [0u8; 16];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::Integer(IntegerTag::I128(i128::from_le_bytes(v).into()))
            }
            TagType::U16Var => Tag::Integer(IntegerTag::U16(
                varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
            )),
            TagType::I16Var => Tag::Integer(IntegerTag::I16(
                varint::read_signed_varint(&mut *self.reader).map_err(ReadError::from)?,
            )),
            TagType::U32Var => Tag::Integer(IntegerTag::U32(
                varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
            )),
            TagType::I32Var => Tag::Integer(IntegerTag::I32(
                varint::read_signed_varint(&mut *self.reader).map_err(ReadError::from)?,
            )),
            TagType::U64Var => Tag::Integer(IntegerTag::U64(
                varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
            )),
            TagType::I64Var => Tag::Integer(IntegerTag::I64(
                varint::read_signed_varint(&mut *self.reader).map_err(ReadError::from)?,
            )),
            TagType::U128Var => Tag::Integer(IntegerTag::U128(PackedU128(
                varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
            ))),
            TagType::I128Var => Tag::Integer(IntegerTag::I128(PackedI128(
                varint::read_signed_varint(&mut *self.reader).map_err(ReadError::from)?,
            ))),
            TagType::F32 => {
                let mut v = [0u8; 4];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::F32(f32::from_le_bytes(v))
            }
            TagType::F64 => {
                let mut v = [0u8; 8];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                Tag::F64(f64::from_le_bytes(v))
            }
            TagType::Char32 => {
                let mut v = [0u8; 4];
                self.reader.read_exact(&mut v).map_err(ReadError::from)?;
                match char::from_u32(u32::from_le_bytes(v)) {
                    Some(c) => Tag::Char(c),
                    None => return Err(ReadError::InvalidChar(u32::from_le_bytes(v)).into()),
                }
            }
            TagType::CharVar => {
                let v = varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?;
                match char::from_u32(v) {
                    Some(c) => Tag::Char(c),
                    None => return Err(ReadError::InvalidChar(v).into()),
                }
            }
            TagType::Str => Tag::Str(RefArcStr::Arc(self.read_str_inner()?)),
            TagType::StrDirect => Tag::StrDirect {
                len: varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
            },
            TagType::EmptyStr => Tag::EmptyStr,
            TagType::Bytes => Tag::Bytes {
                len: varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
            },
            TagType::None => Tag::Option(OptionTag::None),
            TagType::Some => Tag::Option(OptionTag::Some),
            TagType::UnitStruct => Tag::Struct(StructTag::Unit),
            TagType::UnitVariant => Tag::Variant {
                name: self.read_str_inner()?,
                ty: StructTag::Unit,
            },
            TagType::NewtypeStruct => Tag::Struct(StructTag::Newtype),
            TagType::NewtypeVariant => Tag::Variant {
                name: self.read_str_inner()?,
                ty: StructTag::Newtype,
            },
            TagType::Array => Tag::Array { len: None },
            TagType::LenArray => Tag::Array {
                len: Some(
                    varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
                ),
            },
            TagType::Tuple => Tag::Tuple {
                len: varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
            },
            TagType::TupleStruct => Tag::Struct(StructTag::Tuple {
                len: varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
            }),
            TagType::TupleVariant => {
                let name = self.read_str_inner()?;
                let len =
                    varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?;
                Tag::Variant {
                    name,
                    ty: StructTag::Tuple { len },
                }
            }
            TagType::Map => Tag::Map { len: None },
            TagType::LenMap => Tag::Map {
                len: Some(
                    varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
                ),
            },
            TagType::Struct => Tag::Struct(StructTag::Struct {
                len: varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?,
            }),
            TagType::StructVariant => {
                let name = self.read_str_inner()?;
                let len =
                    varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?;
                Tag::Variant {
                    name,
                    ty: StructTag::Struct { len },
                }
            }

            TagType::RepeatTag => {
                if self.last_tag_repeat != 0 {
                    return Err(ReadError::StackedTagRepeats.into());
                }
                match &self.last_tag {
                    None => return Err(ReadError::NothingToRepeat.into()),
                    Some(t) => {
                        repeat = true;
                        t.clone()
                    },
                }
            }

            TagType::RepeatTagMany => {
                if self.last_tag_repeat != 0 {
                    return Err(ReadError::StackedTagRepeats.into());
                }
                let count: usize = varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?;
                let tag = match &self.last_tag {
                    None => return Err(ReadError::NothingToRepeat.into()),
                    Some(t) => {
                        repeat = true;
                        t.clone()
                    },
                };

                let Some(count) = count.checked_add(1) else {
                    return Err(ReadError::TooManyRepeats.into());
                };

                self.last_tag_repeat = count;

                tag
            }

            TagType::End => return Ok(TagOrEnd::End),
        };

        if !repeat {
            self.last_tag = Some(tag.clone());
        }

        Ok(TagOrEnd::Tag(tag))
    }

    fn read_tag_inner(&mut self) -> ReadResult<Tag<'static>> {
        match self.tag_peek {
            TagPeek::None => {}
            TagPeek::LastTag => {
                self.tag_peek = TagPeek::None;
                return Ok(self.last_tag.clone().expect("proper tag_peek and last_tag"))
            }
        }

        match self.read_tag_or_end()? {
            TagOrEnd::Tag(tag) => Ok(tag),
            TagOrEnd::End => Err(ReadError::UnexpectedEnd.into()),
        }
    }

    fn peek_tag_inner(&mut self) -> ReadResult<&Tag<'static>> {
        if matches!(self.tag_peek, TagPeek::None) {
            if let TagOrEnd::End = self.read_tag_or_end()? {
                return Err(ReadError::UnexpectedEnd.into())
            }
            self.tag_peek = TagPeek::LastTag;
        }

        match &self.tag_peek {
            TagPeek::None => unreachable!(),
            TagPeek::LastTag => Ok(self.last_tag.as_ref().expect("proper tag_peek and last_tag")),
        }
    }

    fn read_seq_end_inner(&mut self) -> ReadResult<bool> {
        if matches!(self.tag_peek, TagPeek::None) {
            if let TagOrEnd::End = self.read_tag_or_end()? {
                return Ok(true);
            }
            self.tag_peek = TagPeek::LastTag;
        }

        Ok(false)
    }

    fn read_str_inner(&mut self) -> ReadResult<Arc<str>> {
        let (index, sign) =
            varint::read_varint_with_sign(&mut *self.reader).map_err(ReadError::from)?;

        Ok(match sign {
            varint::Sign::Positive => {
                let Some(str) = self.string_map.get(&index) else {
                    return Err(Box::new(ReadError::InvalidStringReference(index)));
                };

                str.clone()
            }
            varint::Sign::Negative => {
                let length =
                    varint::read_unsigned_varint(&mut *self.reader).map_err(ReadError::from)?;
                let mut data = vec![0u8; length];
                self.reader.read_exact(&mut data).map_err(ReadError::from)?;

                let str = String::from_utf8(data).map_err(|_| ReadError::InvalidString)?;
                let arc: Arc<str> = str.into();

                self.string_map.insert(index, arc.clone());

                arc
            }
        })
    }
}

pub(crate) struct ReaderLevel<'rf, 'rd> {
    pub(self) reader: &'rf mut Reader<'rd>,

    #[cfg(smoldata_int_dev_error_checks)]
    pub(self) level: Option<NonZeroUsize>,
}

impl<'rd> ReaderLevel<'_, 'rd> {
    #[track_caller]
    pub fn get(&mut self) -> ReaderRef<'_, 'rd> {
        #[cfg(smoldata_int_dev_error_checks)]
        if self.level.is_some_and(|l| l.get() < self.reader.level) {
            panic!("Attempt to use a Reader before finishing its children")
        } else if self.level.is_none_or(|l| l.get() > self.reader.level) {
            panic!("Attempt to use a Reader after it finished")
        }
        ReaderRef {
            reader: self.reader,
        }
    }

    #[track_caller]
    pub fn finish(&mut self) {
        #[cfg(smoldata_int_dev_error_checks)]
        {
            let level = match self.level {
                None => panic!("Attempted to finish already finished reader"),
                Some(l) if l.get() > self.reader.level => {
                    panic!("Attempted to finish already finished reader")
                }
                Some(l) => l,
            };

            if level.get() < self.reader.level {
                self.reader.finish_parent_levels.insert(level);
            } else {
                self.reader.level -= 1;
                loop {
                    let Some(level) = NonZeroUsize::new(self.reader.level) else {
                        break;
                    };

                    if !self.reader.finish_parent_levels.remove(&level) {
                        break;
                    }

                    self.reader.level -= 1;
                }
            }

            self.level = None;
        }
    }

    /// Begin a new reader below this one
    #[track_caller]
    pub fn begin_sub_level(&mut self) -> ReaderLevel<'_, 'rd> {
        #[cfg(smoldata_int_dev_error_checks)]
        let level = {
            let level = match self.level {
                None => panic!("Attempt to begin a new sub-reader from a finished reader"),
                Some(l) if l.get() > self.reader.level => {
                    panic!("Attempt to begin a new sub-reader from a finished reader")
                }
                Some(l) => l,
            };

            self.reader.level += 1;
            level.checked_add(1).expect("too deep")
        };
        ReaderLevel {
            reader: self.reader,

            #[cfg(smoldata_int_dev_error_checks)]
            level: Some(level),
        }
    }

    /// Finish this reader and continue current level on a new one
    #[track_caller]
    pub fn continue_level(&mut self) -> ReaderLevel<'_, 'rd> {
        #[cfg(smoldata_int_dev_error_checks)]
        let level = {
            let level = match self.level {
                None => panic!("Attempt to continue level from a finished reader"),
                Some(l) if l.get() > self.reader.level => {
                    panic!("Attempt to continue level from a finished reader")
                }
                Some(l) => l,
            };

            self.level = None;
            level
        };

        ReaderLevel {
            reader: self.reader,

            #[cfg(smoldata_int_dev_error_checks)]
            level: Some(level),
        }
    }
    
    pub fn format_version(&self) -> u32 {
        self.reader.format_version
    }
}

pub(crate) struct ReaderRef<'rf, 'rd> {
    pub(self) reader: &'rf mut Reader<'rd>,
}

#[allow(unused)]
impl<'rd> ReaderRef<'_, 'rd> {
    pub fn read_tag(&mut self) -> ReadResult<Tag> {
        self.reader.read_tag_inner()
    }

    pub fn peek_tag(&mut self) -> ReadResult<&Tag> {
        self.reader.peek_tag_inner()
    }

    pub fn read_seq_end(&mut self) -> ReadResult<bool> {
        self.reader.read_seq_end_inner()
    }

    pub fn read_str(&mut self) -> ReadResult<Arc<str>> {
        self.reader.read_str_inner()
    }

    pub fn inner(&mut self) -> &mut dyn io::Read {
        &mut self.reader.reader
    }

    pub fn clone(&mut self) -> ReaderRef<'_, 'rd> {
        ReaderRef {
            reader: self.reader,
        }
    }

    #[allow(unused)]
    pub fn format_version(&self) -> u32 {
        self.reader.format_version
    }
}

impl DerefMut for ReaderRef<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.reader.reader
    }
}

impl<'rd> Deref for ReaderRef<'_, 'rd> {
    type Target = dyn io::Read + 'rd;

    fn deref(&self) -> &Self::Target {
        self.reader.reader
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("Read invalid tag {0}")]
    InvalidTag(u8),

    #[error("Reading varint")]
    VarIntRead(
        #[from]
        #[source]
        varint::VarIntReadError,
    ),

    #[error("Invalid string reference {0}")]
    InvalidStringReference(u32),

    #[error("Invalid string contents for UTF-8")]
    InvalidString,

    #[error("Invalid char value 0x{0:x}")]
    InvalidChar(u32),

    #[error("Read sequence end marker while trying to read a value")]
    UnexpectedEnd,

    #[error(transparent)]
    UnexpectedValueForType(#[from] UnexpectedValueForTypeError),

    #[error(transparent)]
    UnexpectedValueForVariant(#[from] UnexpectedValueForVariantError),

    #[error(transparent)]
    UnexpectedValue(#[from] UnexpectedValueError),

    #[error(
        "Unexpected length while reading {type_name}: Expected {expected} elements, got {got}"
    )]
    UnexpectedLength {
        expected: usize,
        got: usize,
        type_name: &'static str,
    },

    #[error("Unexpected field while reading {type_name}: {name}")]
    UnexpectedStructField {
        name: Arc<str>,
        type_name: &'static str,
    },

    #[error("Missing field for {type_name}: {name}")]
    MissingStructField {
        name: &'static str,
        type_name: &'static str,
    },

    #[error("Duplicate field for {type_name}: {name}")]
    DuplicateStructField {
        name: &'static str,
        type_name: &'static str,
    },

    #[error("Unexpected enum variant name while reading {type_name}: {name}")]
    UnexpectedEnumVariant {
        name: Arc<str>,
        type_name: &'static str,
    },

    #[error("Input stream contains invalid magic value")]
    InvalidMagic,

    #[error("Input stream is of unsupported version {0}, max supported is {FORMAT_VERSION}")]
    UnsupportedVersion(u32),

    #[error("Tried begin repeating tags while already repeating")]
    StackedTagRepeats,

    #[error("Tried to repeat tags before any tags were read")]
    NothingToRepeat,

    #[error("Tried to repeat tags too many times (hitting usize limit)")]
    TooManyRepeats,

    #[error("Value {value} is outside of bounds of {type_name}")]
    ValueOutOfBounds {
        value: String,
        type_name: &'static str,
    }
}

impl From<ReaderInitError> for ReadError {
    fn from(value: ReaderInitError) -> Self {
        match value {
            ReaderInitError::Io(e) => ReadError::Io(e),
            ReaderInitError::VarIntRead(e) => ReadError::VarIntRead(e),
            ReaderInitError::InvalidMagic => ReadError::InvalidMagic,
            ReaderInitError::UnsupportedVersion(v) => ReadError::UnsupportedVersion(v),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Unexpected data wile reading {type_name}: Expected {expected:?}, found {found:?}")]
pub struct UnexpectedValueForTypeError {
    pub expected: ValueTypeRequirement,
    pub found: ValueType,
    pub type_name: &'static str,
}

// TODO: other errors want variant name, make it into an Enum type/variant and use it instead type name in errors
#[derive(Debug, thiserror::Error)]
#[error("Unexpected data wile reading {type_name}::{variant_name}: Expected {expected:?}, found {found:?}")]
pub struct UnexpectedValueForVariantError {
    pub expected: ValueTypeRequirement,
    pub found: ValueType,
    pub type_name: &'static str,
    pub variant_name: &'static str,
}

#[derive(Debug, thiserror::Error)]
#[error("Expected {expected:?}, found {found:?}")]
pub struct UnexpectedValueError {
    pub expected: ValueTypeRequirement,
    pub found: ValueType,
}

impl UnexpectedValueError {
    pub fn with_type_name_of<T>(self) -> UnexpectedValueForTypeError {
        UnexpectedValueForTypeError {
            expected: self.expected,
            found: self.found,
            type_name: type_name::<T>(),
        }
    }

    pub fn with_type_name(self, name: &'static str) -> UnexpectedValueForTypeError {
        UnexpectedValueForTypeError {
            expected: self.expected,
            found: self.found,
            type_name: name,
        }
    }

    pub fn with_variant_name_of<T>(
        self,
        variant_name: &'static str,
    ) -> UnexpectedValueForVariantError {
        UnexpectedValueForVariantError {
            expected: self.expected,
            found: self.found,
            type_name: type_name::<T>(),
            variant_name,
        }
    }

    pub fn with_variant_name(
        self,
        type_name: &'static str,
        variant_name: &'static str,
    ) -> UnexpectedValueForVariantError {
        UnexpectedValueForVariantError {
            expected: self.expected,
            found: self.found,
            type_name,
            variant_name,
        }
    }
}

pub trait UnexpectedValueResultExt<T> {
    fn with_type_name_of<U>(self) -> Result<T, UnexpectedValueForTypeError>;
    fn with_type_name(self, name: &'static str) -> Result<T, UnexpectedValueForTypeError>;
    fn with_variant_name_of<U>(
        self,
        variant_name: &'static str,
    ) -> Result<T, UnexpectedValueForVariantError>;
    fn with_variant_name(
        self,
        name: &'static str,
        variant_name: &'static str,
    ) -> Result<T, UnexpectedValueForVariantError>;
}

impl<T> UnexpectedValueResultExt<T> for Result<T, UnexpectedValueError> {
    fn with_type_name_of<U>(self) -> Result<T, UnexpectedValueForTypeError> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(e.with_type_name_of::<U>()),
        }
    }

    fn with_type_name(self, name: &'static str) -> Result<T, UnexpectedValueForTypeError> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(e.with_type_name(name)),
        }
    }

    fn with_variant_name_of<U>(
        self,
        variant_name: &'static str,
    ) -> Result<T, UnexpectedValueForVariantError> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(e.with_variant_name_of::<U>(variant_name)),
        }
    }
    fn with_variant_name(
        self,
        name: &'static str,
        variant_name: &'static str,
    ) -> Result<T, UnexpectedValueForVariantError> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(e.with_variant_name(name, variant_name)),
        }
    }
}

pub type ReadResult<T> = Result<T, Box<ReadError>>;

pub struct ValueReader<'rf, 'rd> {
    pub(crate) reader: ReaderLevel<'rf, 'rd>,
}

#[repr(Rust, packed)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PackedU128(pub u128);

impl From<u128> for PackedU128 {
    fn from(value: u128) -> Self {
        Self(value)
    }
}
impl From<PackedU128> for u128 {
    fn from(val: PackedU128) -> Self {
        val.0
    }
}

#[repr(Rust, packed)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PackedI128(pub i128);

impl From<i128> for PackedI128 {
    fn from(value: i128) -> Self {
        Self(value)
    }
}
impl From<PackedI128> for i128 {
    fn from(val: PackedI128) -> Self {
        val.0
    }
}

impl Debug for PackedU128 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = self.0;
        Debug::fmt(&v, f)
    }
}

impl Debug for PackedI128 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = self.0;
        Debug::fmt(&v, f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Primitive {
    Unit,
    Bool(bool),

    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    U128(PackedU128),
    I128(PackedI128),

    Char(char),
    F32(f32),
    F64(f64),
}

impl Primitive {
    pub fn ty(&self) -> PrimitiveType {
        match self {
            Self::Unit => PrimitiveType::Unit,
            Self::Bool(_) => PrimitiveType::Bool,

            Self::U8(_) => PrimitiveType::U8,
            Self::I8(_) => PrimitiveType::I8,
            Self::U16(_) => PrimitiveType::U16,
            Self::I16(_) => PrimitiveType::I16,
            Self::U32(_) => PrimitiveType::U32,
            Self::I32(_) => PrimitiveType::I32,
            Self::I64(_) => PrimitiveType::I64,
            Self::U64(_) => PrimitiveType::U64,
            Self::U128(_) => PrimitiveType::U128,
            Self::I128(_) => PrimitiveType::I128,

            Self::Char(_) => PrimitiveType::Char,
            Self::F32(_) => PrimitiveType::F32,
            Self::F64(_) => PrimitiveType::F64,
        }
    }
}

impl TryFrom<Primitive> for () {
    type Error = UnexpectedValueError;

    fn try_from(value: Primitive) -> Result<Self, Self::Error> {
        match value {
            Primitive::Unit => Ok(()),
            rest => Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Primitive(Some(PrimitiveType::Unit)),
                found: ValueType::Primitive(rest.ty()),
            }),
        }
    }
}

impl crate::writer::InternalWritePrimitive for Primitive {
    fn into_tag(self) -> Tag<'static> {
        match self {
            Primitive::Unit => Tag::Unit,
            Primitive::Bool(b) => Tag::Bool(b),
            Primitive::U8(v) => Tag::Integer(IntegerTag::U8(v)),
            Primitive::I8(v) => Tag::Integer(IntegerTag::I8(v)),
            Primitive::U16(v) => Tag::Integer(IntegerTag::U16(v)),
            Primitive::I16(v) => Tag::Integer(IntegerTag::I16(v)),
            Primitive::U32(v) => Tag::Integer(IntegerTag::U32(v)),
            Primitive::I32(v) => Tag::Integer(IntegerTag::I32(v)),
            Primitive::U64(v) => Tag::Integer(IntegerTag::U64(v)),
            Primitive::I64(v) => Tag::Integer(IntegerTag::I64(v)),
            Primitive::U128(v) => Tag::Integer(IntegerTag::U128(v)),
            Primitive::I128(v) => Tag::Integer(IntegerTag::I128(v)),
            Primitive::Char(c) => Tag::Char(c),
            Primitive::F32(f) => Tag::F32(f),
            Primitive::F64(f) => Tag::F64(f),
        }
    }
}

macro_rules! impl_primitive_try_from {
    ($($ty:ident $primty:ident),* $(,)?) => {
        $(
            impl TryFrom<Primitive> for $ty {
                type Error = UnexpectedValueError;

                fn try_from(value: Primitive) -> Result<Self, Self::Error> {
                    match value {
                        Primitive::$primty(v) => Ok(v.into()),
                        rest => Err(UnexpectedValueError {
                            expected: ValueTypeRequirement::Primitive(Some(PrimitiveType::$primty)),
                            found: ValueType::Primitive(rest.ty()),
                        }),
                    }
                }
            }
        )*
    };
}

impl_primitive_try_from! {
    bool Bool,

    u8 U8,
    i8 I8,
    u16 U16,
    i16 I16,
    u32 U32,
    i32 I32,
    u64 U64,
    i64 I64,
    u128 U128,
    i128 I128,

    char Char,
    f32 F32,
    f64 F64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveType {
    Unit,
    Bool,

    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    I64,
    U64,
    U128,
    I128,

    Char,
    F32,
    F64,
}

enum StringReaderType {
    Str(Arc<str>),
    Direct(usize),
    Empty,
}

pub struct StringReader<'rf, 'rd> {
    reader: ReaderLevel<'rf, 'rd>,
    ty: StringReaderType,
}

impl StringReader<'_, '_> {
    #[track_caller]
    pub fn read(mut self) -> ReadResult<SdString> {
        let mut reader = self.reader.get();
        let str = match self.ty {
            StringReaderType::Str(a) => SdString::Arc(a),
            StringReaderType::Direct(len) => {
                let mut data = vec![0u8; len];
                reader
                    .inner()
                    .read_exact(&mut data)
                    .map_err(ReadError::from)?;

                let str = String::from_utf8(data).map_err(|_| ReadError::InvalidString)?;

                SdString::Owned(str)
            }
            StringReaderType::Empty => SdString::Empty,
        };

        self.reader.finish();

        Ok(str)
    }

    pub fn format_version(&self) -> u32 {
        self.reader.format_version()
    }
}

pub struct BytesReader<'rf, 'rd> {
    reader: ReaderLevel<'rf, 'rd>,
    len: usize,
}

impl BytesReader<'_, '_> {
    #[track_caller]
    pub fn read_into(mut self, buf: &mut Vec<u8>) -> ReadResult<usize> {
        let mut reader = self.reader.get();

        buf.reserve(self.len);
        crate::copy::<_, _, 1024>(reader.deref_mut(), buf, Some(self.len))
            .map_err(ReadError::from)?;

        self.reader.finish();

        Ok(self.len)
    }

    #[track_caller]
    pub fn read(mut self) -> ReadResult<Vec<u8>> {
        let mut reader = self.reader.get();

        let mut vec = vec![0u8; self.len];
        reader
            .deref_mut()
            .read_exact(&mut vec)
            .map_err(ReadError::from)?;

        self.reader.finish();

        Ok(vec)
    }

    pub fn format_version(&self) -> u32 {
        self.reader.format_version()
    }
}

pub struct TupleReader<'rf, 'rd> {
    reader: ReaderLevel<'rf, 'rd>,
    remaining: usize,
}

impl<'rd> TupleReader<'_, 'rd> {
    #[track_caller]
    pub fn read_value(&mut self) -> Option<ValueReader<'_, 'rd>> {
        if self.remaining == 0 {
            return None;
        }

        self.remaining -= 1;

        let sub = if self.remaining == 0 {
            self.reader.continue_level()
        } else {
            self.reader.begin_sub_level()
        };

        Some(ValueReader { reader: sub })
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }

    pub fn format_version(&self) -> u32 {
        self.reader.format_version()
    }
}

pub struct StructReader<'rf, 'rd> {
    reader: ReaderLevel<'rf, 'rd>,
    remaining: usize,
}

impl<'rd> StructReader<'_, 'rd> {
    #[track_caller]
    pub fn read_field(&mut self) -> ReadResult<Option<(Arc<str>, ValueReader<'_, 'rd>)>> {
        if self.remaining == 0 {
            return Ok(None);
        }

        let mut reader = self.reader.get();

        self.remaining -= 1;
        let str = reader.read_str()?;

        let sub = if self.remaining == 0 {
            self.reader.continue_level()
        } else {
            self.reader.begin_sub_level()
        };

        let reader = ValueReader { reader: sub };

        Ok(Some((str, reader)))
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }

    pub fn format_version(&self) -> u32 {
        self.reader.format_version()
    }
}

pub enum StructReading<'rf, 'rd> {
    Unit,
    Newtype(ValueReader<'rf, 'rd>),
    Tuple(TupleReader<'rf, 'rd>),
    Struct(StructReader<'rf, 'rd>),
}

impl<'rf, 'rd> StructReading<'rf, 'rd> {
    pub fn ty(&self) -> StructType {
        match self {
            Self::Unit => StructType::Unit,
            Self::Newtype(_) => StructType::Newtype,
            Self::Tuple(_) => StructType::Tuple,
            Self::Struct(_) => StructType::Struct,
        }
    }

    /// Same as `take_unit_variant`, except gives a struct error on wrong type
    pub fn take_unit_struct(self) -> Result<(), UnexpectedValueError> {
        if let StructReading::Unit = self {
            Ok(())
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Unit)),
                found: ValueType::Struct(self.ty()),
            })
        }
    }

    /// Same as `take_newtype_variant`, except gives a struct error on wrong type
    pub fn take_newtype_struct(self) -> Result<ValueReader<'rf, 'rd>, UnexpectedValueError> {
        if let StructReading::Newtype(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Newtype)),
                found: ValueType::Struct(self.ty()),
            })
        }
    }

    /// Same as `take_tuple_variant`, except gives a struct error on wrong type
    pub fn take_tuple_struct(self) -> Result<TupleReader<'rf, 'rd>, UnexpectedValueError> {
        if let StructReading::Tuple(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Tuple)),
                found: ValueType::Struct(self.ty()),
            })
        }
    }

    /// Same as `take_field_variant`, except gives a struct error on wrong type
    pub fn take_field_struct(self) -> Result<StructReader<'rf, 'rd>, UnexpectedValueError> {
        if let StructReading::Struct(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Struct)),
                found: ValueType::Struct(self.ty()),
            })
        }
    }

    /// Same as `take_unit_struct`, except gives an enum error on wrong type
    pub fn take_unit_variant(self) -> Result<(), UnexpectedValueError> {
        if let StructReading::Unit = self {
            Ok(())
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Enum(Some(StructType::Unit)),
                found: ValueType::Enum(self.ty()),
            })
        }
    }

    /// Same as `take_newtype_struct`, except gives an enum error on wrong type
    pub fn take_newtype_variant(self) -> Result<ValueReader<'rf, 'rd>, UnexpectedValueError> {
        if let StructReading::Newtype(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Enum(Some(StructType::Newtype)),
                found: ValueType::Enum(self.ty()),
            })
        }
    }

    /// Same as `take_tuple_struct`, except gives an enum error on wrong type
    pub fn take_tuple_variant(self) -> Result<TupleReader<'rf, 'rd>, UnexpectedValueError> {
        if let StructReading::Tuple(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Enum(Some(StructType::Tuple)),
                found: ValueType::Enum(self.ty()),
            })
        }
    }

    /// Same as `take_field_struct`, except gives an enum error on wrong type
    pub fn take_field_variant(self) -> Result<StructReader<'rf, 'rd>, UnexpectedValueError> {
        if let StructReading::Struct(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Enum(Some(StructType::Struct)),
                found: ValueType::Enum(self.ty()),
            })
        }
    }
}

pub struct EnumReading<'rf, 'rd> {
    reader: ReaderLevel<'rf, 'rd>,
    name: Arc<str>,
    ty: StructTag,
}

impl<'rf, 'rd> EnumReading<'rf, 'rd> {
    #[track_caller]
    pub fn read_variant(mut self) -> ReadResult<(Arc<str>, StructReading<'rf, 'rd>)> {
        let reader = match self.ty {
            StructTag::Unit => {
                self.reader.finish();
                StructReading::Unit
            }
            StructTag::Newtype => StructReading::Newtype(ValueReader {
                reader: self.reader,
            }),
            StructTag::Tuple { len } => {
                if len == 0 {
                    self.reader.finish();
                }

                StructReading::Tuple(TupleReader {
                    reader: self.reader,
                    remaining: len,
                })
            }
            StructTag::Struct { len } => {
                if len == 0 {
                    self.reader.finish();
                }

                StructReading::Struct(StructReader {
                    reader: self.reader,
                    remaining: len,
                })
            }
        };

        Ok((self.name, reader))
    }

    pub fn ty(&self) -> StructType {
        match &self.ty {
            StructTag::Unit => StructType::Unit,
            StructTag::Newtype => StructType::Newtype,
            StructTag::Tuple { .. } => StructType::Tuple,
            StructTag::Struct { .. } => StructType::Struct,
        }
    }

    pub fn format_version(&self) -> u32 {
        self.reader.format_version()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Primitive(PrimitiveType),
    String,
    Bytes,
    Option(OptionTag),
    Struct(StructType),
    Enum(StructType),
    Tuple,
    Array,
    Map,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ValueTypeRequirement {
    Primitive(Option<PrimitiveType>),
    String,
    Bytes,
    Option(Option<OptionTag>),
    Struct(Option<StructType>),
    Enum(Option<StructType>),
    Tuple,
    Array,
    Map,
    Custom(RefArcStr<'static>)
}

impl Debug for ValueTypeRequirement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Primitive(Some(v)) => f.debug_tuple("Primitive").field(v).finish(),
            Self::Primitive(None) => write!(f, "Primitive"),
            Self::String => write!(f, "String"),
            Self::Bytes => write!(f, "Bytes"),
            Self::Option(Some(v)) => f.debug_tuple("Option").field(v).finish(),
            Self::Option(None) => write!(f, "Option"),
            Self::Struct(Some(v)) => f.debug_tuple("Struct").field(v).finish(),
            Self::Struct(None) => write!(f, "Struct"),
            Self::Enum(Some(v)) => f.debug_tuple("Enum").field(v).finish(),
            Self::Enum(None) => write!(f, "Enum"),
            Self::Tuple => write!(f, "Tuple"),
            Self::Array => write!(f, "Array"),
            Self::Map => write!(f, "Map"),
            Self::Custom(c) => f.write_str(c),
        }
    }
}

pub enum ValueReading<'rf, 'rd> {
    Primitive(Primitive),
    String(StringReader<'rf, 'rd>),
    Bytes(BytesReader<'rf, 'rd>),
    Option(Option<ValueReader<'rf, 'rd>>),
    Struct(StructReading<'rf, 'rd>),
    Enum(EnumReading<'rf, 'rd>),
    Tuple(TupleReader<'rf, 'rd>),
    Array(ArrayReader<'rf, 'rd>),
    Map(MapReader<'rf, 'rd>),
}

impl<'rf, 'rd> ValueReading<'rf, 'rd> {
    pub fn ty(&self) -> ValueType {
        match self {
            Self::Primitive(p) => ValueType::Primitive(p.ty()),
            Self::String(_) => ValueType::String,
            Self::Bytes(_) => ValueType::Bytes,
            Self::Option(o) => ValueType::Option(OptionTag::from_option(o)),
            Self::Struct(s) => ValueType::Struct(s.ty()),
            Self::Enum(s) => ValueType::Enum(s.ty()),
            Self::Tuple(_) => ValueType::Tuple,
            Self::Array(_) => ValueType::Array,
            Self::Map(_) => ValueType::Map,
        }
    }

    pub fn take_primitive(self) -> Result<Primitive, UnexpectedValueError> {
        if let Self::Primitive(p) = self {
            Ok(p)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Primitive(None),
                found: self.ty(),
            })
        }
    }

    pub fn take_string(self) -> Result<StringReader<'rf, 'rd>, UnexpectedValueError> {
        if let Self::String(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::String,
                found: self.ty(),
            })
        }
    }

    pub fn take_bytes(self) -> Result<BytesReader<'rf, 'rd>, UnexpectedValueError> {
        if let Self::Bytes(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Bytes,
                found: self.ty(),
            })
        }
    }

    pub fn take_option(self) -> Result<Option<ValueReader<'rf, 'rd>>, UnexpectedValueError> {
        if let Self::Option(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Option(None),
                found: self.ty(),
            })
        }
    }

    pub fn take_any_struct(self) -> Result<StructReading<'rf, 'rd>, UnexpectedValueError> {
        if let Self::Struct(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(None),
                found: self.ty(),
            })
        }
    }

    pub fn take_unit_struct(self) -> Result<(), UnexpectedValueError> {
        if let Self::Struct(StructReading::Unit) = self {
            Ok(())
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Unit)),
                found: self.ty(),
            })
        }
    }

    pub fn take_newtype_struct(self) -> Result<ValueReader<'rf, 'rd>, UnexpectedValueError> {
        if let Self::Struct(StructReading::Newtype(r)) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Newtype)),
                found: self.ty(),
            })
        }
    }

    pub fn take_tuple_struct(self) -> Result<TupleReader<'rf, 'rd>, UnexpectedValueError> {
        if let Self::Struct(StructReading::Tuple(r)) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Tuple)),
                found: self.ty(),
            })
        }
    }

    pub fn take_field_struct(self) -> Result<StructReader<'rf, 'rd>, UnexpectedValueError> {
        if let Self::Struct(StructReading::Struct(r)) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Struct)),
                found: self.ty(),
            })
        }
    }

    pub fn take_enum(self) -> Result<EnumReading<'rf, 'rd>, UnexpectedValueError> {
        if let Self::Enum(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Enum(None),
                found: self.ty(),
            })
        }
    }

    pub fn take_tuple(self) -> Result<TupleReader<'rf, 'rd>, UnexpectedValueError> {
        if let Self::Tuple(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Tuple,
                found: self.ty(),
            })
        }
    }

    pub fn take_array(self) -> Result<ArrayReader<'rf, 'rd>, UnexpectedValueError> {
        if let Self::Array(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Array,
                found: self.ty(),
            })
        }
    }

    pub fn take_map(self) -> Result<MapReader<'rf, 'rd>, UnexpectedValueError> {
        if let Self::Map(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Map,
                found: self.ty(),
            })
        }
    }
}

pub struct ArrayReader<'rf, 'rd> {
    reader: ReaderLevel<'rf, 'rd>,
    remaining: Option<usize>,
}

impl<'rd> ArrayReader<'_, 'rd> {
    #[track_caller]
    pub fn read_value(&mut self) -> ReadResult<Option<ValueReader<'_, 'rd>>> {
        if self.remaining == Some(0) {
            return Ok(None);
        }

        let mut reader = self.reader.get();

        if self.remaining.is_none() && reader.read_seq_end()? {
            self.remaining = Some(0);
            self.reader.finish();
            return Ok(None);
        }

        if let Some(rem) = &mut self.remaining {
            *rem -= 1;
        }

        let sub = if self.remaining == Some(0) {
            self.reader.continue_level()
        } else {
            self.reader.begin_sub_level()
        };

        Ok(Some(ValueReader { reader: sub }))
    }

    pub fn remaining(&self) -> Option<usize> {
        self.remaining
    }

    pub fn format_version(&self) -> u32 {
        self.reader.format_version()
    }
}

pub struct MapReader<'rf, 'rd> {
    reader: ReaderLevel<'rf, 'rd>,
    remaining: Option<usize>,
}

impl<'rd> MapReader<'_, 'rd> {
    #[track_caller]
    pub fn read_pair(&mut self) -> ReadResult<Option<MapPairReader<'_, 'rd>>> {
        if self.remaining == Some(0) {
            return Ok(None);
        }

        let mut reader = self.reader.get();

        if self.remaining.is_none() && reader.read_seq_end()? {
            self.remaining = Some(0);
            self.reader.finish();
            return Ok(None);
        }

        if let Some(rem) = &mut self.remaining {
            *rem -= 1;
        }

        let sub = if self.remaining == Some(0) {
            self.reader.continue_level()
        } else {
            self.reader.begin_sub_level()
        };

        Ok(Some(MapPairReader {
            reader: sub,
            key_done: false,
        }))
    }

    pub fn remaining(&self) -> Option<usize> {
        self.remaining
    }

    pub fn format_version(&self) -> u32 {
        self.reader.format_version()
    }
}

pub struct MapPairReader<'rf, 'rd> {
    reader: ReaderLevel<'rf, 'rd>,
    key_done: bool,
}

impl<'rf, 'rd> MapPairReader<'rf, 'rd> {
    #[track_caller]
    pub fn read_key(&mut self) -> ValueReader<'_, 'rd> {
        if self.key_done {
            panic!("Attempt to read map key multiple times")
        }

        self.key_done = true;
        ValueReader {
            reader: self.reader.begin_sub_level(),
        }
    }

    #[track_caller]
    pub fn read_value(self) -> ValueReader<'rf, 'rd> {
        if !self.key_done {
            panic!("Attempt to read map value before key")
        }

        ValueReader {
            reader: self.reader,
        }
    }

    pub fn format_version(&self) -> u32 {
        self.reader.format_version()
    }
}

impl<'rf, 'rd> ValueReader<'rf, 'rd> {
    #[track_caller]
    pub fn read(mut self) -> ReadResult<ValueReading<'rf, 'rd>> {
        let mut reader = self.reader.get();
        Ok(match reader.read_tag()? {
            Tag::Unit => {
                self.reader.finish();
                ValueReading::Primitive(Primitive::Unit)
            }
            Tag::Bool(b) => {
                self.reader.finish();
                ValueReading::Primitive(Primitive::Bool(b))
            }
            Tag::Integer(int) => {
                self.reader.finish();
                ValueReading::Primitive(int.into())
            }
            Tag::Char(c) => {
                self.reader.finish();
                ValueReading::Primitive(Primitive::Char(c))
            }
            Tag::F32(v) => {
                self.reader.finish();
                ValueReading::Primitive(Primitive::F32(v))
            }
            Tag::F64(v) => {
                self.reader.finish();
                ValueReading::Primitive(Primitive::F64(v))
            }
            Tag::Str(s) => ValueReading::String(StringReader {
                ty: StringReaderType::Str(s.into()),
                reader: self.reader,
            }),
            Tag::StrDirect { len } => ValueReading::String(StringReader {
                reader: self.reader,
                ty: StringReaderType::Direct(len),
            }),
            Tag::EmptyStr => ValueReading::String(StringReader {
                reader: self.reader,
                ty: StringReaderType::Empty,
            }),
            Tag::Bytes { len } => ValueReading::Bytes(BytesReader {
                reader: self.reader,
                len,
            }),
            Tag::Option(OptionTag::None) => {
                self.reader.finish();
                ValueReading::Option(None)
            }
            Tag::Option(OptionTag::Some) => ValueReading::Option(Some(self)),
            Tag::Struct(StructTag::Unit) => {
                self.reader.finish();
                ValueReading::Struct(StructReading::Unit)
            }
            Tag::Struct(StructTag::Newtype) => {
                ValueReading::Struct(StructReading::Newtype(ValueReader {
                    reader: self.reader,
                }))
            }
            Tag::Struct(StructTag::Tuple { len }) => {
                if len == 0 {
                    self.reader.finish();
                }
                ValueReading::Struct(StructReading::Tuple(TupleReader {
                    reader: self.reader,
                    remaining: len,
                }))
            }
            Tag::Struct(StructTag::Struct { len }) => {
                if len == 0 {
                    self.reader.finish();
                }
                ValueReading::Struct(StructReading::Struct(StructReader {
                    reader: self.reader,
                    remaining: len,
                }))
            }
            Tag::Variant { name, ty } => ValueReading::Enum(EnumReading {
                reader: self.reader,
                name,
                ty,
            }),
            Tag::Array { len } => {
                if len == Some(0) {
                    self.reader.finish();
                }
                ValueReading::Array(ArrayReader {
                    reader: self.reader,
                    remaining: len,
                })
            }
            Tag::Tuple { len } => {
                if len == 0 {
                    self.reader.finish();
                }
                ValueReading::Tuple(TupleReader {
                    reader: self.reader,
                    remaining: len,
                })
            }
            Tag::Map { len } => {
                if len == Some(0) {
                    self.reader.finish();
                }
                ValueReading::Map(MapReader {
                    reader: self.reader,
                    remaining: len,
                })
            }
        })
    }

    pub fn format_version(&self) -> u32 {
        self.reader.format_version()
    }
}
