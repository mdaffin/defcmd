//! A parsed view of a `#[command]` function: its doc comments and parameters.

use syn::ext::IdentExt;
use syn::{Attribute, Expr, ExprLit, FnArg, Ident, ItemFn, Lit, Meta, Pat, Type};

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
            for attr in &pat_type.attrs {
                if attr.path().is_ident("doc") {
                    continue;
                }
                if !attr.path().is_ident("flag") {
                    let name = attr
                        .path()
                        .get_ident()
                        .map_or_else(|| "?".to_string(), |i| i.to_string());
                    return Err(syn::Error::new_spanned(
                        attr,
                        format!("unknown parameter attribute `{name}`"),
                    ));
                }
                if !matches!(attr.meta, Meta::Path(_)) {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "`#[flag]` arguments are not supported yet",
                    ));
                }
                flag = true;
            }
            params.push(Param {
                ident: pat.ident.clone(),
                name: pat.ident.unraw().to_string(),
                doc: doc_of(&pat_type.attrs),
                flag,
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

    #[test]
    fn rejects_flag_with_arguments_for_now() {
        assert_eq!(
            error(parse_quote! { fn main(#[flag(-f)] x: bool) {} }),
            "`#[flag]` arguments are not supported yet"
        );
    }
}
