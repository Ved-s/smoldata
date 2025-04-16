use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{DeriveInput, Generics, Type, spanned::Spanned};

#[proc_macro_derive(SmolWrite, attributes(sd))]
pub fn derive_write(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    derive_from_tokens(TraitTypeAll::Write, tokens)
}

#[proc_macro_derive(SmolRead, attributes(sd))]
pub fn derive_read(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    derive_from_tokens(TraitTypeAll::Read, tokens)
}

#[proc_macro_derive(SmolReadWrite, attributes(sd))]
pub fn derive_read_write(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    derive_from_tokens(TraitTypeAll::All, tokens)
}

fn derive_from_tokens(
    ty: TraitTypeAll,
    tokens: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(tokens as DeriveInput);

    let mut error = None::<syn::Error>;

    let error_mut = &mut error;
    let mut add_error = |err: syn::Error| {
        *error_mut = Some(match error_mut.take() {
            None => err,
            Some(mut error) => {
                error.combine(err);
                error
            }
        });
    };

    if let Err(e) = disallow_attributes(&input.attrs) {
        add_error(e);
    }

    for param in &input.generics.params {
        let attrs = match param {
            syn::GenericParam::Lifetime(v) => &v.attrs,
            syn::GenericParam::Type(v) => &v.attrs,
            syn::GenericParam::Const(v) => &v.attrs,
        };
        if let Err(e) = disallow_attributes(attrs) {
            add_error(e);
        }
    }

    let input_ident_span = input.ident.span();
    let input = MacroInput {
        ty,
        type_name: input.ident,
        generics: input.generics,
        inty: match input.data {
            syn::Data::Union(_) => {
                return compile_error(
                    input_ident_span,
                    "Smoldata read/write cannot be derived on unions",
                )
                .into();
            }
            syn::Data::Struct(data_struct) => InputType::Struct(
                data_struct
                    .fields
                    .into_iter()
                    .filter_map(|f| {
                        let mut attrs = StructFieldAttributes::default();
                        if let Err(e) = read_attributes_into(&f.attrs, &mut attrs) {
                            add_error(e);
                        }

                        Some((f.ident?, f.ty, attrs))
                    })
                    .map(|(ident, ty, attrs)| {
                        let data_name = attrs.rename.unwrap_or_else(|| {
                            StringLitOrPath::String(syn::LitStr::new(
                                &ident_nonraw_string(&ident),
                                Span::call_site(),
                            ))
                        });
                        NamedTypeField {
                            data_name,
                            name_str: ident_nonraw_string(&ident),
                            name: ident,
                            ty,
                        }
                    })
                    .collect(),
            ),
            syn::Data::Enum(data_enum) => InputType::Enum(
                data_enum
                    .variants
                    .into_iter()
                    .map(|v| {
                        let ty = match v.fields {
                            syn::Fields::Unit => EnumVariantType::Unit,
                            syn::Fields::Unnamed(fields) => {
                                for field in &fields.unnamed {
                                    if let Err(e) = disallow_attributes(&field.attrs) {
                                        add_error(e);
                                    }
                                }

                                let mut iter = fields.unnamed.into_iter();

                                let first = iter.next();
                                let second = iter.next();

                                match (first, second) {
                                    (None, _) => EnumVariantType::Unit,
                                    (Some(f), None) => {
                                        let name = Ident::new("v", Span::call_site());
                                        EnumVariantType::Newtype(UnnamedTypeField {
                                            name,
                                            ty: f.ty,
                                        })
                                    }
                                    (Some(a), Some(b)) => {
                                        let iter = [a, b].into_iter().chain(iter);
                                        EnumVariantType::Tuple(
                                            iter.enumerate()
                                                .map(|(i, f)| UnnamedTypeField {
                                                    name: Ident::new(
                                                        &format!("v{i}"),
                                                        Span::call_site(),
                                                    ),
                                                    ty: f.ty,
                                                })
                                                .collect(),
                                        )
                                    }
                                }
                            }
                            syn::Fields::Named(fields) => EnumVariantType::Struct(
                                fields
                                    .named
                                    .into_iter()
                                    .filter_map(|f| {
                                        let mut attrs = StructFieldAttributes::default();

                                        if let Err(e) = read_attributes_into(&f.attrs, &mut attrs) {
                                            add_error(e);
                                        }

                                        Some((f.ident?, f.ty, attrs))
                                    })
                                    .map(|(ident, ty, attrs)| {
                                        let data_name = attrs.rename.unwrap_or_else(|| {
                                            StringLitOrPath::String(syn::LitStr::new(
                                                &ident_nonraw_string(&ident),
                                                Span::call_site(),
                                            ))
                                        });

                                        NamedTypeField {
                                            data_name,
                                            name_str: ident_nonraw_string(&ident),
                                            name: ident,
                                            ty,
                                        }
                                    })
                                    .collect(),
                            ),
                        };

                        let mut attrs = VariantAttributes::default();
                        if let Err(e) = read_attributes_into(&v.attrs, &mut attrs) {
                            add_error(e);
                        }

                        let data_name = attrs.rename.unwrap_or_else(|| {
                            StringLitOrPath::String(syn::LitStr::new(
                                &ident_nonraw_string(&v.ident),
                                Span::call_site(),
                            ))
                        });

                        EnumVariant {
                            ty,
                            data_name,
                            name_str: ident_nonraw_string(&v.ident),
                            name: v.ident,
                        }
                    })
                    .collect(),
            ),
        },
    };

    if let Some(err) = error {
        return err.into_compile_error().into();
    }

    derive_from_parsed(&input).into()
}

fn derive_from_parsed(input: &MacroInput) -> TokenStream {
    let (impl_gen, type_gen, where_gen) = input.generics.split_for_impl();

    let read = if input.ty.read() {
        let reader = Ident::new("reader", Span::call_site());
        let imp = impl_derive_method(TraitType::Read, &input.inty, &reader);
        let type_name = &input.type_name;

        quote! {
            impl #impl_gen ::smoldata::SmolRead for #type_name #type_gen #where_gen {
                fn read(#reader: ::smoldata::reader::ValueReader) -> ::smoldata::reader::ReadResult<Self> {
                    #imp
                }
            }
        }
    } else {
        TokenStream::new()
    };

    let write = if input.ty.write() {
        let writer = Ident::new("writer", Span::call_site());
        let imp = impl_derive_method(TraitType::Write, &input.inty, &writer);
        let type_name = &input.type_name;

        quote! {
            impl #impl_gen ::smoldata::SmolWrite for #type_name #type_gen #where_gen {
                fn write(&self, #writer: ::smoldata::writer::ValueWriter) -> ::std::io::Result<()> {
                    #imp
                }
            }
        }
    } else {
        TokenStream::new()
    };

    quote! {
        #read
        #write
    }
}

fn impl_derive_method(trty: TraitType, inty: &InputType, reader_writer: &Ident) -> TokenStream {
    match (trty, inty) {
        (TraitType::Read, InputType::Struct(fields)) => {
            impl_derive_struct_read_method(fields, reader_writer)
        }
        (TraitType::Read, InputType::Enum(variants)) => {
            impl_derive_enum_read_method(variants, reader_writer)
        }
        (TraitType::Write, InputType::Struct(fields)) => {
            impl_derive_struct_write_method(fields, reader_writer)
        }
        (TraitType::Write, InputType::Enum(variants)) => {
            impl_derive_enum_write_method(variants, reader_writer)
        }
    }
}

fn gen_struct_write(
    fields: &[NamedTypeField],
    writer: &Ident,
    variant_data_name: Option<&StringLitOrPath>,
    field_accessor: &dyn Fn(&NamedTypeField) -> TokenStream,
) -> TokenStream {
    let nfields = fields.len();

    let write_fields = fields.iter().map(|field| {
        let data_name = &field.data_name;
        let ty = &field.ty;
        let accessor = field_accessor(field);

        quote! {
            <#ty as ::smoldata::SmolWrite>::write(#accessor, struc.write_field(#data_name)?)?;
        }
    });

    let writer_call = match variant_data_name {
        None => quote! { write_struct(#nfields) },
        Some(name) => quote! { write_struct_variant(#name, #nfields) },
    };

    quote! {
        let mut struc = #writer.#writer_call?;
        #(#write_fields)*
    }
}

fn impl_derive_struct_write_method(fields: &[NamedTypeField], writer: &Ident) -> TokenStream {
    let body = gen_struct_write(fields, writer, None, &|field| {
        let name = &field.name;
        quote! { &self.#name }
    });
    quote! {
        #body
        Ok(())
    }
}

fn impl_derive_enum_write_method(variants: &[EnumVariant], writer: &Ident) -> TokenStream {
    let member_impls = variants.iter().map(|v| {
        let EnumVariant { name, data_name, ty, .. } = v;

        match ty {
            EnumVariantType::Unit => quote! {
                Self::#name => {
                    writer.write_unit_variant(#data_name)?;
                }
            },
            EnumVariantType::Newtype(field) => {
                let ty = &field.ty;
                quote! {
                    Self::#name(v) => {
                        <#ty as ::smoldata::SmolWrite>::write(v, writer.write_newtype_variant(#data_name)?)?;
                    }
                }
            },
            EnumVariantType::Tuple(fields) if fields.is_empty() => quote! {
                Self::#name() => writer.write_unit_variant(#data_name)?,
            },
            EnumVariantType::Tuple(fields) => {
                let field_names = fields.iter().map(|f| &f.name);
                let nfields = fields.len();

                let writes = fields.iter().map(|field| {
                    let name = &field.name;
                    let ty = &field.ty;
                    quote! {
                        <#ty as ::smoldata::SmolWrite>::write(#name, tup.write_value())?;
                    }
                });

                quote! {
                    Self::#name(#(#field_names),*) => {
                        let mut tup = writer.write_tuple_variant(#data_name, #nfields)?;
                        #(#writes)*
                    }
                }
            }
            EnumVariantType::Struct(fields) => {
                let field_name_remaps_pat = fields.iter().map(|field| {
                    let from = &field.name;
                    let to = Ident::new(&format!("f_{}", field.name_str), Span::call_site());

                    quote! { #from: #to }
                });
                let body = gen_struct_write(
                    fields, writer, Some(data_name),
                    &|field| {
                        Ident::new(&format!("f_{}", field.name_str), Span::call_site())
                            .into_token_stream()
                    });

                quote! {
                    Self::#name { #(#field_name_remaps_pat),* } => {
                        #body
                    }
                }
            },
        }
    });
    quote! {
        match self {
            #(#member_impls)*
        }
        Ok(())
    }
}

/// `struct_builder: Fn(field_setters: ...) -> TokenStream`
fn gen_struct_read(
    fields: &[NamedTypeField],
    reader: &Ident,
    variant_name: Option<&str>,

    struct_builder: &dyn Fn(&mut dyn Iterator<Item = TokenStream>) -> TokenStream,
) -> TokenStream {
    let fields: Vec<_> = fields
        .iter()
        .map(|field| {
            let name_str = &field.name_str;
            let tmp_ident = Ident::new(&format!("f_{name_str}"), Span::call_site());
            (field, tmp_ident)
        })
        .collect();

    let tmp_defs = fields.iter().map(|(field, tmp_ident)| {
        let ty = &field.ty;
        quote! {
            let mut #tmp_ident = None::<#ty>;
        }
    });

    let reads = fields.iter().map(|(field, tmp_ident)| {
        let data_name = &field.data_name;
        let name_str = &field.name_str;
        let ty = &field.ty;
        quote! {
            #data_name => {
                if #tmp_ident.is_some() {
                    return Err(::smoldata::reader::ReadError::DuplicateStructField {
                        name: #name_str,
                        type_name: ::std::any::type_name::<Self>(),
                    }
                    .into());
                }
                #tmp_ident = Some(<#ty as ::smoldata::SmolRead>::read(field.1)?);
            }
        }
    });

    let unwraps = fields.iter().map(|(field, tmp_ident)| {
        let name_str = &field.name_str;
        quote! {
            let #tmp_ident = #tmp_ident.ok_or_else(|| ::smoldata::reader::ReadError::MissingStructField {
                name: #name_str,
                type_name: ::std::any::type_name::<Self>(),
            })?;
        }
    });

    let mut struct_fields = fields.iter().map(|(field, tmp_ident)| {
        let name = &field.name;
        quote! {
            #name: #tmp_ident
        }
    });

    let reader_call = match variant_name {
        None => quote! {
            read()?.take_field_struct().map_err(|e| e.with_type_name_of::<Self>())
        },
        Some(name) => quote! {
            take_field_variant().map_err(|e| e.with_variant_name_of::<Self>(#name))
        },
    };

    let result = struct_builder(&mut struct_fields);

    quote! {
        let mut struc = #reader
            .#reader_call
            .map_err(::smoldata::reader::ReadError::from)?;

        #(#tmp_defs)*

        while let Some(field) = struc.read_field()? {
            match ::std::ops::Deref::deref(&field.0) {
                #(#reads)*

                _ => {
                    return Err(::smoldata::reader::ReadError::UnexpectedStructField {
                        name: field.0,
                        type_name: ::std::any::type_name::<Self>(),
                    }
                    .into())
                }
            }
        }

        #(#unwraps)*

        #result
    }
}

fn impl_derive_struct_read_method(fields: &[NamedTypeField], reader: &Ident) -> TokenStream {
    gen_struct_read(
        fields,
        reader,
        None,
        &|fields| quote! { Ok(Self { #(#fields,)* }) },
    )
}

fn impl_derive_enum_read_method(variants: &[EnumVariant], reader: &Ident) -> TokenStream {
    let variants_impl = variants.iter().map(|var| {
        let name = &var.name;
        let name_str = &var.name_str;
        let data_name = &var.data_name;

        match &var.ty {
            EnumVariantType::Unit => {
                quote! {
                    #data_name => {
                        #reader
                            .take_unit_variant()
                            .map_err(|e|
                                ::smoldata::reader::ReadError::from(
                                    e.with_variant_name(type_name::<Self>(), #name_str)
                                )
                            )?;
                        Self::#name
                    }
                }
            }
            EnumVariantType::Newtype(field) => {
                let ty = &field.ty;
                quote! {
                    #data_name => {
                        Self::#name(<#ty as ::smoldata::SmolRead>::read(
                        #reader
                            .take_newtype_variant()
                            .map_err(|e|
                                ::smoldata::reader::ReadError::from(
                                    e.with_variant_name(type_name::<Self>(), #name_str)
                                )
                            )?
                        )?)
                    }
                }
            }

            EnumVariantType::Tuple(fields) => {
                let nfields = fields.len();

                let reads = fields.iter().map(|field| {
                    let name = &field.name;
                    let ty = &field.ty;

                    quote! {
                        let Some(#reader) = tuple.read_value() else {
                            break 'read;
                        };

                        let #name = <#ty as ::smoldata::SmolRead>::read(#reader)?;
                    }
                });

                let field_names = fields.iter().map(|field| &field.name);

                quote! {
                    #data_name => {
                        let mut tuple = #reader
                            .take_tuple_variant()
                            .map_err(|e|
                                ::smoldata::reader::ReadError::from(
                                    e.with_variant_name(type_name::<Self>(), #name_str)
                                )
                            )?;

                        let length = tuple.remaining();
                        'outer: {
                            'read: {
                                if length != #nfields {
                                    break 'read;
                                }

                                #(#reads)*

                                break 'outer Self::#name(#(#field_names,)*);
                            }

                            return Err(::smoldata::reader::ReadError::UnexpectedLength {
                                expected: 3,
                                got: length,
                                type_name: ::std::any::type_name::<Self>(),
                            }
                            .into());
                        }
                    }
                }
            }
            EnumVariantType::Struct(fields) => {
                let body = gen_struct_read(fields, reader, Some(&name_str), &|fields| {
                    quote! {
                        Self::#name { #(#fields,)* }
                    }
                });

                quote! {
                    #data_name => {
                        #body
                    }
                }
            }
        }
    });

    quote! {
        let var = #reader
            .read()?
            .take_enum()
            .map_err(|e| e.with_type_name_of::<Self>())
            .map_err(::smoldata::reader::ReadError::from)?
            .read_variant()?;

        let (name, #reader) = var;

        Ok(match name.deref() {
            #(#variants_impl)*

            _ => {
                return Err(::smoldata::reader::ReadError::UnexpectedEnumVariant {
                    name,
                    type_name: ::std::any::type_name::<Self>(),
                }
                .into())
            }
        })
    }
}

fn compile_error(span: Span, message: &str) -> TokenStream {
    let mut lit = Literal::string(message);
    lit.set_span(span);

    quote! {
        compile_error!(#lit);
    }
}

struct MacroInput {
    ty: TraitTypeAll,
    type_name: Ident,
    generics: Generics,
    inty: InputType,
}

#[derive(Clone, Copy)]
enum TraitType {
    Read,
    Write,
}

enum TraitTypeAll {
    Read,
    Write,
    All,
}

impl TraitTypeAll {
    fn read(&self) -> bool {
        match self {
            Self::Read => true,
            Self::Write => false,
            Self::All => true,
        }
    }

    fn write(&self) -> bool {
        match self {
            Self::Read => false,
            Self::Write => true,
            Self::All => true,
        }
    }
}

enum InputType {
    Struct(Vec<NamedTypeField>),
    Enum(Vec<EnumVariant>),
}

struct EnumVariant {
    name: Ident,
    name_str: String,
    data_name: StringLitOrPath,
    ty: EnumVariantType,
}

enum EnumVariantType {
    Unit,
    Newtype(UnnamedTypeField),
    Tuple(Vec<UnnamedTypeField>),
    Struct(Vec<NamedTypeField>),
}

struct NamedTypeField {
    name: Ident,
    name_str: String,
    data_name: StringLitOrPath,
    ty: Type,
}

struct UnnamedTypeField {
    name: Ident,
    ty: Type,
}

fn ident_nonraw_string(ident: &Ident) -> String {
    let mut string = ident.to_string();
    if string.starts_with("r#") {
        string.replace_range(0..2, "");
    }
    string
}

trait AttributeStruct {
    fn target_item_name(&self) -> &str;
    fn valid_attribute(&self, name: &str) -> bool;
    fn read_attribute(
        &mut self,
        name: &str,
        name_span: Span,
        reader: syn::parse::ParseStream,
    ) -> syn::Result<()>;
}

enum StringLitOrPath {
    String(syn::LitStr),
    Path(syn::Path),
}

impl quote::ToTokens for StringLitOrPath {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            StringLitOrPath::String(v) => v.to_tokens(tokens),
            StringLitOrPath::Path(v) => v.to_tokens(tokens),
        }
    }
}

impl syn::parse::Parse for StringLitOrPath {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        if input.peek(syn::Lit) {
            Ok(Self::String(syn::parse::Parse::parse(input)?))
        } else {
            Ok(Self::Path(syn::parse::Parse::parse(input)?))
        }
    }
}

macro_rules! define_attributes {
    (
        #[target = $target:literal]
        $(#[$meta:meta])*
        struct $name:ident {
            $(
                $attrname:ident: $attrty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Default)]
        struct $name {
            $(
                $attrname: Option<$attrty>,
            )*
        }

        impl AttributeStruct for $name {
            fn target_item_name(&self) -> &str {
                $target
            }

            fn valid_attribute(&self, name: &str) -> bool {
                match name {
                    $(
                        stringify!($attrname) => true,
                    )*
                    _ => false
                }
            }

            fn read_attribute(&mut self, name: &str, name_span: Span, reader: syn::parse::ParseStream) -> syn::Result<()> {
                match name {
                    $(
                        stringify!($attrname) => {
                            <syn::Token![=] as syn::parse::Parse>::parse(reader)?;
                            self.$attrname = Some(syn::parse::Parse::parse(reader)?);
                        }
                    )*
                    _ => return Err(syn::Error::new(name_span, format!("invalid attribute for {}: {}", $target, name))),
                }

                Ok(())
            }
        }

        impl syn::parse::Parse for $name {
            fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
                let mut val = Self::default();
                parse_attributes(input, &mut val)?;
                Ok(val)
            }
        }
    };
}

define_attributes! {
    #[target = "enum variant"]
    struct VariantAttributes {
        rename: StringLitOrPath,
    }
}

define_attributes! {
    #[target = "struct field"]
    struct StructFieldAttributes {
        rename: StringLitOrPath,
    }
}

// fn read_attributes<T: AttributeStruct + Default>(attrs: &[syn::Attribute]) -> syn::Result<T> {
//     let mut val = T::default();
//     read_attributes_into(attrs, &mut val)?;
//     Ok(val)
// }

fn disallow_attributes(attrs: &[syn::Attribute]) -> syn::Result<()> {
    let mut error = None::<syn::Error>;

    let error_mut = &mut error;
    let mut add_error = |err: syn::Error| {
        *error_mut = Some(match error_mut.take() {
            None => err,
            Some(mut error) => {
                error.combine(err);
                error
            }
        });
    };

    for attr in attrs {
        if attr
            .path()
            .get_ident()
            .is_some_and(|i| ident_nonraw_string(i) == "sd")
        {
            add_error(syn::Error::new(
                attr.span(),
                "sd attributes are disallowed here",
            ));
        }
    }

    if let Some(err) = error {
        return Err(err);
    }
    Ok(())
}

fn read_attributes_into(
    attrs: &[syn::Attribute],
    struc: &mut dyn AttributeStruct,
) -> syn::Result<()> {
    let mut error = None::<syn::Error>;

    let error_mut = &mut error;
    let mut add_error = |err: syn::Error| {
        *error_mut = Some(match error_mut.take() {
            None => err,
            Some(mut error) => {
                error.combine(err);
                error
            }
        });
    };

    for attr in attrs {
        let stream = match &attr.meta {
            syn::Meta::Path(path) => {
                if path
                    .get_ident()
                    .is_some_and(|i| ident_nonraw_string(i) == "sd")
                {
                    add_error(syn::Error::new(
                        path.span(),
                        "sd attribute requires parameters: #[sd(...)]",
                    ));
                }
                continue;
            }
            syn::Meta::List(list) => {
                if list
                    .path
                    .get_ident()
                    .is_some_and(|i| ident_nonraw_string(i) == "sd")
                {
                    &list.tokens
                } else {
                    continue;
                }
            }
            syn::Meta::NameValue(nv) => {
                if nv
                    .path
                    .get_ident()
                    .is_some_and(|i| ident_nonraw_string(i) == "sd")
                {
                    add_error(syn::Error::new(
                        nv.span(),
                        "sd attribute requires parameters: #[sd(...)]",
                    ));
                }
                continue;
            }
        };

        let parser = |stream: syn::parse::ParseStream| -> syn::Result<()> {
            parse_attributes(stream, struc)
        };

        if let Err(e) = syn::parse::Parser::parse2(parser, stream.clone()) {
            add_error(e);
        }
    }

    if let Some(err) = error {
        return Err(err);
    }
    Ok(())
}

fn parse_attributes(
    stream: syn::parse::ParseStream,
    struc: &mut dyn AttributeStruct,
) -> syn::Result<()> {
    let mut first = true;

    loop {
        if stream.is_empty() {
            break;
        }

        if !first {
            <syn::Token![,] as syn::parse::Parse>::parse(stream)?;
        }
        first = false;

        if stream.is_empty() {
            break;
        }

        let attr_name = <syn::Ident as syn::parse::Parse>::parse(stream)?;
        let attr_name_str = ident_nonraw_string(&attr_name);

        if !struc.valid_attribute(&attr_name_str) {
            return Err(syn::Error::new(
                attr_name.span(),
                format!(
                    "invalid attribute {} for {}",
                    attr_name_str,
                    struc.target_item_name()
                ),
            ));
        }

        struc.read_attribute(&attr_name_str, attr_name.span(), stream)?;
    }

    Ok(())
}
