use std::{any::type_name, collections::BTreeMap, fmt::Debug, io, sync::Arc};

use crate::{
    tag::{FloatWidth, IntWidth, OptionTag, StructType, TagReadError, TypeTag},
    varint, str::SdString,
};

pub struct Reader<R: io::Read> {
    reader: R,
    data: ReaderData,
}

impl<R: io::Read> Reader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            data: ReaderData {
                tag_peek: None,
                string_map: Default::default(),
            },
        }
    }

    fn ctx(&mut self) -> ReaderContext {
        ReaderContext {
            reader: &mut self.reader,
            data: &mut self.data,
        }
    }

    pub fn read(&mut self) -> ValueReader {
        let ctx = self.ctx();
        ValueReader { reader: ctx }
    }
}

struct ReaderData {
    tag_peek: Option<TypeTag>,
    string_map: BTreeMap<u32, Arc<str>>,
}

struct ReaderContext<'a> {
    reader: &'a mut dyn io::Read,
    data: &'a mut ReaderData,
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("Reading tag")]
    TagRead(
        #[from]
        #[source]
        TagReadError,
    ),

    #[error("Reading tag")]
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

    #[error("Unexpected length while reading {type_name}: Expected {expected} elements, got {got}")]
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
}

#[derive(Debug, thiserror::Error)]
#[error("Unexpected data wile reading {type_name}: Expected {expected:?}, found {found:?}")]
pub struct UnexpectedValueForTypeError {
    expected: ValueTypeRequirement,
    found: ValueType,
    type_name: &'static str,
}

// TODO: other errors want variant name, make it into an Enum type/variant and use it instead type name in errors
#[derive(Debug, thiserror::Error)]
#[error("Unexpected data wile reading {type_name}::{variant_name}: Expected {expected:?}, found {found:?}")]
pub struct UnexpectedValueForVariantError {
    expected: ValueTypeRequirement,
    found: ValueType,
    type_name: &'static str,
    variant_name: &'static str,
}

#[derive(Debug, thiserror::Error)]
#[error("Expected {expected:?}, found {found:?}")]
pub struct UnexpectedValueError {
    expected: ValueTypeRequirement,
    found: ValueType,
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

    pub fn with_variant_name_of<T>(self, variant_name: &'static str) -> UnexpectedValueForVariantError {
        UnexpectedValueForVariantError {
            expected: self.expected,
            found: self.found,
            type_name: type_name::<T>(),
            variant_name
        }
    }

    pub fn with_variant_name(self, type_name: &'static str, variant_name: &'static str) -> UnexpectedValueForVariantError {
        UnexpectedValueForVariantError {
            expected: self.expected,
            found: self.found,
            type_name,
            variant_name
        }
    }
}

pub trait UnexpectedValueResultExt<T> {
    fn with_type_name_of<U>(self) -> Result<T, UnexpectedValueForTypeError>;
    fn with_type_name(self, name: &'static str) -> Result<T, UnexpectedValueForTypeError>;
    fn with_variant_name_of<U>(self, variant_name: &'static str) -> Result<T, UnexpectedValueForVariantError>;
    fn with_variant_name(self, name: &'static str, variant_name: &'static str) -> Result<T, UnexpectedValueForVariantError>;
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

    fn with_variant_name_of<U>(self, variant_name: &'static str) -> Result<T, UnexpectedValueForVariantError> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(e.with_variant_name_of::<U>(variant_name))
        }
    }
    fn with_variant_name(self, name: &'static str, variant_name: &'static str) -> Result<T, UnexpectedValueForVariantError> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(e.with_variant_name(name, variant_name))
        }
    }
}

pub type ReadResult<T> = Result<T, Box<ReadError>>;

impl ReaderContext<'_> {
    fn read_tag(&mut self) -> ReadResult<TypeTag> {
        if let Some(tag) = self.data.tag_peek.take() {
            return Ok(tag);
        }

        let tag = TypeTag::read(self.reader).map_err(ReadError::from)?;
        Ok(tag)
    }

    fn peek_tag(&mut self) -> ReadResult<TypeTag> {
        if let Some(tag) = self.data.tag_peek {
            return Ok(tag);
        }

        let tag = TypeTag::read(self.reader).map_err(ReadError::from)?;
        self.data.tag_peek = Some(tag);
        Ok(tag)
    }

    fn inner(&mut self) -> &mut dyn io::Read {
        self.reader
    }

    fn read_str(&mut self) -> ReadResult<Arc<str>> {
        let (index, sign) =
            varint::read_varint_with_sign(&mut *self.reader).map_err(ReadError::from)?;

        Ok(match sign {
            varint::Sign::Positive => {
                let Some(str) = self.data.string_map.get(&index) else {
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

                self.data.string_map.insert(index, arc.clone());

                arc
            }
        })
    }

    fn child(&mut self) -> ReaderContext<'_> {
        ReaderContext {
            reader: self.reader,
            data: self.data,
        }
    }
}

pub struct ValueReader<'a> {
    reader: ReaderContext<'a>,
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
    I64(i64),
    U64(u64),
    U128(u128),
    I128(i128),

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

macro_rules! impl_primitive_try_from {
    ($($ty:ident $primty:ident),* $(,)?) => {
        $(
            impl TryFrom<Primitive> for $ty {
                type Error = UnexpectedValueError;
            
                fn try_from(value: Primitive) -> Result<Self, Self::Error> {
                    match value {
                        Primitive::$primty(v) => Ok(v),
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
    Str,
    Direct,
    Empty,
}

pub struct StringReader<'a> {
    reader: ReaderContext<'a>,
    ty: StringReaderType,
}

impl StringReader<'_> {
    pub fn read(mut self) -> ReadResult<SdString> {
        Ok(match self.ty {
            StringReaderType::Str => SdString::Arc(self.reader.read_str()?),
            StringReaderType::Direct => {
                let length =
                    varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)?;
                let mut data = vec![0u8; length];
                self.reader
                    .inner()
                    .read_exact(&mut data)
                    .map_err(ReadError::from)?;

                let str = String::from_utf8(data).map_err(|_| ReadError::InvalidString)?;

                SdString::Owned(str)
            }
            StringReaderType::Empty => SdString::Empty,
        })
    }
}

pub struct BytesReader<'a> {
    reader: ReaderContext<'a>,
}

impl BytesReader<'_> {
    pub fn read_into(mut self, buf: &mut Vec<u8>) -> ReadResult<usize> {
        let length = varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)?;

        buf.reserve(length);
        crate::copy::<_, _, 1024>(self.reader.inner(), buf, Some(length))
            .map_err(ReadError::from)?;

        Ok(length)
    }

    pub fn read(mut self) -> ReadResult<Vec<u8>> {
        let length = varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)?;
        let mut vec = vec![0u8; length];
        self.reader
            .inner()
            .read_exact(&mut vec)
            .map_err(ReadError::from)?;
        Ok(vec)
    }
}

pub struct TupleReader<'a> {
    reader: ReaderContext<'a>,
    remaining: usize,
}

impl TupleReader<'_> {
    pub fn read_value(&mut self) -> Option<ValueReader> {
        if self.remaining == 0 {
            return None;
        }

        self.remaining -= 1;

        Some(ValueReader {
            reader: self.reader.child(),
        })
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }
}

pub struct StructReader<'a> {
    reader: ReaderContext<'a>,
    remaining: usize,
}

impl StructReader<'_> {
    pub fn read_field(&mut self) -> ReadResult<Option<(Arc<str>, ValueReader)>> {
        if self.remaining == 0 {
            return Ok(None);
        }

        self.remaining -= 1;
        let str = self.reader.read_str()?;
        let reader = ValueReader {
            reader: self.reader.child(),
        };

        Ok(Some((str, reader)))
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }
}

pub enum StructReading<'a> {
    Unit,
    Newtype(ValueReader<'a>),
    Tuple(TupleReader<'a>),
    Struct(StructReader<'a>),
}

impl<'a> StructReading<'a> {
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
    pub fn take_newtype_struct(self) -> Result<ValueReader<'a>, UnexpectedValueError> {
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
    pub fn take_tuple_struct(self) -> Result<TupleReader<'a>, UnexpectedValueError> {
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
    pub fn take_field_struct(self) -> Result<StructReader<'a>, UnexpectedValueError> {
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
    pub fn take_newtype_variant(self) -> Result<ValueReader<'a>, UnexpectedValueError> {
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
    pub fn take_tuple_variant(self) -> Result<TupleReader<'a>, UnexpectedValueError> {
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
    pub fn take_field_variant(self) -> Result<StructReader<'a>, UnexpectedValueError> {
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

pub struct EnumReading<'a> {
    reader: ReaderContext<'a>,
    ty: StructType,
}

impl<'a> EnumReading<'a> {
    pub fn read_variant(mut self) -> ReadResult<(Arc<str>, StructReading<'a>)> {
        let name = self.reader.read_str()?;

        let reader = match self.ty {
            StructType::Unit => StructReading::Unit,
            StructType::Newtype => StructReading::Newtype(ValueReader {
                reader: self.reader,
            }),
            StructType::Tuple => {
                let length =
                    varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)?;
                StructReading::Tuple(TupleReader {
                    reader: self.reader,
                    remaining: length,
                })
            }
            StructType::Struct => {
                let length =
                    varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)?;
                StructReading::Struct(StructReader {
                    reader: self.reader,
                    remaining: length,
                })
            }
        };

        Ok((name, reader))
    }

    pub fn ty(&self) -> StructType {
        self.ty
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

#[derive(Clone, Copy, PartialEq, Eq)]
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
        }
    }
}

pub enum ValueReading<'a> {
    Primitive(Primitive),
    String(StringReader<'a>),
    Bytes(BytesReader<'a>),
    Option(Option<ValueReader<'a>>),
    Struct(StructReading<'a>),
    Enum(EnumReading<'a>),
    Tuple(TupleReader<'a>),
    Array(ArrayReader<'a>),
    Map(MapReader<'a>),
}

impl<'a> ValueReading<'a> {
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

    pub fn take_string(self) -> Result<StringReader<'a>, UnexpectedValueError> {
        if let Self::String(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::String,
                found: self.ty(),
            })
        }
    }

    pub fn take_bytes(self) -> Result<BytesReader<'a>, UnexpectedValueError> {
        if let Self::Bytes(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Bytes,
                found: self.ty(),
            })
        }
    }

    pub fn take_option(self) -> Result<Option<ValueReader<'a>>, UnexpectedValueError> {
        if let Self::Option(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Option(None),
                found: self.ty(),
            })
        }
    }

    pub fn take_any_struct(self) -> Result<StructReading<'a>, UnexpectedValueError> {
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

    pub fn take_newtype_struct(self) -> Result<ValueReader<'a>, UnexpectedValueError> {
        if let Self::Struct(StructReading::Newtype(r)) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Newtype)),
                found: self.ty(),
            })
        }
    }

    pub fn take_tuple_struct(self) -> Result<TupleReader<'a>, UnexpectedValueError> {
        if let Self::Struct(StructReading::Tuple(r)) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Tuple)),
                found: self.ty(),
            })
        }
    }

    pub fn take_field_struct(self) -> Result<StructReader<'a>, UnexpectedValueError> {
        if let Self::Struct(StructReading::Struct(r)) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Struct(Some(StructType::Struct)),
                found: self.ty(),
            })
        }
    }

    pub fn take_enum(self) -> Result<EnumReading<'a>, UnexpectedValueError> {
        if let Self::Enum(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Enum(None),
                found: self.ty(),
            })
        }
    }

    pub fn take_tuple(self) -> Result<TupleReader<'a>, UnexpectedValueError> {
        if let Self::Tuple(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Tuple,
                found: self.ty(),
            })
        }
    }

    pub fn take_array(self) -> Result<ArrayReader<'a>, UnexpectedValueError> {
        if let Self::Array(r) = self {
            Ok(r)
        } else {
            Err(UnexpectedValueError {
                expected: ValueTypeRequirement::Array,
                found: self.ty(),
            })
        }
    }

    pub fn take_map(self) -> Result<MapReader<'a>, UnexpectedValueError> {
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

pub struct ArrayReader<'a> {
    reader: ReaderContext<'a>,
    remaining: Option<usize>,
}

impl ArrayReader<'_> {
    pub fn read_value(&mut self) -> ReadResult<Option<ValueReader>> {
        if self.remaining == Some(0) {
            return Ok(None);
        }

        if self.remaining.is_none() && matches!(self.reader.peek_tag()?, TypeTag::End) {
            self.remaining = Some(0);
            return Ok(None);
        }

        if let Some(rem) = &mut self.remaining {
            *rem -= 1;
        }

        Ok(Some(ValueReader {
            reader: self.reader.child(),
        }))
    }

    pub fn remaining(&self) -> Option<usize> {
        self.remaining
    }
}

pub struct MapReader<'a> {
    reader: ReaderContext<'a>,
    remaining: Option<usize>,
}

impl MapReader<'_> {
    pub fn read_pair(&mut self) -> ReadResult<Option<MapPairReader>> {
        if self.remaining == Some(0) {
            return Ok(None);
        }

        if self.remaining.is_none() && matches!(self.reader.peek_tag()?, TypeTag::End) {
            self.remaining = Some(0);
            return Ok(None);
        }

        if let Some(rem) = &mut self.remaining {
            *rem -= 1;
        }

        Ok(Some(MapPairReader {
            reader: self.reader.child(),
            key_done: false,
        }))
    }

    pub fn remaining(&self) -> Option<usize> {
        self.remaining
    }
}

pub struct MapPairReader<'a> {
    reader: ReaderContext<'a>,
    key_done: bool,
}

impl<'a> MapPairReader<'a> {
    pub fn read_key(&mut self) -> ValueReader {
        if self.key_done {
            panic!("Attempt to read map key multiple times")
        }

        self.key_done = true;
        ValueReader {
            reader: self.reader.child(),
        }
    }

    pub fn read_value(self) -> ValueReader<'a> {
        if !self.key_done {
            panic!("Attempt to read map value before key")
        }

        ValueReader {
            reader: self.reader,
        }
    }
}

impl<'a> ValueReader<'a> {
    pub fn read(mut self) -> ReadResult<ValueReading<'a>> {
        Ok(match self.reader.read_tag()? {
            TypeTag::Unit => ValueReading::Primitive(Primitive::Unit),
            TypeTag::Bool(b) => ValueReading::Primitive(Primitive::Bool(b)),
            TypeTag::Integer {
                width,
                signed,
                varint,
            } => ValueReading::Primitive(self.read_integer(width, signed, varint)?),
            TypeTag::Char { varint: true } => {
                let val =
                    varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)?;
                let char = char::from_u32(val).ok_or(ReadError::InvalidChar(val))?;
                ValueReading::Primitive(Primitive::Char(char))
            }
            TypeTag::Char { varint: false } => {
                let mut buf = [0u8; 4];
                self.reader
                    .inner()
                    .read_exact(&mut buf)
                    .map_err(ReadError::from)?;
                let val = u32::from_le_bytes(buf);
                let char = char::from_u32(val).ok_or(ReadError::InvalidChar(val))?;
                ValueReading::Primitive(Primitive::Char(char))
            }
            TypeTag::Float(FloatWidth::F32) => {
                let mut buf = [0u8; 4];
                self.reader
                    .inner()
                    .read_exact(&mut buf)
                    .map_err(ReadError::from)?;
                let val = f32::from_le_bytes(buf);
                ValueReading::Primitive(Primitive::F32(val))
            }
            TypeTag::Float(FloatWidth::F64) => {
                let mut buf = [0u8; 8];
                self.reader
                    .inner()
                    .read_exact(&mut buf)
                    .map_err(ReadError::from)?;
                let val = f64::from_le_bytes(buf);
                ValueReading::Primitive(Primitive::F64(val))
            }
            TypeTag::Str => ValueReading::String(StringReader {
                reader: self.reader,
                ty: StringReaderType::Str,
            }),
            TypeTag::StrDirect => ValueReading::String(StringReader {
                reader: self.reader,
                ty: StringReaderType::Direct,
            }),
            TypeTag::EmptyStr => ValueReading::String(StringReader {
                reader: self.reader,
                ty: StringReaderType::Empty,
            }),
            TypeTag::Bytes => ValueReading::Bytes(BytesReader {
                reader: self.reader,
            }),
            TypeTag::Option(OptionTag::None) => ValueReading::Option(None),
            TypeTag::Option(OptionTag::Some) => ValueReading::Option(Some(self)),
            TypeTag::Struct(StructType::Unit) => ValueReading::Struct(StructReading::Unit),
            TypeTag::Struct(StructType::Newtype) => {
                ValueReading::Struct(StructReading::Newtype(ValueReader {
                    reader: self.reader,
                }))
            }
            TypeTag::Struct(StructType::Tuple) => {
                let length =
                    varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)?;
                ValueReading::Struct(StructReading::Tuple(TupleReader {
                    reader: self.reader,
                    remaining: length,
                }))
            }
            TypeTag::Struct(StructType::Struct) => {
                let length =
                    varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)?;
                ValueReading::Struct(StructReading::Struct(StructReader {
                    reader: self.reader,
                    remaining: length,
                }))
            }
            TypeTag::EnumVariant(ty) => ValueReading::Enum(EnumReading {
                ty,
                reader: self.reader,
            }),
            TypeTag::Array { has_length } => {
                let length = has_length
                    .then(|| {
                        varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)
                    })
                    .transpose()?;
                ValueReading::Array(ArrayReader {
                    reader: self.reader,
                    remaining: length,
                })
            }
            TypeTag::Tuple => {
                let length =
                    varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)?;
                ValueReading::Tuple(TupleReader {
                    reader: self.reader,
                    remaining: length,
                })
            }
            TypeTag::Map { has_length } => {
                let length = has_length
                    .then(|| {
                        varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)
                    })
                    .transpose()?;
                ValueReading::Map(MapReader {
                    reader: self.reader,
                    remaining: length,
                })
            }
            TypeTag::End => return Err(ReadError::UnexpectedEnd.into()),
        })
    }

    fn read_integer(
        mut self,
        width: IntWidth,
        signed: bool,
        varint: bool,
    ) -> ReadResult<Primitive> {
        // Short but very cryptic macro lol
        macro_rules! integer_read {
            (
                match ($widthparam:ident, $signedparam:ident, $varintparam:ident) {
                    $($widthty:ident $unsty:ident $unstyprim:ident $sty:ident $styprim:ident $width:literal),*
                    $(,)?
                }
            ) => {
            match ($widthparam, $signedparam, $varintparam) {
                $(
                    (IntWidth::$widthty, true, true) => Primitive::$styprim(
                        varint::read_signed_varint(self.reader.inner()).map_err(ReadError::from)?,
                    ),
                    (IntWidth::$widthty, true, false) => {
                        let mut buf = [0u8; $width];
                        self.reader.inner().read_exact(&mut buf).map_err(ReadError::from)?;
                        Primitive::$styprim($sty::from_le_bytes(buf))
                    },
                    (IntWidth::$widthty, false, true) => Primitive::$unstyprim(
                        varint::read_unsigned_varint(self.reader.inner()).map_err(ReadError::from)?,
                    ),
                    (IntWidth::$widthty, false, false) => {
                        let mut buf = [0u8; $width];
                        self.reader.inner().read_exact(&mut buf).map_err(ReadError::from)?;
                        Primitive::$unstyprim($unsty::from_le_bytes(buf))
                    },
                )*
            }
            };
        }

        Ok(integer_read! {
            match (width, signed, varint) {
                W8 u8 U8 i8 I8 1,
                W16 u16 U16 i16 I16 2,
                W32 u32 U32 i32 I32 4,
                W64 u64 U64 i64 I64 8,
                W128 u128 U128 i128 I128 16,
            }
        })
    }
}
