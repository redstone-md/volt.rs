//! Procedural macros for volt.rs. Use them through the `voltrs` crate.

mod handler;
mod serde_attr;
mod ty;

use proc_macro::TokenStream;

/// Exposes an async function as a server action callable from React through `@volt/api`.
#[proc_macro_attribute]
pub fn action(args: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand(handler::Kind::Action, args.into(), item.into()).into()
}

/// Loads page data for a route: `#[loader("/blog/[slug]")]`.
#[proc_macro_attribute]
pub fn loader(args: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand(handler::Kind::Loader, args.into(), item.into()).into()
}

/// Registers a sitemap provider returning `Result<Vec<SitemapEntry>>`.
#[proc_macro_attribute]
pub fn sitemap(args: TokenStream, item: TokenStream) -> TokenStream {
    handler::expand(handler::Kind::Sitemap, args.into(), item.into()).into()
}

macro_rules! http_method {
    ($($name:ident => $method:literal),+) => {
        $(
            #[doc = concat!("Registers an API route for `", $method, "` requests.")]
            #[proc_macro_attribute]
            pub fn $name(args: TokenStream, item: TokenStream) -> TokenStream {
                handler::expand(handler::Kind::Api($method), args.into(), item.into()).into()
            }
        )+
    };
}

http_method!(get => "GET", post => "POST", put => "PUT", patch => "PATCH", delete => "DELETE");

/// Derives serde and TypeScript support with camelCase fields.
#[proc_macro_attribute]
pub fn model(args: TokenStream, item: TokenStream) -> TokenStream {
    ty::model(args.into(), item.into()).into()
}

/// Describes a type for the generated TypeScript SDK. Honors serde attributes.
#[proc_macro_derive(Type, attributes(serde))]
pub fn derive_type(item: TokenStream) -> TokenStream {
    ty::derive(item.into()).into()
}
