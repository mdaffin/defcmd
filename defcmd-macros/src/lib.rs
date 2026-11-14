mod expand;
mod model;

use proc_macro::TokenStream;
use syn::{ItemFn, LitStr, parse_macro_input};

/// Marks a function as a command. `fn main` becomes the root command and the program entry
/// point. Any other function becomes a subcommand of the root, named after the function, or
/// by the path given as a string: `#[command("remote add")]`.
#[proc_macro_attribute]
pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let path = match parse_attr(attr) {
        Ok(path) => path,
        Err(e) => return e.to_compile_error().into(),
    };
    let result = if func.sig.ident == "main" {
        match path {
            None => expand::expand_main(func),
            Some(_) => Err(syn::Error::new_spanned(
                &func.sig.ident,
                "the root command `main` takes no path",
            )),
        }
    } else {
        expand::expand_sub(func, path)
    };
    result.unwrap_or_else(|e| e.to_compile_error()).into()
}

/// The path words of `#[command("a b")]`, or `None` for a bare `#[command]`.
fn parse_attr(attr: TokenStream) -> syn::Result<Option<Vec<String>>> {
    if attr.is_empty() {
        return Ok(None);
    }
    let lit: LitStr = syn::parse(attr).map_err(|e| {
        syn::Error::new(
            e.span(),
            "`#[command(..)]` takes only a path string, like \"remote add\"",
        )
    })?;
    model::parse_path(&lit).map(Some)
}
