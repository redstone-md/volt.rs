use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{Attribute, Data, DeriveInput, Fields, GenericParam, Item, Type, parse_quote, parse2, spanned::Spanned};

use crate::serde_attr::{self, Container, Field};

const TYPE: &str = "::voltrs::__private::Type";

fn type_trait() -> TokenStream {
    TYPE.parse().unwrap_or_default()
}

pub fn model(args: TokenStream, item: TokenStream) -> TokenStream {
    if !args.is_empty() {
        return syn::Error::new(args.span(), "#[model] takes no arguments").into_compile_error();
    }
    let item: Item = match parse2(item) {
        Ok(item) => item,
        Err(error) => return error.into_compile_error(),
    };
    let attrs = match &item {
        Item::Struct(s) => &s.attrs,
        Item::Enum(e) => &e.attrs,
        other => {
            return syn::Error::new(other.span(), "#[model] applies to structs and enums").into_compile_error();
        }
    };
    let has_rule = Container::parse(attrs).map(|c| c.rename_all.is_some()).unwrap_or(false);
    let rename = (matches!(item, Item::Struct(_)) && !has_rule).then(|| quote!(#[serde(rename_all = "camelCase")]));
    quote! {
        #[derive(::voltrs::serde::Serialize, ::voltrs::serde::Deserialize, ::voltrs::Type)]
        #[serde(crate = "::voltrs::serde")]
        #rename
        #item
    }
}

pub fn derive(item: TokenStream) -> TokenStream {
    parse2::<DeriveInput>(item).and_then(|input| expand(&input)).unwrap_or_else(syn::Error::into_compile_error)
}

fn docs(attrs: &[Attribute]) -> Option<String> {
    let lines: Vec<String> = attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| match &a.meta {
            syn::Meta::NameValue(nv) => match &nv.value {
                syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Some(s.value().trim().to_owned()),
                _ => None,
            },
            _ => None,
        })
        .filter(|line| !line.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join(" ").replace("*/", "* /"))
}

fn is_option(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "Option"))
}

fn key(name: &str) -> String {
    let valid = name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$');
    if valid { name.to_owned() } else { format!("{name:?}") }
}

/// Builds an expression evaluating to a TypeScript object type for named fields.
/// A non-empty `indent` renders one property per line.
fn object(
    fields: &syn::FieldsNamed,
    rule: Option<&str>,
    indent: &str,
    extra: Option<String>,
) -> syn::Result<TokenStream> {
    let ts_trait = type_trait();
    let mut pushes = Vec::new();
    let mut flattened = Vec::new();
    if let Some(extra) = extra {
        let line = format!("{indent}{extra};");
        pushes.push(quote!(lines.push(::std::string::String::from(#line));));
    }
    for field in &fields.named {
        let attrs = Field::parse(&field.attrs)?;
        if attrs.skip {
            continue;
        }
        let ty = &field.ty;
        if attrs.flatten {
            flattened.push(quote!(<#ty as #ts_trait>::ts(ts)));
            continue;
        }
        let ident = field.ident.as_ref().map(ToString::to_string).unwrap_or_default();
        let name = attrs.rename.unwrap_or_else(|| serde_attr::rename(ident.trim_start_matches("r#"), rule));
        let optional = if attrs.optional || is_option(ty) { "?" } else { "" };
        let prop = format!("{indent}{}{optional}: ", key(&name));
        if let Some(doc) = docs(&field.attrs).filter(|_| !indent.is_empty()) {
            let line = format!("{indent}/** {doc} */");
            pushes.push(quote!(lines.push(::std::string::String::from(#line));));
        }
        pushes.push(quote!(lines.push(format!("{}{};", #prop, <#ty as #ts_trait>::ts(ts)));));
    }
    let multiline = !indent.is_empty();
    Ok(quote! {{
        let mut lines: ::std::vec::Vec<::std::string::String> = ::std::vec::Vec::new();
        #(#pushes)*
        let mut object = match (lines.is_empty(), #multiline) {
            (true, _) => ::std::string::String::from("{}"),
            (false, true) => format!("{{\n{}\n}}", lines.join("\n")),
            (false, false) => format!("{{ {} }}", lines.join(" ")),
        };
        #(object = format!("{} & {}", object, #flattened);)*
        object
    }})
}

fn tuple(fields: &syn::FieldsUnnamed) -> TokenStream {
    let ts_trait = type_trait();
    let types: Vec<&Type> = fields.unnamed.iter().map(|f| &f.ty).collect();
    if let [single] = types.as_slice() {
        return quote!(<#single as #ts_trait>::ts(ts));
    }
    quote! {{
        let items: ::std::vec::Vec<::std::string::String> = ::std::vec![#(<#types as #ts_trait>::ts(ts)),*];
        format!("[{}]", items.join(", "))
    }}
}

fn quoted(value: &str) -> String {
    format!("{value:?}")
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let container = Container::parse(&input.attrs)?;
    let ident = &input.ident;
    let name = container.rename.clone().unwrap_or_else(|| ident.to_string());
    let generic = input.generics.params.iter().any(|p| matches!(p, GenericParam::Type(_)));
    let rule = container.rename_all.as_deref();
    let doc = docs(&input.attrs).map(|d| format!("/** {d} */\n")).unwrap_or_default();

    let (body, interface) = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) if !container.transparent => {
                let has_flatten = fields.named.iter().any(|f| Field::parse(&f.attrs).is_ok_and(|a| a.flatten));
                let indent = if generic || has_flatten { "" } else { "  " };
                (object(fields, rule, indent, None)?, !generic && !has_flatten)
            }
            Fields::Named(fields) => {
                let ty =
                    fields.named.first().map(|f| &f.ty).ok_or_else(|| syn::Error::new(ident.span(), "empty struct"))?;
                let ts_trait = type_trait();
                (quote!(<#ty as #ts_trait>::ts(ts)), false)
            }
            Fields::Unnamed(fields) => (tuple(fields), false),
            Fields::Unit => (quote!(::std::string::String::from("null")), false),
        },
        Data::Enum(data) => (variants(data, &container)?, false),
        Data::Union(_) => return Err(syn::Error::new(ident.span(), "unions are not supported")),
    };

    let mut generics = input.generics.clone();
    for param in &mut generics.params {
        if let GenericParam::Type(param) = param {
            param.bounds.push(parse_quote!(::voltrs::__private::Type));
        }
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let ts_trait = type_trait();
    let describe = if generic {
        body
    } else if interface {
        let head = format!("{doc}export interface {name} ");
        quote!(ts.named(#name, |ts| format!("{}{}", #head, #body)))
    } else {
        let head = format!("{doc}export type {name} = ");
        quote!(ts.named(#name, |ts| format!("{}{};", #head, #body)))
    };
    let tokens = quote! {
        impl #impl_generics #ts_trait for #ident #ty_generics #where_clause {
            fn ts(ts: &mut ::voltrs::__private::TypeScript) -> ::std::string::String {
                #describe
            }
        }
    };
    Ok(tokens.into_token_stream())
}

fn variants(data: &syn::DataEnum, container: &Container) -> syn::Result<TokenStream> {
    let rule = container.rename_all.as_deref();
    let mut arms = Vec::new();
    for variant in &data.variants {
        let attrs = Field::parse(&variant.attrs)?;
        if attrs.skip {
            continue;
        }
        let tag_value = attrs.rename.unwrap_or_else(|| serde_attr::rename(&variant.ident.to_string(), rule));
        let literal = quoted(&tag_value);
        let payload: Option<TokenStream> = match &variant.fields {
            Fields::Unit => None,
            Fields::Unnamed(fields) => Some(tuple(fields)),
            Fields::Named(fields) => Some(object(fields, None, "", None)?),
        };
        let arm = match (&container.tag, &container.content, container.untagged) {
            (_, _, true) => payload.unwrap_or_else(|| quote!(::std::string::String::from("null"))),
            (Some(tag), Some(content), _) => {
                let head = format!("{{ {}: {literal}", key(tag));
                match payload {
                    Some(p) => {
                        let field = key(content);
                        quote!(format!("{}; {}: {} }}", #head, #field, #p))
                    }
                    None => quote!(format!("{} }}", #head)),
                }
            }
            (Some(tag), None, _) => {
                let tag_field = format!("{}: {literal}", key(tag));
                match (&variant.fields, payload) {
                    (Fields::Named(fields), _) => object(fields, None, "", Some(tag_field))?,
                    (_, Some(p)) => {
                        let head = format!("{{ {tag_field} }}");
                        quote!(format!("{} & {}", #head, #p))
                    }
                    (_, None) => {
                        let head = format!("{{ {tag_field} }}");
                        quote!(::std::string::String::from(#head))
                    }
                }
            }
            (None, _, false) => match payload {
                Some(p) => {
                    let head = format!("{{ {}: ", key(&tag_value));
                    quote!(format!("{}{} }}", #head, #p))
                }
                None => quote!(::std::string::String::from(#literal)),
            },
        };
        arms.push(arm);
    }
    if arms.is_empty() {
        return Ok(quote!(::std::string::String::from("never")));
    }
    Ok(quote! {{
        let parts: ::std::vec::Vec<::std::string::String> = ::std::vec![#(#arms),*];
        parts.join(" | ")
    }})
}
