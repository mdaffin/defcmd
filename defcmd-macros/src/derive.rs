//! `#[derive(FromArg)]` for fieldless enums.

use proc_macro2::TokenStream;
use quote::quote;
use syn::ext::IdentExt;
use syn::{Data, DeriveInput, Fields};

/// The kebab-case spelling of a variant name: `FastAndLoose` is `fast-and-loose`. A hyphen goes
/// before an upper-case letter that follows a lower-case letter or digit, or that ends an
/// acronym (`HTTPServer` is `http-server`).
pub fn kebab_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' {
            out.push('-');
            continue;
        }
        if c.is_uppercase() && i > 0 && chars[i - 1] != '_' {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if !prev.is_uppercase() || next_lower {
                out.push('-');
            }
        }
        out.extend(c.to_lowercase());
    }
    out
}

pub fn expand_derive(input: DeriveInput) -> syn::Result<TokenStream> {
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`#[derive(FromArg)]` only supports enums whose variants have no fields",
        ));
    };
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`#[derive(FromArg)]` needs at least one variant",
        ));
    }
    let mut names = Vec::new();
    let mut idents = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "`#[derive(FromArg)]` only supports enums whose variants have no fields",
            ));
        }
        let name = kebab_case(&variant.ident.unraw().to_string());
        if names.contains(&name) {
            return Err(syn::Error::new_spanned(
                variant,
                format!("two variants are spelled `{name}` on the command line"),
            ));
        }
        names.push(name);
        idents.push(&variant.ident);
    }
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let expected = format!("expected one of: {}", names.join(", "));
    let type_name = names.join("|");
    let shown: Vec<String> = names.iter().map(|n| format!("'{n}'")).collect();
    Ok(quote! {
        impl #impl_generics ::defcmd::FromArg for #ident #ty_generics #where_clause {
            fn from_arg(
                arg: &::std::ffi::OsStr,
            ) -> ::core::result::Result<Self, ::defcmd::ArgError> {
                match arg.to_str() {
                    #(::core::option::Option::Some(#names) => ::core::result::Result::Ok(Self::#idents),)*
                    _ => ::core::result::Result::Err(::defcmd::ArgError::new(#expected)),
                }
            }

            fn possible_values() -> &'static [&'static str] {
                &[#(#names),*]
            }

            fn type_name() -> ::std::string::String {
                ::std::string::String::from(#type_name)
            }

            fn display_arg(&self) -> ::core::option::Option<::std::string::String> {
                ::core::option::Option::Some(::std::string::String::from(match self {
                    #(Self::#idents => #shown,)*
                }))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn kebab_case_splits_words() {
        for (from, to) in [
            ("Fast", "fast"),
            ("FastAndLoose", "fast-and-loose"),
            ("HTTPServer", "http-server"),
            ("Ipv4", "ipv4"),
            ("V2Beta", "v2-beta"),
            ("A", "a"),
            ("ABC", "abc"),
            ("snake_case", "snake-case"),
            ("r#type", "r#type"),
        ] {
            assert_eq!(kebab_case(from), to, "{from}");
        }
    }

    fn rejected(input: DeriveInput) -> String {
        expand_derive(input).unwrap_err().to_string()
    }

    #[test]
    fn only_fieldless_enums_are_supported() {
        let msg = "`#[derive(FromArg)]` only supports enums whose variants have no fields";
        assert_eq!(rejected(parse_quote! { struct S; }), msg);
        assert_eq!(rejected(parse_quote! { struct S { a: u8 } }), msg);
        assert_eq!(rejected(parse_quote! { union U { a: u8 } }), msg);
        assert_eq!(rejected(parse_quote! { enum E { A(u8) } }), msg);
        assert_eq!(rejected(parse_quote! { enum E { A { x: u8 } } }), msg);
        assert_eq!(
            rejected(parse_quote! { enum E {} }),
            "`#[derive(FromArg)]` needs at least one variant"
        );
    }

    #[test]
    fn variants_that_collide_are_rejected() {
        assert_eq!(
            rejected(parse_quote! { enum E { FooBar, Foo_Bar } }),
            "two variants are spelled `foo-bar` on the command line"
        );
    }

    #[test]
    fn a_fieldless_enum_expands() {
        assert!(expand_derive(parse_quote! { enum Mode { Fast, Careful } }).is_ok());
        assert!(expand_derive(parse_quote! { enum Mode { Fast = 1, Careful = 2 } }).is_ok());
    }
}
