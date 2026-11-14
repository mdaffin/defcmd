mod derive;
mod expand;
mod model;

use proc_macro::TokenStream;
use syn::{DeriveInput, ItemFn, parse_macro_input};

/// Marks a function as a command. `fn main` becomes the root command and the program entry
/// point. Any other function becomes a subcommand of the root, named after the function, or
/// by the path given as a string: `#[command("remote add")]`. `alias = name` adds another name.
#[proc_macro_attribute]
pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let attr = match syn::parse::Parser::parse(model::parse_command_attr, attr) {
        Ok(attr) => attr,
        Err(e) => return e.to_compile_error().into(),
    };
    let result = if func.sig.ident == "main" {
        match attr {
            model::CommandAttr {
                path: None,
                aliases,
            } if aliases.is_empty() => expand::expand_main(func),
            model::CommandAttr { path: Some(_), .. } => Err(syn::Error::new_spanned(
                &func.sig.ident,
                "the root command `main` takes no path",
            )),
            _ => Err(syn::Error::new_spanned(
                &func.sig.ident,
                "the root command `main` takes no alias",
            )),
        }
    } else {
        expand::expand_sub(func, attr)
    };
    result.unwrap_or_else(|e| e.to_compile_error()).into()
}

/// Implements `FromArg` for an enum whose variants have no fields. The accepted spellings are the
/// kebab-case variant names (`FastAndLoose` is `fast-and-loose`), exact and case sensitive.
#[proc_macro_derive(FromArg)]
pub fn derive_from_arg(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive::expand_derive(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
