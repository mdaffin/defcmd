#[allow(dead_code)] // used by the expansion in M4.2
mod model;

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

/// Marks a function as a command. Currently re-emits the function unchanged.
#[proc_macro_attribute]
pub fn command(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    quote!(#func).into()
}
