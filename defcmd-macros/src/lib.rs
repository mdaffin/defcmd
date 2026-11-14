mod expand;
mod model;

use proc_macro::TokenStream;
use syn::{ItemFn, parse_macro_input};

/// Marks a function as a command. `fn main` becomes the root command and the program entry
/// point. Any other function becomes a subcommand of the root, named after the function.
#[proc_macro_attribute]
pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    if !attr.is_empty() {
        let attr = proc_macro2::TokenStream::from(attr);
        return syn::Error::new_spanned(attr, "`#[command]` arguments are not supported yet")
            .to_compile_error()
            .into();
    }
    if func.sig.ident == "main" {
        return expand::expand_main(func)
            .unwrap_or_else(|e| e.to_compile_error())
            .into();
    }
    expand::expand_sub(func)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
