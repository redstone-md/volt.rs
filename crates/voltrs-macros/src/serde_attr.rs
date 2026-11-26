use syn::{Attribute, LitStr, Token, meta::ParseNestedMeta};

#[derive(Default)]
pub struct Container {
    pub rename: Option<String>,
    pub rename_all: Option<String>,
    pub tag: Option<String>,
    pub content: Option<String>,
    pub untagged: bool,
    pub transparent: bool,
}

#[derive(Default)]
pub struct Field {
    pub rename: Option<String>,
    pub skip: bool,
    pub optional: bool,
    pub flatten: bool,
}

fn skip_value(meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(Token![=]) {
        meta.value()?.parse::<syn::Expr>()?;
    } else if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        content.parse::<proc_macro2::TokenStream>()?;
    }
    Ok(())
}

fn string(meta: &ParseNestedMeta<'_>) -> syn::Result<Option<String>> {
    if meta.input.peek(Token![=]) {
        Ok(Some(meta.value()?.parse::<LitStr>()?.value()))
    } else {
        skip_value(meta)?;
        Ok(None)
    }
}

fn each(attrs: &[Attribute], mut f: impl FnMut(&ParseNestedMeta<'_>, &str) -> syn::Result<bool>) -> syn::Result<()> {
    for attr in attrs.iter().filter(|a| a.path().is_ident("serde")) {
        attr.parse_nested_meta(|meta| {
            let key = meta.path.get_ident().map(ToString::to_string).unwrap_or_default();
            if !f(&meta, &key)? {
                skip_value(&meta)?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

impl Container {
    pub fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut out = Self::default();
        each(attrs, |meta, key| {
            match key {
                "rename" => out.rename = string(meta)?,
                "rename_all" => out.rename_all = string(meta)?,
                "tag" => out.tag = string(meta)?,
                "content" => out.content = string(meta)?,
                "untagged" => out.untagged = true,
                "transparent" => out.transparent = true,
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(out)
    }
}

impl Field {
    pub fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut out = Self::default();
        each(attrs, |meta, key| {
            match key {
                "rename" => out.rename = string(meta)?,
                "skip" | "skip_serializing" => out.skip = true,
                "default" | "skip_serializing_if" => {
                    out.optional = true;
                    return Ok(false);
                }
                "flatten" => out.flatten = true,
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(out)
    }
}

/// Applies a serde `rename_all` rule to a Rust identifier.
pub fn rename(ident: &str, rule: Option<&str>) -> String {
    let words: Vec<String> = split_words(ident);
    match rule {
        Some("lowercase") => ident.to_lowercase(),
        Some("UPPERCASE") => ident.to_uppercase(),
        Some("camelCase") => {
            words.iter().enumerate().map(|(i, w)| if i == 0 { w.to_lowercase() } else { capitalize(w) }).collect()
        }
        Some("PascalCase") => words.iter().map(|w| capitalize(w)).collect(),
        Some("snake_case") => words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("_"),
        Some("SCREAMING_SNAKE_CASE") => words.iter().map(|w| w.to_uppercase()).collect::<Vec<_>>().join("_"),
        Some("kebab-case") => words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("-"),
        Some("SCREAMING-KEBAB-CASE") => words.iter().map(|w| w.to_uppercase()).collect::<Vec<_>>().join("-"),
        _ => ident.to_owned(),
    }
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}

fn split_words(ident: &str) -> Vec<String> {
    let ident = ident.trim_start_matches("r#");
    let mut words = Vec::new();
    let mut current = String::new();
    for c in ident.chars() {
        if c == '_' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else if c.is_uppercase() && !current.is_empty() && !current.chars().last().is_some_and(char::is_uppercase) {
            words.push(std::mem::take(&mut current));
            current.push(c);
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}
