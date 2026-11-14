//! A parsed view of a `#[command]` function: its doc comments and parameters.

use syn::ext::IdentExt;
use syn::parse::ParseStream;
use syn::{Attribute, Expr, ExprLit, FnArg, Ident, ItemFn, Lit, LitInt, Meta, Pat, Token, Type};

/// How a parameter's type is treated, decided syntactically by the last path segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Bool,
    Option,
    Vec,
    Other,
}

pub struct Param {
    pub ident: Ident,
    /// The parameter name without `r#`.
    pub name: String,
    pub doc: String,
    /// Marked `#[flag]`.
    pub flag: bool,
    /// The long name given in `#[flag(--long)]`; the derived name is used when `None`.
    pub explicit_long: Option<String>,
    /// The short name given in `#[flag(-x)]`.
    pub short: Option<char>,
    /// The expression of `#[default(expr)]`.
    pub default: Option<Expr>,
    pub ty: Type,
    pub kind: TypeKind,
}

pub struct Command {
    pub doc: String,
    pub params: Vec<Param>,
}

/// The doc comment: each `#[doc]` line with one leading space removed, joined with `\n`, trimmed.
pub fn doc_of(attrs: &[Attribute]) -> String {
    let lines: Vec<String> = attrs
        .iter()
        .filter_map(|a| match &a.meta {
            Meta::NameValue(nv) if nv.path.is_ident("doc") => match &nv.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) => {
                    let text = s.value();
                    Some(text.strip_prefix(' ').unwrap_or(&text).to_string())
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    lines.join("\n").trim().to_string()
}

/// The kind of `ty`, by the last segment of its path.
pub fn classify(ty: &Type) -> TypeKind {
    let Type::Path(path) = ty else {
        return TypeKind::Other;
    };
    match path.path.segments.last() {
        Some(seg) if seg.ident == "bool" => TypeKind::Bool,
        Some(seg) if seg.ident == "Option" => TypeKind::Option,
        Some(seg) if seg.ident == "Vec" => TypeKind::Vec,
        _ => TypeKind::Other,
    }
}

/// The names given in `#[flag(..)]`.
#[derive(Default)]
struct FlagNames {
    long: Option<String>,
    short: Option<char>,
}

/// One word of a flag name: an identifier (keywords allowed) or an integer.
fn name_word(input: ParseStream) -> syn::Result<String> {
    if input.peek(LitInt) {
        let lit: LitInt = input.parse()?;
        Ok(lit.to_string())
    } else {
        Ok(Ident::parse_any(input)?.unraw().to_string())
    }
}

/// Parses the arguments of `#[flag(..)]`: `-x`, `--long` and `--long-name`, comma separated, at
/// most one of each. `-x` and `--long` are not Rust expressions, so the tokens are read by hand.
fn parse_flag_names(input: ParseStream) -> syn::Result<FlagNames> {
    let mut names = FlagNames::default();
    while !input.is_empty() {
        let first: Token![-] = input.parse()?;
        if input.peek(Token![-]) {
            input.parse::<Token![-]>()?;
            let start = input.span();
            let mut long = name_word(input)?;
            while input.peek(Token![-]) {
                input.parse::<Token![-]>()?;
                long.push('-');
                long.push_str(&name_word(input)?);
            }
            let valid = long.starts_with(|c: char| c.is_ascii_alphabetic())
                && long.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            if !valid {
                return Err(syn::Error::new(
                    start,
                    "a long flag name is ASCII letters, digits and hyphens, starting with a letter",
                ));
            }
            if names.long.replace(long).is_some() {
                return Err(syn::Error::new(start, "a flag has only one long name"));
            }
        } else {
            let start = input.span();
            let word = name_word(input)?;
            let mut chars = word.chars();
            let short = match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => c,
                _ => {
                    return Err(syn::Error::new(
                        start,
                        "a short flag name is exactly one ASCII letter or digit",
                    ));
                }
            };
            if names.short.replace(short).is_some() {
                return Err(syn::Error::new_spanned(
                    first,
                    "a flag has only one short name",
                ));
            }
        }
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }
    }
    Ok(names)
}

impl Command {
    pub fn from_fn(f: &ItemFn) -> syn::Result<Command> {
        let mut params = Vec::new();
        for arg in &f.sig.inputs {
            let FnArg::Typed(pat_type) = arg else {
                return Err(syn::Error::new_spanned(arg, "a command cannot take `self`"));
            };
            let Pat::Ident(pat) = &*pat_type.pat else {
                return Err(syn::Error::new_spanned(
                    &pat_type.pat,
                    "command parameters must be plain names",
                ));
            };
            let mut flag = false;
            let mut names = FlagNames::default();
            let mut default = None;
            for attr in &pat_type.attrs {
                if attr.path().is_ident("doc") {
                    continue;
                }
                if attr.path().is_ident("flag") {
                    if flag {
                        return Err(syn::Error::new_spanned(attr, "duplicate `#[flag]`"));
                    }
                    flag = true;
                    names = match &attr.meta {
                        Meta::Path(_) => FlagNames::default(),
                        Meta::List(_) => attr.parse_args_with(parse_flag_names)?,
                        Meta::NameValue(_) => {
                            return Err(syn::Error::new_spanned(
                                attr,
                                "use `#[flag]` or `#[flag(-x, --long)]`",
                            ));
                        }
                    };
                } else if attr.path().is_ident("default") {
                    if default.is_some() {
                        return Err(syn::Error::new_spanned(attr, "duplicate `#[default]`"));
                    }
                    let Meta::List(_) = &attr.meta else {
                        return Err(syn::Error::new_spanned(
                            attr,
                            "`#[default]` needs a value: `#[default(expr)]`",
                        ));
                    };
                    default = Some(attr.parse_args::<Expr>()?);
                } else {
                    let name = attr
                        .path()
                        .get_ident()
                        .map_or_else(|| "?".to_string(), |i| i.to_string());
                    return Err(syn::Error::new_spanned(
                        attr,
                        format!("unknown parameter attribute `{name}`"),
                    ));
                }
            }
            params.push(Param {
                ident: pat.ident.clone(),
                name: pat.ident.unraw().to_string(),
                doc: doc_of(&pat_type.attrs),
                flag,
                explicit_long: names.long,
                short: names.short,
                default,
                ty: (*pat_type.ty).clone(),
                kind: classify(&pat_type.ty),
            });
        }
        Ok(Command {
            doc: doc_of(&f.attrs),
            params,
        })
    }
}

/// The words of `#[command("remote add")]`: one per level, relative to the root. Each must be
/// non-empty, free of whitespace, must not start with `-` and must not be `main`.
pub fn parse_path(lit: &syn::LitStr) -> syn::Result<Vec<String>> {
    let text = lit.value();
    let bad = |why: &str| Err(syn::Error::new(lit.span(), format!("command path {why}")));
    if text.trim().is_empty() {
        return bad("must not be empty");
    }
    // Split on single spaces so that doubled spaces show up as empty words.
    let words: Vec<String> = text.split(' ').map(str::to_string).collect();
    for word in &words {
        if word.is_empty() {
            return bad("must have exactly one space between words and none at the ends");
        }
        if word.chars().any(char::is_whitespace) {
            return bad("words must be separated by single spaces only");
        }
        if word.starts_with('-') {
            return bad(&format!("word `{word}` must not start with `-`"));
        }
        if word == "main" {
            return bad("must not contain `main`: the root is implied, so `main` is not a word");
        }
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn command(f: ItemFn) -> Command {
        Command::from_fn(&f).unwrap()
    }

    fn error(f: ItemFn) -> String {
        match Command::from_fn(&f) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn doc_lines_lose_one_leading_space_and_are_joined() {
        let f: ItemFn = parse_quote! {
            /// Print a greeting.
            ///
            /// Longer text,
            ///   indented.
            fn main() {}
        };
        assert_eq!(
            doc_of(&f.attrs),
            "Print a greeting.\n\nLonger text,\n  indented."
        );
    }

    #[test]
    fn no_doc_is_empty() {
        let f: ItemFn = parse_quote! { fn main() {} };
        assert_eq!(doc_of(&f.attrs), "");
    }

    #[test]
    fn non_doc_attributes_are_not_part_of_the_doc() {
        let f: ItemFn = parse_quote! {
            /// Text.
            #[allow(dead_code)]
            fn main() {}
        };
        assert_eq!(doc_of(&f.attrs), "Text.");
    }

    #[test]
    fn classifies_by_last_path_segment() {
        let kind = |t: Type| classify(&t);
        assert_eq!(kind(parse_quote!(bool)), TypeKind::Bool);
        assert_eq!(kind(parse_quote!(Option<String>)), TypeKind::Option);
        assert_eq!(
            kind(parse_quote!(std::option::Option<u8>)),
            TypeKind::Option
        );
        assert_eq!(kind(parse_quote!(Vec<String>)), TypeKind::Vec);
        assert_eq!(kind(parse_quote!(String)), TypeKind::Other);
        assert_eq!(kind(parse_quote!(std::path::PathBuf)), TypeKind::Other);
        assert_eq!(kind(parse_quote!(&str)), TypeKind::Other);
        // Aliases are not recognised.
        assert_eq!(kind(parse_quote!(Maybe<u8>)), TypeKind::Other);
    }

    #[test]
    fn collects_params_with_docs_and_flags() {
        let c = command(parse_quote! {
            /// Print a greeting.
            fn main(
                /// Who to greet.
                name: String,
                /// Print in upper case.
                #[flag]
                shout: bool,
                times: Option<u8>,
            ) {}
        });
        assert_eq!(c.doc, "Print a greeting.");
        assert_eq!(c.params.len(), 3);
        let p = &c.params[0];
        assert_eq!(
            (p.name.as_str(), p.doc.as_str(), p.flag),
            ("name", "Who to greet.", false)
        );
        assert_eq!(p.kind, TypeKind::Other);
        let p = &c.params[1];
        assert_eq!(
            (p.name.as_str(), p.doc.as_str(), p.flag),
            ("shout", "Print in upper case.", true)
        );
        assert_eq!(p.kind, TypeKind::Bool);
        let p = &c.params[2];
        assert_eq!(
            (p.doc.as_str(), p.flag, p.kind),
            ("", false, TypeKind::Option)
        );
    }

    #[test]
    fn raw_identifiers_lose_the_prefix() {
        let c = command(parse_quote! { fn main(r#type: String) {} });
        assert_eq!(c.params[0].name, "type");
    }

    #[test]
    fn mut_bindings_are_fine() {
        let c = command(parse_quote! { fn main(mut x: String) {} });
        assert_eq!(c.params[0].name, "x");
    }

    #[test]
    fn rejects_self() {
        assert_eq!(
            error(parse_quote! { fn main(&self) {} }),
            "a command cannot take `self`"
        );
    }

    #[test]
    fn rejects_patterns_that_are_not_names() {
        assert_eq!(
            error(parse_quote! { fn main((a, b): (u8, u8)) {} }),
            "command parameters must be plain names"
        );
    }

    #[test]
    fn rejects_unknown_parameter_attributes() {
        assert_eq!(
            error(parse_quote! { fn main(#[bogus] x: String) {} }),
            "unknown parameter attribute `bogus`"
        );
    }

    fn names(f: ItemFn) -> (Option<String>, Option<char>) {
        let p = &command(f).params[0];
        (p.explicit_long.clone(), p.short)
    }

    #[test]
    fn flag_names_in_every_form() {
        let none = (None, None);
        assert_eq!(names(parse_quote! { fn main(#[flag] x: bool) {} }), none);
        assert_eq!(
            names(parse_quote! { fn main(#[flag(-f)] x: bool) {} }),
            (None, Some('f'))
        );
        assert_eq!(
            names(parse_quote! { fn main(#[flag(--jobs)] x: bool) {} }),
            (Some("jobs".into()), None)
        );
        assert_eq!(
            names(parse_quote! { fn main(#[flag(--jobs, -j)] x: bool) {} }),
            (Some("jobs".into()), Some('j'))
        );
        assert_eq!(
            names(parse_quote! { fn main(#[flag(-j, --jobs,)] x: bool) {} }),
            (Some("jobs".into()), Some('j'))
        );
    }

    #[test]
    fn hyphenated_and_keyword_long_names() {
        let long = |f: ItemFn| names(f).0.unwrap();
        assert_eq!(
            long(parse_quote! { fn main(#[flag(--dry-run)] x: bool) {} }),
            "dry-run"
        );
        assert_eq!(
            long(parse_quote! { fn main(#[flag(--no-color-at-all)] x: bool) {} }),
            "no-color-at-all"
        );
        assert_eq!(
            long(parse_quote! { fn main(#[flag(--type)] x: bool) {} }),
            "type"
        );
        assert_eq!(
            long(parse_quote! { fn main(#[flag(--utf-8)] x: bool) {} }),
            "utf-8"
        );
    }

    #[test]
    fn bad_flag_names_are_errors() {
        let long = "a long flag name is ASCII letters, digits and hyphens, starting with a letter";
        let short = "a short flag name is exactly one ASCII letter or digit";
        assert_eq!(
            error(parse_quote! { fn main(#[flag(-ab)] x: bool) {} }),
            short
        );
        assert_eq!(
            error(parse_quote! { fn main(#[flag(-é)] x: bool) {} }),
            short
        );
        assert_eq!(
            error(parse_quote! { fn main(#[flag(--é)] x: bool) {} }),
            long
        );
        assert_eq!(
            error(parse_quote! { fn main(#[flag(--r#a_b)] x: bool) {} }),
            long
        );
        assert_eq!(
            error(parse_quote! { fn main(#[flag(--9a)] x: bool) {} }),
            long
        );
        assert_eq!(
            error(parse_quote! { fn main(#[flag(-a, -b)] x: bool) {} }),
            "a flag has only one short name"
        );
        assert_eq!(
            error(parse_quote! { fn main(#[flag(--a, --b)] x: bool) {} }),
            "a flag has only one long name"
        );
        assert_eq!(
            error(parse_quote! { fn main(#[flag(-a --b)] x: bool) {} }),
            "expected `,`"
        );
        assert_eq!(
            error(parse_quote! { fn main(#[flag(a)] x: bool) {} }),
            "expected `-`"
        );
        assert_eq!(
            error(parse_quote! { fn main(#[flag = "x"] x: bool) {} }),
            "use `#[flag]` or `#[flag(-x, --long)]`"
        );
        assert_eq!(
            error(parse_quote! { fn main(#[flag] #[flag] x: bool) {} }),
            "duplicate `#[flag]`"
        );
    }

    #[test]
    fn default_keeps_its_expression() {
        let c = command(parse_quote! {
            fn main(#[flag] #[default(1 + 2)] n: u8, #[default("x")] s: String, t: String) {}
        });
        let shown = |p: &Param| p.default.as_ref().map(|e| quote::quote!(#e).to_string());
        assert_eq!(shown(&c.params[0]).as_deref(), Some("1 + 2"));
        assert_eq!(shown(&c.params[1]).as_deref(), Some("\"x\""));
        assert!(c.params[2].default.is_none());
    }

    #[test]
    fn bad_default_attributes_are_errors() {
        assert_eq!(
            error(parse_quote! { fn main(#[default] x: u8) {} }),
            "`#[default]` needs a value: `#[default(expr)]`"
        );
        assert_eq!(
            error(parse_quote! { fn main(#[default = 1] x: u8) {} }),
            "`#[default]` needs a value: `#[default(expr)]`"
        );
        assert_eq!(
            error(parse_quote! { fn main(#[default(1)] #[default(2)] x: u8) {} }),
            "duplicate `#[default]`"
        );
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    fn path(s: &str) -> Result<Vec<String>, String> {
        parse_path(&syn::LitStr::new(s, proc_macro2::Span::call_site())).map_err(|e| e.to_string())
    }

    #[test]
    fn words_are_split_on_spaces() {
        assert_eq!(path("add").unwrap(), ["add"]);
        assert_eq!(path("remote add").unwrap(), ["remote", "add"]);
        assert_eq!(path("a-b c_d").unwrap(), ["a-b", "c_d"]);
    }

    #[test]
    fn bad_paths_are_errors() {
        for bad in [
            "",
            " ",
            "a  b",
            " a",
            "a ",
            "a\tb",
            "a\nb",
            "-x",
            "a -x",
            "main",
            "remote main",
        ] {
            assert!(path(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(path("main").unwrap_err().contains("main"));
        assert!(path("a -x").unwrap_err().contains("-x"));
    }
}
