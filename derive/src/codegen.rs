use crate::{StringLitOrPath, TraitType, TraitTypeAll};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Generics, Ident, Type};

pub struct Codegen {
    data: CodegenData,
}

impl Codegen {
    pub fn new(data: CodegenData) -> Self {
        Self { data }
    }

    pub fn generate(&self) -> TokenStream {
        
        let base_crate = &self.data.base_crate;

        let read = if self.data.ty.read() {
            let (impl_gen, type_gen, where_gen) = self.data.read_generics.split_for_impl();
            let reader = Ident::new("reader", Span::call_site());
            let imp = self.gen_derive_method(TraitType::Read, &reader);
            let type_name = &self.data.type_name;

            quote! {
                impl #impl_gen #base_crate::SmolRead for #type_name #type_gen #where_gen {
                    fn read(#reader: #base_crate::reader::ValueReader) -> #base_crate::reader::ReadResult<Self> {
                        let #reader = #reader.read()?;
                        #imp
                    }
                }
            }
        } else {
            TokenStream::new()
        };

        let write = if self.data.ty.write() {
            let (impl_gen, type_gen, where_gen) = self.data.write_generics.split_for_impl();
            let writer = Ident::new("writer", Span::call_site());
            let imp = self.gen_derive_method(TraitType::Write, &writer);
            let type_name = &self.data.type_name;

            quote! {
                impl #impl_gen #base_crate::SmolWrite for #type_name #type_gen #where_gen {
                    fn write(&self, #writer: #base_crate::writer::ValueWriter) -> ::std::io::Result<()> {
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

    fn gen_derive_method(&self, trty: TraitType, reader_writer: &Ident) -> TokenStream {
        match (trty, &self.data.inty) {
            (TraitType::Read, InputType::Struct(ty)) => self.gen_struct_read(ty, reader_writer),
            (TraitType::Read, InputType::Enum(variants)) => {
                self.gen_enum_read(variants, reader_writer)
            }
            (TraitType::Write, InputType::Struct(ty)) => self.gen_struct_write(ty, reader_writer),
            (TraitType::Write, InputType::Enum(variants)) => {
                self.gen_enum_write(variants, reader_writer)
            }
        }
    }

    /// writes fields from their tmp names into tuple_writer
    fn gen_tuple_write_core(&self, fields: &[TupleField], tuple_writer: &Ident) -> TokenStream {
        let base_crate = &self.data.base_crate;

        let writes = fields.iter().map(|field| {
            let tmp_name = &field.tmp_name;
            let ty = &field.ty;
            quote! {
                <#ty as #base_crate::SmolWrite>::write(#tmp_name, #tuple_writer.write_value())?;
            }
        });

        quote! {
            #(#writes)*
        }
    }

    /// writes fields from their tmp names into the writer
    /// writer_init: Fn(num_fields) -> writer_init_tokens
    fn gen_struct_write_core(
        &self,
        fields: &[StructField],
        writer_init: &dyn Fn(&Ident) -> TokenStream,
    ) -> TokenStream {
        let base_crate = &self.data.base_crate;

        let mut const_fields = 0usize;
        let mut variable_field_counters = vec![];
        let mut write_fields = Vec::with_capacity(fields.len());

        let num_fields = Ident::new("num_fields", Span::call_site());
        let struct_writer = Ident::new("struc", Span::call_site());

        let init_tokens = writer_init(&num_fields);

        for field in fields {
            let ty = &field.ty;
            let tmp_name = &field.tmp_name;
            let data_name = &field.data_name;

            if field.optimize_option {
                variable_field_counters.push(quote! {
                    if <#ty as #base_crate::OptionalFieldOptimization>::get(#tmp_name).is_some() {
                        #num_fields += 1;
                    }
                });
                write_fields.push(quote! {
                    if let Some(f) = <#ty as #base_crate::OptionalFieldOptimization>::get(#tmp_name) {
                        <<#ty as #base_crate::OptionalFieldOptimization>::Inner as #base_crate::SmolWrite>::write(f, #struct_writer.write_field(#data_name)?)?;
                    }
                });
            } else {
                const_fields += 1;
                write_fields.push(quote! {
                    <#ty as #base_crate::SmolWrite>::write(#tmp_name, #struct_writer.write_field(#data_name)?)?;
                });
            }
        }

        quote! {
            let mut #num_fields = #const_fields;
            #(#variable_field_counters)*
            let mut #struct_writer = #init_tokens;

            #(#write_fields)*
        }
    }

    fn gen_struct_write(&self, ty: &StructType, writer: &Ident) -> TokenStream {
        let base_crate = &self.data.base_crate;
        match ty {
            StructType::Unit => {
                quote! {
                    #writer.write_unit_struct()
                }
            }
            StructType::Newtype(f) => {
                let TupleField { tmp_name, ty } = f;
                quote! {
                    let Self(#tmp_name) = self;
                    let writer = #writer.write_newtype_struct()?;
                    <#ty as #base_crate::SmolWrite>::write(#tmp_name, writer)
                }
            }
            StructType::Tuple(fs) => {
                let tmp_names = fs.iter().map(|f| &f.tmp_name);
                let nfields = fs.len();
                let tuple_writer = Ident::new("tup", Span::call_site());
                let imp = self.gen_tuple_write_core(fs, &tuple_writer);
                quote! {
                    let Self(#(#tmp_names),*) = self;
                    let mut #tuple_writer = #writer.write_tuple_struct(#nfields)?;
                    #imp
                    Ok(())
                }
            }
            StructType::Struct(fs) => {
                let names = fs.iter().map(|f| &f.name_ident);
                let tmp_names = fs.iter().map(|f| &f.tmp_name);
                let imp = self.gen_struct_write_core(fs, &|num_fields| {
                    quote! {
                        #writer.write_struct(#num_fields)?
                    }
                });

                quote! {
                    let Self { #(#names: #tmp_names),* } = self;
                    #imp
                    Ok(())
                }
            }
        }
    }

    fn gen_enum_write(&self, variants: &[EnumVariant], writer: &Ident) -> TokenStream {
        let base_crate = &self.data.base_crate;

        let member_impls = variants.iter().map(|v| {
            let EnumVariant { name_ident, display_name: _, data_name, ty } = v;

            match ty {
                StructType::Unit => quote! {
                    Self::#name_ident => {
                        writer.write_unit_variant(#data_name)
                    }
                },
                StructType::Newtype(f) => {
                    let TupleField { tmp_name, ty } = f;
                    quote! {
                        Self::#name_ident(#tmp_name) => {
                            let writer = #writer.write_newtype_variant(#data_name)?;
                            <#ty as #base_crate::SmolWrite>::write(#tmp_name, writer)
                        }
                    }
                },
                StructType::Tuple(fs) => {
                    let tmp_names = fs.iter().map(|f| &f.tmp_name);
                    let nfields = fs.len();
                    let tuple_writer = Ident::new("tup", Span::call_site());
                    let imp = self.gen_tuple_write_core(fs, &tuple_writer);
                    quote! {
                        Self::#name_ident(#(#tmp_names),*) => {
                            let mut #tuple_writer = #writer.write_tuple_variant(#data_name, #nfields)?;
                            #imp
                            Ok(())
                        }
                    }
                }
                StructType::Struct(fs) => {
                    let names = fs.iter().map(|f| &f.name_ident);
                    let tmp_names = fs.iter().map(|f| &f.tmp_name);
                    let imp = self.gen_struct_write_core(fs, &|num_fields| {
                        quote! {
                            #writer.write_struct_variant(#data_name, #num_fields)?
                        }
                    });

                    quote! {
                        Self::#name_ident { #(#names: #tmp_names),* } => {
                            #imp
                            Ok(())
                        }
                    }
                },
            }
        });
        quote! {
            match self {
                #(#member_impls)*
            }
        }
    }

    /// reads fields from tuple_reader into fields tmp names
    fn gen_tuple_read_core(&self, fields: &[TupleField], tuple_reader: &Ident) -> TokenStream {
        let nfields = fields.len();
        let base_crate = &self.data.base_crate;

        let reads = fields.iter().map(|field| {
            let tmp_name = &field.tmp_name;
            let ty = &field.ty;

            quote! {
                let Some(#tmp_name) = #tuple_reader.read_value() else {
                    return Err(length_error().into());
                };

                let #tmp_name = <#ty as #base_crate::SmolRead>::read(#tmp_name)?;
            }
        });

        quote! {
            let length = #tuple_reader.remaining();
            let length_error = || {
                #base_crate::reader::ReadError::UnexpectedLength {
                    expected: 3,
                    got: length,
                    type_name: ::std::any::type_name::<Self>(),
                }
            };

            if length != #nfields {
                return Err(length_error().into());
            }

            #(#reads)*
        }
    }

    /// reads fields from struct_reader into fields tmp names
    /// `struct_builder: Fn(field_setters: ...) -> TokenStream`
    fn gen_struct_read_core(&self, fields: &[StructField], struct_reader: &Ident) -> TokenStream {
        let base_crate = &self.data.base_crate;

        let tmp_defs = fields.iter().map(|f| {
            let ty = &f.ty;
            let tmp_name = &f.tmp_name;

            quote! {
                let mut #tmp_name = None::<#ty>;
            }
        });

        let reads = fields.iter().map(|f| {
            let data_name = &f.data_name;
            let display_name = &f.display_name;
            let ty = &f.ty;
            let tmp_name = &f.tmp_name;

            let reading_preoptopt = quote! {
                #tmp_name = Some(<#ty as #base_crate::SmolRead>::read(field_reader)?);
            };

            let reading = if f.optimize_option {
                quote! {
                    if format_version >= <#ty as #base_crate::OptionalFieldOptimization>::MIN_FORMAT_VERSION {
                        #tmp_name = Some(
                            <#ty as #base_crate::OptionalFieldOptimization>::make_some(
                                <<#ty as #base_crate::OptionalFieldOptimization>::Inner as #base_crate::SmolRead>::read(field_reader)?
                            )
                        );
                    } else {
                        #reading_preoptopt
                    }
                }
            } else {
                reading_preoptopt
            };

            quote! {
                #data_name => {
                    if #tmp_name.is_some() {
                        return Err(#base_crate::reader::ReadError::DuplicateStructField {
                            name: #display_name,
                            type_name: ::std::any::type_name::<Self>(),
                        }
                        .into());
                    }
                    #reading
                }
            }
        });

        let unwraps = fields.iter().map(|f| {
            let display_name = &f.display_name;
            let ty = &f.ty;
            let tmp_name = &f.tmp_name;

            let missing_field_err = quote! {
                #base_crate::reader::ReadError::MissingStructField {
                    name: #display_name,
                    type_name: ::std::any::type_name::<Self>(),
                }
            };

            if f.optimize_option {
                quote! {
                    let #tmp_name = match (#tmp_name, (format_version >= <#ty as #base_crate::OptionalFieldOptimization>::MIN_FORMAT_VERSION)) {
                        (Some(v), _) => v,
                        (None, true) => {
                            <#ty as #base_crate::OptionalFieldOptimization>::make_none()
                        }
                        (None, false) => {
                            return Err(#missing_field_err.into())
                        }
                    };
                }
            }
            else {
                quote! {
                    let #tmp_name = #tmp_name.ok_or_else(|| #missing_field_err)?;
                }
            }
        });

        quote! {
            let format_version = #struct_reader.format_version();

            #(#tmp_defs)*

            while let Some((field_name, field_reader)) = #struct_reader.read_field()? {
                match ::std::ops::Deref::deref(&field_name) {
                    #(#reads)*

                    _ => {
                        return Err(#base_crate::reader::ReadError::UnexpectedStructField {
                            name: field_name,
                            type_name: ::std::any::type_name::<Self>(),
                        }
                        .into())
                    }
                }
            }

            #(#unwraps)*
        }
    }
    
    fn gen_struct_read(&self, ty: &StructType, reader: &Ident) -> TokenStream {
        let base_crate = &self.data.base_crate;
        match ty {
            StructType::Unit => quote! {
                #reader.take_unit_struct().map_err(|e|
                    #base_crate::reader::ReadError::from(
                        e.with_type_name_of::<Self>()
                    )
                )?;
                Ok(Self)
            },
            StructType::Newtype(f) => {
                let ty = &f.ty;
                let tmp_name = &f.tmp_name;
                quote! {
                    let #reader = #reader.take_newtype_struct().map_err(|e|
                        #base_crate::reader::ReadError::from(
                            e.with_type_name_of::<Self>()
                        )
                    )?;
                    let #tmp_name = <#ty as #base_crate::SmolRead>::read(#reader)?;
                    Ok(Self(#tmp_name))
                }
            },
            StructType::Tuple(fs) => {
                let tuple_reader = Ident::new("tup", Span::call_site());
                let imp = self.gen_tuple_read_core(fs, &tuple_reader);
                let tmp_names = fs.iter().map(|f| &f.tmp_name);
                quote! {
                    let mut #tuple_reader = #reader.take_tuple_struct().map_err(|e|
                        #base_crate::reader::ReadError::from(
                            e.with_type_name_of::<Self>()
                        )
                    )?;
                    #imp
                    Ok(Self(#(#tmp_names),*))
                }
            },
            StructType::Struct(fs) => {
                let struct_reader = Ident::new("struc", Span::call_site());
                let imp = self.gen_struct_read_core(fs, &struct_reader);
                let tmp_names = fs.iter().map(|f| &f.tmp_name);
                let names = fs.iter().map(|f| &f.name_ident);
                quote! {
                    let mut #struct_reader = #reader.take_field_struct().map_err(|e|
                        #base_crate::reader::ReadError::from(
                            e.with_type_name_of::<Self>()
                        )
                    )?;
                    #imp
                    Ok(Self{ #(#names: #tmp_names),* })
                }
            },
        }
    }
    
    fn gen_enum_read(&self, variants: &[EnumVariant], reader: &Ident) -> TokenStream {
        let base_crate = &self.data.base_crate;
        let variants_impl = variants.iter().map(|var| {
            let data_name = &var.data_name;
            let name = &var.name_ident;
            let display_name = &var.display_name;

            match &var.ty {
                StructType::Unit => {
                    quote! {
                        #data_name => {
                            #reader
                                .take_unit_variant()
                                .map_err(|e|
                                    #base_crate::reader::ReadError::from(
                                        e.with_variant_name(::std::any::type_name::<Self>(), #display_name)
                                    )
                                )?;
                            Ok(Self::#name)
                        }
                    }
                }
                StructType::Newtype(f) => {
                    let ty = &f.ty;
                    let tmp_name = &f.tmp_name;
                    quote! {
                        #data_name => {
                            let #reader = #reader
                                .take_newtype_variant()
                                .map_err(|e|
                                    #base_crate::reader::ReadError::from(
                                        e.with_variant_name_of::<Self>(#display_name)
                                    )
                                )?;
                            let #tmp_name = <#ty as #base_crate::SmolRead>::read(#reader)?;
                            Ok(Self::#name(#tmp_name))
                        }
                    }
                }

                StructType::Tuple(fs) => {
                    let tuple_reader = Ident::new("tup", Span::call_site());
                    let imp = self.gen_tuple_read_core(fs, &tuple_reader);
                    let tmp_names = fs.iter().map(|f| &f.tmp_name);
                    quote! {
                        #data_name => {
                            let mut #tuple_reader = #reader
                                .take_tuple_variant()
                                .map_err(|e|
                                    #base_crate::reader::ReadError::from(
                                        e.with_variant_name_of::<Self>(#display_name)
                                    )
                                )?;
                            #imp
                            Ok(Self::#name(#(#tmp_names),*))
                        }
                    }
                }
                StructType::Struct(fs) => {
                    let struct_reader = Ident::new("struc", Span::call_site());
                    let imp = self.gen_struct_read_core(fs, &struct_reader);
                    let names = fs.iter().map(|f| &f.name_ident);
                    let tmp_names = fs.iter().map(|f| &f.tmp_name);
                    quote! {
                        #data_name => {
                            let mut #struct_reader = #reader
                                .take_field_variant()
                                .map_err(|e|
                                    #base_crate::reader::ReadError::from(
                                        e.with_variant_name_of::<Self>(#display_name)
                                    )
                                )?;
                            #imp
                            Ok(Self::#name { #(#names: #tmp_names),* })
                        }
                    }
                }
            }
        });

        quote! {
            let (name, #reader) = #reader
                .take_enum()
                .map_err(|e| e.with_type_name_of::<Self>())
                .map_err(#base_crate::reader::ReadError::from)?
                .read_variant()?;

            match std::ops::Deref::deref(&name) {
                #(#variants_impl)*

                _ => {
                    return Err(#base_crate::reader::ReadError::UnexpectedEnumVariant {
                        name,
                        type_name: ::std::any::type_name::<Self>(),
                    }
                    .into())
                }
            }
        }
    }
}

pub struct CodegenData {
    pub ty: TraitTypeAll,
    pub type_name: Ident,
    pub read_generics: Generics,
    pub write_generics: Generics,
    pub inty: InputType,

    pub base_crate: syn::Path,
}

pub enum InputType {
    Struct(StructType),
    Enum(Vec<EnumVariant>),
}

pub struct EnumVariant {
    pub name_ident: Ident,
    pub display_name: String,
    pub data_name: StringLitOrPath,
    pub ty: StructType,
}

pub enum StructType {
    Unit,
    Newtype(TupleField),
    Tuple(Vec<TupleField>),
    Struct(Vec<StructField>),
}

pub struct StructField {
    pub name_ident: Ident,
    pub tmp_name: Ident,
    pub display_name: String,
    pub data_name: StringLitOrPath,
    pub ty: Type,

    pub optimize_option: bool,
}

pub struct TupleField {
    pub tmp_name: Ident,
    pub ty: Type,
}
