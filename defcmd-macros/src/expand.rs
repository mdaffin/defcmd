//! Code generation for `#[command]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{Expr, ExprLit, FnArg, ItemFn, Lit, Type};

use crate::model::{Command, Param, TypeKind, inner_type};

/// Rejects parameter combinations that are not supported, so nothing is silently miscompiled.
fn check_supported(command: &Command) -> syn::Result<()> {
    let unsupported = |p: &Param, what: &str| {
        Err(syn::Error::new_spanned(
            &p.ty,
            format!("{what} is not supported yet"),
        ))
    };
    let mut seen_optional = false;
    let mut seen_rest = false;
    for p in &command.params {
        if p.rest && (p.flag || p.default.is_some()) {
            return Err(syn::Error::new_spanned(
                &p.ty,
                "`#[rest]` cannot be combined with `#[flag]` or `#[default]`",
            ));
        }
        if p.rest && p.kind != TypeKind::Vec {
            return Err(syn::Error::new_spanned(&p.ty, "`#[rest]` needs a `Vec<T>`"));
        }
        if !p.flag && seen_rest {
            return Err(syn::Error::new_spanned(
                &p.ty,
                "the rest positional must be the last positional",
            ));
        }
        if is_rest(p) {
            seen_rest = true;
            if p.default.is_some() {
                return Err(syn::Error::new_spanned(
                    &p.ty,
                    "`#[default]` cannot be combined with a `Vec` positional",
                ));
            }
            continue;
        }
        if !p.flag {
            if is_optional(p) {
                seen_optional = true;
            } else if seen_optional && p.kind != TypeKind::Vec {
                return Err(syn::Error::new_spanned(
                    &p.ty,
                    "a required positional cannot follow an optional one",
                ));
            }
        }
        match (p.flag, p.kind, p.default.is_some()) {
            (false, TypeKind::Option, true) => {
                return Err(syn::Error::new_spanned(
                    &p.ty,
                    "`#[default]` cannot be combined with `Option`",
                ));
            }
            (false, TypeKind::Option, false) => {}
            (_, TypeKind::Option | TypeKind::Vec, _) => {
                return unsupported(p, "this parameter type");
            }
            (true, TypeKind::Bool, true) => return unsupported(p, "`#[default]` on a switch"),
            (true, TypeKind::Other, false) => {
                return Err(syn::Error::new_spanned(
                    &p.ty,
                    "a flag of this type needs `#[default(..)]`",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// A positional that may be left out: `Option<T>` or one with `#[default]`.
fn is_optional(p: &Param) -> bool {
    !p.flag && (p.kind == TypeKind::Option || p.default.is_some())
}

/// The variadic positional: marked `#[rest]` or a `Vec<T>` that is not a flag.
fn is_rest(p: &Param) -> bool {
    !p.flag && (p.rest || p.kind == TypeKind::Vec)
}

/// The type `FromArg` converts to: `T` for `Option<T>` and rest positionals, the type itself
/// otherwise.
fn value_type(p: &Param) -> &Type {
    match (p.kind, inner_type(&p.ty)) {
        (TypeKind::Option, Some(inner)) => inner,
        (TypeKind::Vec, Some(inner)) if is_rest(p) => inner,
        _ => &p.ty,
    }
}

/// The name of the generated function that produces the default of parameter `index`.
fn default_fn(index: usize) -> syn::Ident {
    format_ident!("__defcmd_default_{index}")
}

/// The name of the generated function that renders the default of parameter `index` for help.
fn default_display_fn(index: usize) -> syn::Ident {
    format_ident!("__defcmd_default_display_{index}")
}

/// The body of the default function: the expression, except that a string literal goes through
/// `From` so `#[default("x")]` works for `String`, `PathBuf` and `OsString`.
fn default_value(expr: &Expr, ty: &Type) -> TokenStream {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(_), ..
        }) => quote!(<#ty as ::core::convert::From<&'static str>>::from(#expr)),
        _ => quote!(#expr),
    }
}

/// The functions that evaluate and render the default of parameter `index`, if it has one. They
/// are free functions so the expression cannot see the glue's parameter locals.
fn default_fns(index: usize, p: &Param) -> Option<TokenStream> {
    let expr = p.default.as_ref()?;
    let ty = &p.ty;
    let make = default_fn(index);
    let display = default_display_fn(index);
    let value = default_value(expr, ty);
    Some(quote! {
        fn #make() -> #ty {
            #value
        }
        fn #display() -> ::core::option::Option<::std::string::String> {
            ::defcmd::FromArg::display_arg(&#make())
        }
    })
}

/// The `ParamSpec` expression for one parameter.
fn param_spec(index: usize, p: &Param) -> TokenStream {
    let name = &p.name;
    let doc = &p.doc;
    let ty = value_type(p);
    let optional = is_optional(p);
    let rest = is_rest(p);
    let (kind, long) = if p.flag {
        let long = p
            .explicit_long
            .clone()
            .unwrap_or_else(|| p.name.replace('_', "-"));
        let kind = if p.kind == TypeKind::Bool {
            quote!(Switch)
        } else {
            quote!(Flag)
        };
        (kind, quote!(::core::option::Option::Some(#long)))
    } else {
        (quote!(Positional), quote!(::core::option::Option::None))
    };
    let short = match p.short {
        Some(c) => quote!(::core::option::Option::Some(#c)),
        None => quote!(::core::option::Option::None),
    };
    let default_display = if p.default.is_some() {
        let display = default_display_fn(index);
        quote!(::core::option::Option::Some(#display))
    } else {
        quote!(::core::option::Option::None)
    };
    quote! {
        ::defcmd::ParamSpec {
            name: #name,
            kind: ::defcmd::ParamKind::#kind,
            long: #long,
            short: #short,
            doc: #doc,
            type_name: <#ty as ::defcmd::FromArg>::type_name,
            possible_values: <#ty as ::defcmd::FromArg>::possible_values,
            default_display: #default_display,
            optional: #optional,
            rest: #rest,
        }
    }
}

/// The glue statement that converts parameter `index` into a local named like the parameter.
fn convert(index: usize, p: &Param) -> TokenStream {
    let ident = &p.ident;
    let ty = &p.ty;
    if is_rest(p) {
        let inner = value_type(p);
        quote!(let #ident: #ty = __parsed.get_all::<#inner>(#index)?;)
    } else if p.kind == TypeKind::Option && !p.flag {
        let inner = value_type(p);
        quote!(let #ident: #ty = __parsed.get_opt::<#inner>(#index)?;)
    } else if p.default.is_some() {
        let make = default_fn(index);
        quote!(let #ident: #ty = __parsed.get_opt::<#ty>(#index)?.unwrap_or_else(#make);)
    } else if p.flag {
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
    expand(func, true, None)
}

/// Expands a subcommand: the function keeps its name and signature and is registered under `path`,
/// or under the name derived from the function name when there is none.
pub fn expand_sub(func: ItemFn, path: Option<Vec<String>>) -> syn::Result<TokenStream> {
    expand(func, false, path)
}

fn expand(mut func: ItemFn, root: bool, path: Option<Vec<String>>) -> syn::Result<TokenStream> {
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
    let path: Vec<String> = match path {
        _ if root => Vec::new(),
        Some(path) => path,
        None => vec![command_name(&func.sig.ident)],
    };
    for arg in &mut func.sig.inputs {
        if let FnArg::Typed(t) = arg {
            t.attrs.clear();
        }
    }

    let doc = &command.doc;
    let count = command.params.len();
    let specs = command
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| param_spec(i, p));
    let defaults = command
        .params
        .iter()
        .enumerate()
        .filter_map(|(i, p)| default_fns(i, p));
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

            #(#defaults)*

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

    fn rejected(f: ItemFn) -> String {
        let command = Command::from_fn(&f).unwrap();
        check_supported(&command).unwrap_err().to_string()
    }

    #[test]
    fn a_required_positional_cannot_follow_an_optional_one() {
        let f: ItemFn = parse_quote! { fn main(a: Option<String>, b: String) {} };
        assert_eq!(
            rejected(f),
            "a required positional cannot follow an optional one"
        );
        let f: ItemFn = parse_quote! { fn main(#[default(1)] a: u8, b: String) {} };
        assert_eq!(
            rejected(f),
            "a required positional cannot follow an optional one"
        );
    }

    #[test]
    fn default_cannot_be_combined_with_option() {
        let f: ItemFn = parse_quote! { fn main(#[default(None)] a: Option<String>) {} };
        assert_eq!(rejected(f), "`#[default]` cannot be combined with `Option`");
    }

    #[test]
    fn rest_rules() {
        let f: ItemFn = parse_quote! { fn main(a: Vec<String>, b: String) {} };
        assert_eq!(
            rejected(f),
            "the rest positional must be the last positional"
        );
        let f: ItemFn = parse_quote! { fn main(#[rest] a: Vec<String>, b: Option<String>) {} };
        assert_eq!(
            rejected(f),
            "the rest positional must be the last positional"
        );
        let f: ItemFn = parse_quote! { fn main(#[rest] a: String) {} };
        assert_eq!(rejected(f), "`#[rest]` needs a `Vec<T>`");
        let f: ItemFn = parse_quote! { fn main(#[rest] #[flag] a: Vec<String>) {} };
        assert_eq!(
            rejected(f),
            "`#[rest]` cannot be combined with `#[flag]` or `#[default]`"
        );
        let f: ItemFn = parse_quote! { fn main(#[rest] #[default(vec![])] a: Vec<String>) {} };
        assert_eq!(
            rejected(f),
            "`#[rest]` cannot be combined with `#[flag]` or `#[default]`"
        );
        let f: ItemFn = parse_quote! { fn main(#[default(vec![])] a: Vec<String>) {} };
        assert_eq!(
            rejected(f),
            "`#[default]` cannot be combined with a `Vec` positional"
        );
    }

    #[test]
    fn rest_after_optional_positionals_is_fine() {
        let f: ItemFn = parse_quote! { fn main(a: Option<String>, #[rest] b: Vec<String>) {} };
        assert!(check_supported(&Command::from_fn(&f).unwrap()).is_ok());
        let f: ItemFn = parse_quote! { fn main(a: String, b: Vec<u8>, #[flag] c: bool) {} };
        assert!(check_supported(&Command::from_fn(&f).unwrap()).is_ok());
    }

    #[test]
    fn optional_and_default_positionals_are_supported() {
        let f: ItemFn = parse_quote! {
            fn main(a: String, #[default(1)] b: u8, c: Option<String>, #[flag] d: bool) {}
        };
        assert!(check_supported(&Command::from_fn(&f).unwrap()).is_ok());
    }
}
