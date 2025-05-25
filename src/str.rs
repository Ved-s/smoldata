use std::{ops::Deref, sync::Arc};

#[derive(Debug, Clone)]
pub enum RefArcStr<'a> {
    Arc(Arc<str>),
    Str(&'a str),
}
impl RefArcStr<'_> {
    pub fn into_static(self) -> RefArcStr<'static> {
        match self {
            RefArcStr::Arc(a) => RefArcStr::Arc(a),
            RefArcStr::Str("") => RefArcStr::Str(""),
            RefArcStr::Str(s) => RefArcStr::Arc(s.into()),
        }
    }
}

impl<'a> From<&'a str> for RefArcStr<'a> {
    fn from(value: &'a str) -> Self {
        Self::Str(value)
    }
}

impl From<Arc<str>> for RefArcStr<'_> {
    fn from(value: Arc<str>) -> Self {
        Self::Arc(value)
    }
}

impl Deref for RefArcStr<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        match self {
            RefArcStr::Arc(arc) => arc.deref(),
            RefArcStr::Str(s) => s,
        }
    }
}

impl<'a> From<RefArcStr<'a>> for Arc<str> {
    fn from(val: RefArcStr<'a>) -> Self {
        match val {
            RefArcStr::Arc(a) => a,
            RefArcStr::Str(s) => s.into(),
        }
    }
}

impl<T: Deref<Target = str>> PartialEq<T> for RefArcStr<'_> {
    fn eq(&self, other: &T) -> bool {
        self.deref().eq(other.deref())
    }
}

impl Eq for RefArcStr<'_> {}

pub enum SdString {
    Empty,
    Arc(Arc<str>),
    Owned(String),
}

impl SdString {
    pub fn as_ref_arc(&self) -> RefArcStr<'_> {
        match self {
            SdString::Empty => RefArcStr::Str(""),
            SdString::Arc(a) => RefArcStr::Arc(a.clone()),
            SdString::Owned(s) => RefArcStr::Str(s),
        }
    }
}

impl From<Arc<str>> for SdString {
    fn from(value: Arc<str>) -> Self {
        Self::Arc(value)
    }
}

impl From<String> for SdString {
    fn from(value: String) -> Self {
        Self::Owned(value)
    }
}

impl Deref for SdString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        match self {
            SdString::Empty => "",
            SdString::Arc(s) => s.deref(),
            SdString::Owned(s) => s.deref(),
        }
    }
}

impl From<SdString> for Arc<str> {
    fn from(val: SdString) -> Self {
        match val {
            SdString::Empty => "".into(),
            SdString::Arc(s) => s,
            SdString::Owned(s) => s.into(),
        }
    }
}

impl From<SdString> for String {
    fn from(val: SdString) -> Self {
        match val {
            SdString::Empty => "".into(),
            SdString::Arc(s) => s.deref().into(),
            SdString::Owned(s) => s,
        }
    }
}