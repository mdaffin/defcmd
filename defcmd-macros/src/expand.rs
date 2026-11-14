//! Code generation for `#[command]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{Expr, ExprLit, FnArg, ItemFn, Lit, Type};

use crate::model::{
    Command, CommandAttr, LONG_NAME_RULE, Param, ParseAttr, TypeKind, classify, inner_type,
    valid_long_name,
};

/// Rejects parameter combinations that are not supported, so nothing is silently miscompiled.
fn check_supported(command: &Command) -> syn::Result<()> {
    let mut seen_optional = false;
    let mut seen_rest = false;
    for p in &command.params {
        if p.parse.is_some() && p.flag && (p.kind == TypeKind::Bool || is_tri_state(p)) {
            return Err(syn::Error::new_spanned(
                &p.ty,
                "`#[parse]` cannot be used on a switch",
            ));
        }
        if p.kind == TypeKind::Option
            && inner_type(&p.ty)
                .is_some_and(|t| matches!(classify(t), TypeKind::Option | TypeKind::Vec))
        {
            return Err(syn::Error::new_spanned(
                &p.ty,
                "`Option` of `Option` or `Vec` is not supported: use `Option<T>` or `Vec<T>`",
            ));
        }
        if p.flag {
            let long = p
                .explicit_long
                .clone()
                .unwrap_or_else(|| p.name.replace('_', "-"));
            if !valid_long_name(&long) {
                return Err(syn::Error::new_spanned(
                    &p.ident,
                    format!("the flag name `{long}` is not valid: {LONG_NAME_RULE}"),
                ));
            }
        }
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
            (_, TypeKind::Option, true) => {
                return Err(syn::Error::new_spanned(
                    &p.ty,
                    "`#[default]` cannot be combined with `Option`",
                ));
            }
            (true, TypeKind::Vec, true) => {
                return Err(syn::Error::new_spanned(
                    &p.ty,
                    "`#[default]` cannot be combined with a `Vec` flag",
                ));
            }
            (_, TypeKind::Option, false) => {}
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

/// A flag of type `Option<bool>`: a switch with three states.
fn is_tri_state(p: &Param) -> bool {
    p.flag
        && p.kind == TypeKind::Option
        && inner_type(&p.ty).is_some_and(|t| classify(t) == TypeKind::Bool)
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
        (TypeKind::Vec, Some(inner)) if is_rest(p) || p.flag => inner,
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
    let shown = if p.parse.is_some() {
        quote!(::defcmd::__private::display_parsed(&#make()))
    } else {
        quote!(::defcmd::FromArg::display_arg(&#make()))
    };
    Some(quote! {
        fn #make() -> #ty {
            #value
        }
        fn #display() -> ::core::option::Option<::std::string::String> {
            #shown
        }
    })
}

/// The function that converts a raw value to `T` (the value type), when `#[parse(..)]` replaces
/// `FromArg`.
fn converter(p: &Param) -> Option<TokenStream> {
    let ty = value_type(p);
    match p.parse.as_ref()? {
        ParseAttr::FromStr => Some(quote!(::defcmd::__private::from_str_arg::<#ty>)),
        ParseAttr::Function(path) => Some(quote!(::defcmd::__private::adapt(#path))),
    }
}

/// The `ParamSpec` expression for one parameter.
fn param_spec(index: usize, p: &Param) -> TokenStream {
    let name = &p.name;
    let doc = &p.doc;
    let ty = value_type(p);
    let optional = is_optional(p);
    let rest = is_rest(p);
    let repeatable = p.flag && p.kind == TypeKind::Vec;
    let negatable = is_tri_state(p) || (p.flag && p.kind == TypeKind::Bool && p.default.is_some());
    let (kind, long) = if p.flag {
        let long = p
            .explicit_long
            .clone()
            .unwrap_or_else(|| p.name.replace('_', "-"));
        let kind = if p.kind == TypeKind::Bool || is_tri_state(p) {
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
    let (type_name, possible_values) = if p.parse.is_some() {
        (
            quote!(::defcmd::__private::default_type_name::<#ty>),
            quote!(::defcmd::__private::no_values),
        )
    } else {
        (
            quote!(<#ty as ::defcmd::FromArg>::type_name),
            quote!(<#ty as ::defcmd::FromArg>::possible_values),
        )
    };
    quote! {
        ::defcmd::ParamSpec {
            name: #name,
            kind: ::defcmd::ParamKind::#kind,
            long: #long,
            short: #short,
            doc: #doc,
            type_name: #type_name,
            possible_values: #possible_values,
            default_display: #default_display,
            optional: #optional,
            rest: #rest,
            repeatable: #repeatable,
            negatable: #negatable,
        }
    }
}

/// The glue statement that converts parameter `index` into a local named like the parameter.
fn convert(index: usize, p: &Param) -> TokenStream {
    let ident = &p.ident;
    let ty = &p.ty;
    let inner = value_type(p);
    let conv = converter(p);
    // The getter call, `get_opt::<T>` or `get_opt_with(.., converter)`.
    let getter = |name: &str| {
        let plain = format_ident!("{name}");
        let with = format_ident!("{name}_with");
        match &conv {
            Some(conv) => quote!(__parsed.#with(#index, #conv)),
            None => quote!(__parsed.#plain::<#inner>(#index)),
        }
    };
    if is_rest(p) || (p.flag && p.kind == TypeKind::Vec) {
        let get = getter("get_all");
        quote!(let #ident: #ty = #get?;)
    } else if p.kind == TypeKind::Option {
        let get = getter("get_opt");
        quote!(let #ident: #ty = #get?;)
    } else if p.default.is_some() {
        let make = default_fn(index);
        let get = getter("get_opt");
        quote!(let #ident: #ty = #get?.unwrap_or_else(#make);)
    } else if p.flag {
        let get = getter("get_opt");
        quote!(let #ident: #ty = #get?.unwrap_or_default();)
    } else {
        let get = getter("get");
        quote!(let #ident: #ty = #get?;)
    }
}

/// The command name a bare `#[command]` gives a function: `_` shown as `-`, `r#` stripped.
pub fn command_name(ident: &syn::Ident) -> String {
    ident.unraw().to_string().replace('_', "-")
}

/// Expands the root command: `fn main` is renamed and the real entry point is emitted.
pub fn expand_main(func: ItemFn) -> syn::Result<TokenStream> {
    expand(func, true, CommandAttr::default())
}

/// Expands a subcommand: the function keeps its name and signature and is registered under `path`,
/// or under the name derived from the function name when there is none.
pub fn expand_sub(func: ItemFn, attr: CommandAttr) -> syn::Result<TokenStream> {
    expand(func, false, attr)
}

fn expand(mut func: ItemFn, root: bool, attr: CommandAttr) -> syn::Result<TokenStream> {
    let CommandAttr { path, aliases } = attr;
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
                aliases: &[#(#aliases),*],
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
    fn option_and_vec_flags_are_supported() {
        let f: ItemFn = parse_quote! {
            fn main(
                #[flag] a: Option<u8>,
                #[flag(-x)] b: Vec<String>,
                #[flag] c: Option<bool>,
                #[flag] #[default(true)] d: bool,
            ) {}
        };
        assert!(check_supported(&Command::from_fn(&f).unwrap()).is_ok());
    }

    #[test]
    fn flags_cannot_have_defaults_with_option_or_vec() {
        let f: ItemFn = parse_quote! { fn main(#[flag] #[default(None)] a: Option<u8>) {} };
        assert_eq!(rejected(f), "`#[default]` cannot be combined with `Option`");
        let f: ItemFn = parse_quote! { fn main(#[flag] #[default(vec![])] a: Vec<u8>) {} };
        assert_eq!(
            rejected(f),
            "`#[default]` cannot be combined with a `Vec` flag"
        );
    }

    #[test]
    fn parse_attribute_is_supported_but_not_on_switches() {
        let f: ItemFn = parse_quote! {
            fn main(
                #[parse(from_str)] a: u8,
                #[flag] #[parse(crate::h)] b: Option<u8>,
            ) {}
        };
        assert!(check_supported(&Command::from_fn(&f).unwrap()).is_ok());
        let f: ItemFn = parse_quote! { fn main(#[flag] #[parse(from_str)] a: bool) {} };
        assert_eq!(rejected(f), "`#[parse]` cannot be used on a switch");
        let f: ItemFn = parse_quote! { fn main(#[flag] #[parse(from_str)] a: Option<bool>) {} };
        assert_eq!(rejected(f), "`#[parse]` cannot be used on a switch");
    }

    #[test]
    fn optional_and_default_positionals_are_supported() {
        let f: ItemFn = parse_quote! {
            fn main(a: String, #[default(1)] b: u8, c: Option<String>, #[flag] d: bool) {}
        };
        assert!(check_supported(&Command::from_fn(&f).unwrap()).is_ok());
    }
}
