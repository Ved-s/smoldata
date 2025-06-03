use proc_macro2::TokenStream;
use syn::{Data, DeriveInput, Fields, FieldsNamed, FieldsUnnamed, GenericParam, Generics, Ident, Type};

use crate::{
    ErrorCollector, TraitTypeAll,
    attributes::{
        InputTypeAttributes, StructFieldAttributes, VariantAttributes, disallow_attributes,
        read_attributes_into,
    },
};

pub fn parse_input(ty: TraitTypeAll, tokens: TokenStream) -> syn::Result<MacroInput> {
    let input: DeriveInput = syn::parse2(tokens)?;

    let mut errors = ErrorCollector::new();

    for param in &input.generics.params {
        let attrs = match param {
            GenericParam::Lifetime(v) => &v.attrs,
            GenericParam::Type(v) => &v.attrs,
            GenericParam::Const(v) => &v.attrs,
        };
        if let Err(e) = disallow_attributes(attrs) {
            errors.add(e);
        }
    }

    let mut attrs = InputTypeAttributes::default();
    if let Err(e) = read_attributes_into(&input.attrs, &mut attrs) {
        errors.add(e);
    }

    let input_ident_span = input.ident.span();
    let input = MacroInput {
        ty,
        type_name: input.ident,
        generics: input.generics,
        attrs,
        inty: match input.data {
            Data::Union(_) => {
                return Err(syn::Error::new(
                    input_ident_span,
                    "Smoldata read/write cannot be derived on unions",
                ));
            }
            Data::Struct(data_struct) => {
                let ty = match data_struct.fields {
                    Fields::Unit => StructType::Unit,
                    Fields::Unnamed(fields) => parse_unnamed_fields(fields, &mut errors),
                    Fields::Named(fields) => {
                        StructType::Struct(parse_named_fields(fields, &mut errors))
                    }
                };
                InputType::Struct(ty)
            }

            Data::Enum(data_enum) => InputType::Enum(
                data_enum
                    .variants
                    .into_iter()
                    .map(|v| {
                        let ty = match v.fields {
                            Fields::Unit => StructType::Unit,
                            Fields::Unnamed(fields) => {
                                parse_unnamed_fields(fields, &mut errors)
                            }
                            Fields::Named(fields) => {
                                StructType::Struct(parse_named_fields(fields, &mut errors))
                            }
                        };

                        let mut attrs = VariantAttributes::default();
                        errors.try_add(read_attributes_into(&v.attrs, &mut attrs));

                        EnumVariant {
                            ty,
                            name: v.ident,
                            attrs,
                        }
                    })
                    .collect(),
            ),
        },
    };

    errors.wrap(input)
}

pub fn parse_unnamed_fields(fields: FieldsUnnamed, errors: &mut ErrorCollector) -> StructType {
    for field in &fields.unnamed {
        if let Err(e) = disallow_attributes(&field.attrs) {
            errors.add(e);
        }
    }

    let mut iter = fields.unnamed.into_iter().map(|f| TupleField { ty: f.ty });

    let first = iter.next();
    let second = iter.next();

    match (first, second) {
        (None, _) => StructType::Unit,
        (Some(f), None) => StructType::Newtype(f),
        (Some(a), Some(b)) => {
            let iter = [a, b].into_iter().chain(iter);
            StructType::Tuple(iter.collect())
        }
    }
}

pub fn parse_named_fields(fields: FieldsNamed, errors: &mut ErrorCollector) -> Vec<StructField> {
    fields
        .named
        .into_iter()
        .filter_map(|f| {
            let mut attrs = StructFieldAttributes::default();
            errors.try_add(read_attributes_into(&f.attrs, &mut attrs));

            Some((f.ident?, f.ty, attrs))
        })
        .map(|(ident, ty, attrs)| StructField {
            name: ident,
            ty,
            attrs,
        })
        .collect()
}
pub struct MacroInput {
    pub ty: TraitTypeAll,
    pub type_name: Ident,
    pub attrs: InputTypeAttributes,
    pub generics: Generics,
    pub inty: InputType,
}

pub enum InputType {
    Struct(StructType),
    Enum(Vec<EnumVariant>),
}

pub struct EnumVariant {
    pub name: Ident,
    pub ty: StructType,

    pub attrs: VariantAttributes,
}

pub enum StructType {
    Unit,
    Newtype(TupleField),
    Tuple(Vec<TupleField>),
    Struct(Vec<StructField>),
}

pub struct StructField {
    pub name: Ident,
    pub ty: Type,

    pub attrs: StructFieldAttributes,
}

pub struct TupleField {
    pub ty: Type,
}
