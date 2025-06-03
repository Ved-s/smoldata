use proc_macro2::Span;
use syn::spanned::Spanned;

use crate::{ident_nonraw_string, Empty, StringLitOrPath};

pub trait AttributeStruct {
    fn target_item_name(&self) -> &str;
    fn valid_attribute(&self, name: &str) -> bool;
    fn read_attribute(
        &mut self,
        name: &str,
        name_span: Span,
        reader: syn::parse::ParseStream,
    ) -> syn::Result<()>;
}

pub trait ParseAttribute: Sized {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self>;
}

impl<T: syn::parse::Parse> ParseAttribute for T {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        <syn::Token![=] as syn::parse::Parse>::parse(input)?;
        <Self as syn::parse::Parse>::parse(input)
    }
}

macro_rules! define_attributes {
    (@parse $reader:ident ()) => { () };
    (@parse $reader:ident $attrty:ty) => {{

    }};
    {
        #[target = $target:literal]
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
               $attrvis:vis  $attrname:ident: $attrty:ty
            ),* $(,)?
        }
    } => {
        $(#[$meta])*
        #[derive(Default)]
        $vis struct $name {
            $(
                $attrvis $attrname: Option<Attribute<$attrty>>,
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
                            self.$attrname = Some(Attribute {
                                span: name_span,
                                attr: <$attrty as ParseAttribute>::parse(reader)?
                            });
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

#[derive(Clone)]
pub struct Attribute<T> {
    pub span: Span,
    pub attr: T,
}

define_attributes! {
    #[target = "enum variant"]
    pub struct VariantAttributes {
        pub rename: StringLitOrPath,
    }
}

define_attributes! {
    #[target = "struct field"]
    pub struct StructFieldAttributes {
        pub rename: StringLitOrPath,
        pub do_opt_option: Empty,
        pub dont_opt_option: Empty,
    }
}

define_attributes! {
    #[target = "type"]
    pub struct InputTypeAttributes {
        pub smoldata: syn::Path
    }
}

impl StructFieldAttributes {
    pub fn do_optimize_option(&self, ty: &syn::Type) -> bool {
        if self.dont_opt_option.is_some() {
            return false;
        }

        if self.do_opt_option.is_some() {
            return true;
        }

        let syn::Type::Path(path) = ty else {
            return false;
        };

        if path.qself.is_some() {
            return false;
        }

        let mut matching_std_option = false;
        for (i, v) in path.path.segments.iter().enumerate() {
            let name = v.ident.to_string();
            if i == 0 {
                if path.path.leading_colon.is_none()
                    && name == "Option"
                    && path.path.segments.len() == 1
                {
                    return true;
                } else if (name == "std" || name == "core") && path.path.segments.len() == 3 {
                    matching_std_option = true;
                    continue;
                } else {
                    return false;
                }
            } else if i == 1 && matching_std_option && name == "option" {
                continue;
            } else if i == 2 && matching_std_option && name == "Option" {
                return true;
            } else {
                return false;
            }
        }

        false
    }

    pub fn verify_attributes(&self) -> syn::Result<()> {
        if let Some((d, dn)) = self
            .do_opt_option
            .as_ref()
            .zip(self.dont_opt_option.as_ref())
        {
            let mut err1 = syn::Error::new(
                d.span,
                "Both `do_opt_option` and `dont_opt_option` are specified",
            );
            let err2 = syn::Error::new(
                dn.span,
                "Both `do_opt_option` and `dont_opt_option` are specified",
            );

            err1.combine(err2);

            return Err(err1);
        }

        Ok(())
    }
}

// fn read_attributes<T: AttributeStruct + Default>(attrs: &[syn::Attribute]) -> syn::Result<T> {
//     let mut val = T::default();
//     read_attributes_into(attrs, &mut val)?;
//     Ok(val)
// }

pub fn disallow_attributes(attrs: &[syn::Attribute]) -> syn::Result<()> {
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

pub fn read_attributes_into(
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
