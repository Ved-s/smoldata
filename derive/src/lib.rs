use attributes::ParseAttribute;
use proc_macro2::{Ident, TokenStream};

mod attributes;
mod codegen;
mod parsing;
mod processing;

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
    match derive_from_tokens_inner(ty, tokens.into()) {
        Ok(t) => t.into(),
        Err(e) => e.into_compile_error().into(),
    }
}

fn derive_from_tokens_inner(ty: TraitTypeAll, tokens: TokenStream) -> syn::Result<TokenStream> {
    let input = parsing::parse_input(ty, tokens)?;
    let data = processing::process_input(input)?;
    let codegen = codegen::Codegen::new(data);
    Ok(codegen.generate())
}

struct ErrorCollector {
    error: Option<syn::Error>,
}

impl ErrorCollector {
    pub fn new() -> Self {
        Self { error: None }
    }

    pub fn add(&mut self, e: syn::Error) {
        if let Some(error) = &mut self.error {
            error.combine(e);
        } else {
            self.error = Some(e);
        }
    }

    pub fn wrap<T>(self, ok: T) -> syn::Result<T> {
        match self.error {
            Some(e) => Err(e),
            None => Ok(ok),
        }
    }

    pub fn try_add<T>(&mut self, res: syn::Result<T>) -> Option<T> {
        match res {
            Ok(v) => Some(v),
            Err(e) => {
                self.add(e);
                None
            }
        }
    }
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

fn ident_nonraw_string(ident: &Ident) -> String {
    let mut string = ident.to_string();
    if string.starts_with("r#") {
        string.replace_range(0..2, "");
    }
    string
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

impl ParseAttribute for Empty {
    fn parse(_input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Self)
    }
}

#[derive(Clone, Copy)]
struct Empty;
