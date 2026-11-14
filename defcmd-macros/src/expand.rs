//! Code generation for `#[command]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{FnArg, ItemFn};

use crate::model::{Command, Param, TypeKind};

/// Rejects parameter combinations that are not supported, so nothing is silently miscompiled.
fn check_supported(command: &Command) -> syn::Result<()> {
    for p in &command.params {
        let supported = if p.flag {
            p.kind == TypeKind::Bool
        } else {
            !matches!(p.kind, TypeKind::Option | TypeKind::Vec)
        };
        if !supported {
            return Err(syn::Error::new_spanned(
                &p.ty,
                "this parameter type is not supported yet",
            ));
        }
    }
    Ok(())
}

/// The `ParamSpec` expression for one parameter.
fn param_spec(p: &Param) -> TokenStream {
    let name = &p.name;
    let doc = &p.doc;
    let ty = &p.ty;
    let (kind, long) = if p.flag {
        let long = p.name.replace('_', "-");
        (quote!(Switch), quote!(::core::option::Option::Some(#long)))
    } else {
        (quote!(Positional), quote!(::core::option::Option::None))
    };
    quote! {
        ::defcmd::ParamSpec {
            name: #name,
            kind: ::defcmd::ParamKind::#kind,
            long: #long,
            short: ::core::option::Option::None,
            doc: #doc,
            type_name: <#ty as ::defcmd::FromArg>::type_name,
            possible_values: <#ty as ::defcmd::FromArg>::possible_values,
            default_display: ::core::option::Option::None,
        }
    }
}

/// The glue statement that converts parameter `index` into a local named like the parameter.
fn convert(index: usize, p: &Param) -> TokenStream {
    let ident = &p.ident;
    let ty = &p.ty;
    if p.flag {
        quote!(let #ident: #ty = __parsed.get_opt::<#ty>(#index)?.unwrap_or_default();)
    } else {
        quote!(let #ident: #ty = __parsed.get::<#ty>(#index)?;)
    }
}

/// The command name a bare `#[command]` gives a function: `_` shown as `-`, `r#` stripped.
pub fn command_name(ident: &syn::Ident) -> String {
    ident.unraw().to_string().replace('_', "-")
}

/// Expands the root command: `fn main` is renamed and the real entry point is emitted.
pub fn expand_main(func: ItemFn) -> syn::Result<TokenStream> {
    expand(func, true)
}

/// Expands a subcommand: the function keeps its name and signature and is registered under the
/// name derived from it.
pub fn expand_sub(func: ItemFn) -> syn::Result<TokenStream> {
    expand(func, false)
}

fn expand(mut func: ItemFn, root: bool) -> syn::Result<TokenStream> {
    let command = Command::from_fn(&func)?;
    check_supported(&command)?;

    // The user's function keeps its body and signature, minus the parameter attributes that
    // only this macro understands (`#[flag]`, doc comments).
    let inner = if root {
        let inner = format_ident!("__defcmd_main");
        func.sig.ident = inner.clone();
        inner
    } else {
        func.sig.ident.clone()
    };
    let path: Vec<String> = if root {
        Vec::new()
    } else {
        vec![command_name(&func.sig.ident)]
    };
    for arg in &mut func.sig.inputs {
        if let FnArg::Typed(t) = arg {
            t.attrs.clear();
        }
    }

    let doc = &command.doc;
    let count = command.params.len();
    let specs = command.params.iter().map(param_spec);
    let converts = command
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| convert(i, p));
    let idents = command.params.iter().map(|p| &p.ident);

    let entry = root.then(|| {
        quote! {
            #[allow(dead_code)]
            fn main() -> ::std::process::ExitCode {
                ::defcmd::__private::main(::core::option_env!("CARGO_BIN_NAME"))
            }
        }
    });

    Ok(quote! {
        #func

        const _: () = {
            static __PARAMS: [::defcmd::ParamSpec; #count] = [#(#specs),*];
            static __SPEC: ::defcmd::CommandSpec = ::defcmd::CommandSpec {
                path: &[#(#path),*],
                aliases: &[],
                doc: #doc,
                params: &__PARAMS,
                version: ::core::option_env!("CARGO_PKG_VERSION"),
            };

            fn __glue(
                __parsed: &::defcmd::Parsed,
                __err: &mut dyn ::std::io::Write,
            ) -> ::core::result::Result<::std::process::ExitCode, ::defcmd::UsageError> {
                // Taken before the parameters are bound, so a parameter named like the
                // function cannot hide it.
                let __command = #inner;
                #(#converts)*
                ::core::result::Result::Ok(::defcmd::Output::report(
                    __command(#(#idents),*),
                    __err,
                ))
            }

            ::defcmd::__private::inventory::submit! {
                ::defcmd::Registration { spec: &__SPEC, run: __glue }
            }
        };

        #entry
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn names_show_underscores_as_dashes_and_lose_raw_prefixes() {
        assert_eq!(command_name(&parse_quote!(check)), "check");
        assert_eq!(command_name(&parse_quote!(list_all)), "list-all");
        assert_eq!(command_name(&parse_quote!(r#match)), "match");
    }
}
