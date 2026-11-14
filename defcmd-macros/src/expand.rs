//! Code generation for `#[command]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
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

/// Expands the root command: `fn main` is renamed and the real entry point is emitted.
pub fn expand_main(mut func: ItemFn) -> syn::Result<TokenStream> {
    let command = Command::from_fn(&func)?;
    check_supported(&command)?;

    // The user's function keeps its body and signature, minus the parameter attributes that
    // only this macro understands (`#[flag]`, doc comments).
    let inner = format_ident!("__defcmd_main");
    func.sig.ident = inner.clone();
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

    Ok(quote! {
        #func

        const _: () = {
            static __PARAMS: [::defcmd::ParamSpec; #count] = [#(#specs),*];
            static __SPEC: ::defcmd::CommandSpec = ::defcmd::CommandSpec {
                path: &[],
                aliases: &[],
                doc: #doc,
                params: &__PARAMS,
                version: ::core::option_env!("CARGO_PKG_VERSION"),
            };

            fn __glue(
                __parsed: &::defcmd::Parsed,
                __err: &mut dyn ::std::io::Write,
            ) -> ::core::result::Result<::std::process::ExitCode, ::defcmd::UsageError> {
                #(#converts)*
                ::core::result::Result::Ok(::defcmd::Output::report(
                    #inner(#(#idents),*),
                    __err,
                ))
            }

            ::defcmd::__private::inventory::submit! {
                ::defcmd::Registration { spec: &__SPEC, run: __glue }
            }
        };

        #[allow(dead_code)]
        fn main() -> ::std::process::ExitCode {
            ::defcmd::__private::main(::core::option_env!("CARGO_BIN_NAME"))
        }
    })
}
