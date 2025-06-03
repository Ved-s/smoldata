use proc_macro2::{Ident, Span};

use crate::{
    codegen, ident_nonraw_string, parsing::{self, MacroInput}, ErrorCollector, StringLitOrPath, TraitType
};

pub fn process_input(input: MacroInput) -> syn::Result<codegen::CodegenData> {
    let MacroInput {
        ty,
        type_name,
        attrs,
        generics,
        inty,
    } = input;

    let mut errors = ErrorCollector::new();

    let base_crate = attrs.smoldata.map(|a| a.attr).unwrap_or_else(|| {
        // ::smoldata
        syn::Path {
            leading_colon: Some(syn::token::PathSep {
                spans: [Span::call_site(), Span::call_site()],
            }),
            segments: syn::punctuated::Punctuated::from_iter([syn::PathSegment {
                ident: Ident::new("smoldata", Span::call_site()),
                arguments: syn::PathArguments::None,
            }]),
        }
    });

    let inty = match inty {
        parsing::InputType::Struct(s) => codegen::InputType::Struct(process_struct(s, &mut errors)),
        parsing::InputType::Enum(ev) => {
                codegen::InputType::Enum(ev.into_iter().map(|v| {
                    process_enum_variant(v, &mut errors)
                }).collect())
            },
    };

    let read_generics = process_generics(generics.clone(), &base_crate, TraitType::Read);
    let write_generics = process_generics(generics, &base_crate, TraitType::Write);

    let data = codegen::CodegenData {
        ty,
        type_name,
        read_generics,
        write_generics,
        inty,
        base_crate,
    };

    errors.wrap(data)
}

fn process_struct(s: parsing::StructType, errors: &mut ErrorCollector) -> codegen::StructType {
    match s {
       parsing::StructType::Unit => {
            codegen::StructType::Unit
        }
       parsing::StructType::Newtype(f) => {
            codegen::StructType::Newtype(process_tuple_field(None, f))
        }
       parsing::StructType::Tuple(fs) => {
            codegen::StructType::Tuple(
                fs.into_iter()
                    .enumerate()
                    .map(|(i, f)| process_tuple_field(Some(i), f))
                    .collect(),
            )
        }
       parsing::StructType::Struct(fs) => {
            codegen::StructType::Struct(
                fs.into_iter()
                    .map(|f| process_struct_field(f, errors))
                    .collect(),
            )
        }
    }
}

fn process_enum_variant(v: parsing::EnumVariant, errors: &mut ErrorCollector) -> codegen::EnumVariant {
    let parsing::EnumVariant { name, ty, attrs } = v;

    let name_str = ident_nonraw_string(&name);

    let data_name = attrs
        .rename
        .map(|a| a.attr)
        .unwrap_or_else(|| StringLitOrPath::String(syn::LitStr::new(&name_str, Span::call_site())));

    codegen::EnumVariant {
        name_ident: name,
        display_name: name_str,
        data_name,
        ty: process_struct(ty, errors),
    }
}

fn process_tuple_field(index: Option<usize>, f: parsing::TupleField) -> codegen::TupleField {
    let tmp_name = match index {
        Some(i) => Ident::new(&format!("v{i}"), Span::call_site()),
        None => Ident::new("v", Span::call_site()),
    };
    codegen::TupleField { tmp_name, ty: f.ty }
}

fn process_struct_field(
    f: parsing::StructField,
    errors: &mut ErrorCollector,
) -> codegen::StructField {
    let parsing::StructField { name, ty, attrs } = f;

    errors.try_add(attrs.verify_attributes());
    let optimize_option = attrs.do_optimize_option(&ty);

    let name_str = ident_nonraw_string(&name);
    let tmp_name = Ident::new(&format!("f_{name_str}"), Span::call_site());

    let data_name = attrs
        .rename
        .map(|a| a.attr)
        .unwrap_or_else(|| StringLitOrPath::String(syn::LitStr::new(&name_str, Span::call_site())));

    codegen::StructField {
        name_ident: name,
        tmp_name,
        display_name: name_str,
        data_name,
        ty,
        optimize_option,
    }
}

fn process_generics(mut g: syn::Generics, base_crate: &syn::Path, ty: TraitType) -> syn::Generics {
    for param in &mut g.params {
        let syn::GenericParam::Type(param) = param else {
            continue;
        };

        let name = match ty {
            TraitType::Read => "SmolRead",
            TraitType::Write => "SmolWrite",
        };
        let name = Ident::new(name, Span::call_site());
        let mut trai = base_crate.clone();
        trai.segments.push(syn::PathSegment {
            ident: name,
            arguments: syn::PathArguments::None,
        });

        param.bounds.insert(0, syn::TypeParamBound::Trait(syn::TraitBound {
            paren_token: None,
            modifier: syn::TraitBoundModifier::None,
            lifetimes: None,
            path: trai,
        }));
    }
    g
}